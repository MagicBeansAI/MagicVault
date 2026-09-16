//! Bounded private pipe contract. This crate has no vault, network or agent API.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

pub const MAX_MESSAGE_BYTES: usize = 16 * 1024;
pub const MAX_REQUEST_BYTES: usize = MAX_MESSAGE_BYTES * 6 + 256;
pub const MAX_SECRET_BYTES: usize = 4096;
pub const DETAILS_SEPARATOR: &str = "\n\nRequest details\n";

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Confirm,
    Secret,
    Use,
    SecretOnce,
    ConfirmOnce,
}
impl Kind {
    pub fn is_input(self) -> bool {
        matches!(self, Self::Secret | Self::SecretOnce)
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Confirm => "Review request",
            Self::Secret => "Save to your vault",
            Self::Use => "Use saved credentials",
            Self::SecretOnce => "Enter a one-time value",
            Self::ConfirmOnce => "Ready to fill once?",
        }
    }
    pub fn action(self) -> &'static str {
        match self {
            Self::Confirm => "Allow",
            Self::Secret | Self::SecretOnce => "Continue",
            Self::Use => "Allow once",
            Self::ConfirmOnce => "Use once",
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    version: u8,
    pub kind: Kind,
    pub message: String,
}
impl Prompt {
    pub fn new(kind: Kind, message: String) -> io::Result<Self> {
        let prompt = Self {
            version: 1,
            kind,
            message,
        };
        prompt.validate()?;
        Ok(prompt)
    }
    fn validate(&self) -> io::Result<()> {
        if self.version != 1 || self.message.is_empty() || self.message.len() > MAX_MESSAGE_BYTES {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn sections(&self) -> (&str, Option<&str>) {
        match self.message.split_once(DETAILS_SEPARATOR) {
            Some((summary, details)) => (summary, Some(details)),
            None => (&self.message, None),
        }
    }
    pub fn write(&self, mut output: impl Write) -> io::Result<()> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        output.write_all(&(bytes.len() as u32).to_be_bytes())?;
        output.write_all(&bytes)?;
        output.flush()
    }
    pub fn read(mut input: impl Read) -> io::Result<Self> {
        let mut length = [0; 4];
        input.read_exact(&mut length)?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_REQUEST_BYTES {
            return Err(invalid());
        }
        let mut bytes = vec![0; length];
        input.read_exact(&mut bytes)?;
        let prompt: Self = serde_json::from_slice(&bytes)?;
        prompt.validate()?;
        Ok(prompt)
    }
}

// Never derive Debug or Serialize for replies. Only the owning daemon's pipe
// receives the value, as tag + exact UTF-8 bytes, without line normalization.
pub enum Reply {
    Deny,
    Allow,
    Always,
    Secret(Zeroizing<String>),
}
impl Reply {
    pub fn encode(&self) -> Zeroizing<Vec<u8>> {
        match self {
            Self::Deny => Zeroizing::new(vec![0]),
            Self::Allow => Zeroizing::new(vec![1]),
            Self::Always => Zeroizing::new(vec![2]),
            Self::Secret(value) => {
                let mut bytes = Zeroizing::new(Vec::with_capacity(value.len() + 1));
                bytes.push(3);
                bytes.extend_from_slice(value.as_bytes());
                bytes
            }
        }
    }
    pub fn decode(kind: Kind, bytes: &[u8]) -> io::Result<Self> {
        match bytes {
            [0] => Ok(Self::Deny),
            [1] if !kind.is_input() => Ok(Self::Allow),
            [2] if kind == Kind::Use => Ok(Self::Always),
            [3, value @ ..] if kind.is_input() => {
                let text = std::str::from_utf8(value).map_err(|_| invalid())?;
                if !valid_secret(text) {
                    return Err(invalid());
                }
                Ok(Self::Secret(Zeroizing::new(text.to_owned())))
            }
            _ => Err(invalid()),
        }
    }
}
pub fn valid_secret(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_SECRET_BYTES && !value.contains(['\r', '\n', '\0'])
}
fn invalid() -> io::Error {
    io::Error::from(io::ErrorKind::InvalidData)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requests_are_bounded_versioned_and_preserve_exact_details() {
        let message = format!("Site: https://login.example:8443{DETAILS_SEPARATOR}Frame: https://frame.example\n\"password\" -> \"#pass\\\\word\"");
        let prompt = Prompt::new(Kind::SecretOnce, message.clone()).unwrap();
        let mut bytes = Vec::new();
        prompt.write(&mut bytes).unwrap();
        let result = Prompt::read(bytes.as_slice()).unwrap();
        assert_eq!(result.message, message);
        assert_eq!(result.sections().1, prompt.sections().1);
        assert!(Prompt::read(&[255, 255, 255, 255][..]).is_err());
        assert!(Prompt::read(&bytes[..bytes.len() - 1]).is_err());
        assert!(Prompt::new(Kind::Confirm, "x".repeat(MAX_MESSAGE_BYTES + 1)).is_err());
        for json in [
            r#"{"version":2,"kind":"confirm","message":"test"}"#,
            r#"{"version":1,"kind":"confirm","message":"test","allow":true}"#,
        ] {
            let mut wire = (json.len() as u32).to_be_bytes().to_vec();
            wire.extend_from_slice(json.as_bytes());
            assert!(Prompt::read(wire.as_slice()).is_err());
        }
    }
    #[test]
    fn reply_cannot_broaden_decision_or_smuggle_values() {
        for kind in [
            Kind::Confirm,
            Kind::Secret,
            Kind::SecretOnce,
            Kind::ConfirmOnce,
        ] {
            assert!(Reply::decode(kind, &[2]).is_err());
        }
        assert!(Reply::decode(Kind::Use, &[2]).is_ok());
        assert!(Reply::decode(Kind::ConfirmOnce, b"\x01secret").is_err());
        assert!(Reply::decode(Kind::Confirm, b"\x03secret").is_err());
        for bytes in [b"\x03".as_slice(), b"\x03line\nline", b"\x03\xff", b"\x01"] {
            assert!(Reply::decode(Kind::SecretOnce, bytes).is_err());
        }
        let reply = Reply::Secret(Zeroizing::new("  synthetic value  ".into()));
        let bytes = reply.encode();
        match Reply::decode(Kind::SecretOnce, &bytes).unwrap() {
            Reply::Secret(value) => assert_eq!(value.as_str(), "  synthetic value  "),
            _ => panic!("wrong reply variant"),
        }
        assert!(Reply::decode(Kind::SecretOnce, &vec![3; MAX_SECRET_BYTES + 2]).is_err());
    }
}
