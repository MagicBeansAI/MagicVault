//! Trusted delivery material, intentionally not serializable or debug-printable.
//! Only closed outcomes leave these adapters; recipients' output is withheld.
use magicvault_protocol::{DeliveryState, ErrorCode, InputValue};
use std::collections::BTreeMap;
use zeroize::Zeroizing;

#[derive(Default)]
pub struct DeliveryMaterial {
    values: BTreeMap<(String, String), Zeroizing<String>>,
}
impl DeliveryMaterial {
    pub fn insert(&mut self, reference: String, field: String, value: Zeroizing<String>) {
        self.values.insert((reference, field), value);
    }
    pub(crate) fn render(&self, input: &InputValue) -> Result<Zeroizing<String>, ErrorCode> {
        match input {
            InputValue::Literal { value } => Ok(Zeroizing::new(value.clone())),
            InputValue::Credential {
                credential_ref,
                credential_field,
                prefix,
                suffix,
            } => {
                let value = self
                    .values
                    .get(&(credential_ref.clone(), credential_field.clone()))
                    .ok_or(ErrorCode::Denied)?;
                if value.is_empty() || value.len() > 4096 {
                    return Err(ErrorCode::InvalidRequest);
                }
                let mut result = Zeroizing::new(String::with_capacity(
                    prefix.len() + value.len() + suffix.len(),
                ));
                result.push_str(prefix);
                result.push_str(value);
                result.push_str(suffix);
                Ok(result)
            }
        }
    }
}

pub struct DeliveryOutcome {
    pub state: DeliveryState,
    pub may_have_run: bool,
    pub error: Option<ErrorCode>,
}
impl DeliveryOutcome {
    pub fn failed(error: ErrorCode) -> Self {
        let state = match error {
            ErrorCode::Denied | ErrorCode::Unauthorized | ErrorCode::PermissionDenied => {
                DeliveryState::Denied
            }
            ErrorCode::Cancelled => DeliveryState::Cancelled,
            ErrorCode::Expired => DeliveryState::Expired,
            _ => DeliveryState::Failed,
        };
        Self {
            state,
            may_have_run: false,
            error: Some(error),
        }
    }
    pub fn uncertain(error: ErrorCode) -> Self {
        Self {
            state: DeliveryState::Uncertain,
            may_have_run: true,
            error: Some(error),
        }
    }
    pub fn completed() -> Self {
        Self {
            state: DeliveryState::Completed,
            may_have_run: true,
            error: None,
        }
    }
    pub fn after_dispatch(mut self) -> Self {
        self.may_have_run = true;
        self
    }
}
