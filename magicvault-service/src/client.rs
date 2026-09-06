//! Shared CLI/MCP client. No raw material or human-grant method exists.
use std::{fs::OpenOptions, io::Write, path::{Path, PathBuf}};
use magicvault_protocol::*;
use magicvault_primitives::durable_io::sync_parent_dir_blocking;
use uuid::Uuid;
use zeroize::Zeroizing;
use crate::{ipc, storage};

pub struct Client { root: PathBuf, pairing: Option<Pairing> }
impl Client {
    pub fn load(root: PathBuf, profile: &str) -> Result<Self, ErrorCode> {
        if !valid_name(profile) { return Err(ErrorCode::InvalidRequest); }
        storage::inspect_instance(&root)?;
        let path = profile_path(&root, profile);
        let pairing = match path.symlink_metadata() {
          Ok(_) => {
            let bytes = storage::read_private(&path, 4096)?;
            let pairing: Pairing = serde_json::from_slice(&bytes).map_err(|_| ErrorCode::Unavailable)?;
            if pairing.token.len() != 64 || !pairing.token.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(ErrorCode::Unavailable); }
            Some(pairing)
          },
          Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
          Err(_) => return Err(ErrorCode::Unavailable),
        };
        Ok(Self { root, pairing })
    }

    pub fn unpaired(root: PathBuf) -> Result<Self, ErrorCode> {
        storage::inspect_instance(&root)?;
        Ok(Self { root, pairing: None })
    }

    pub async fn status(&self) -> Result<ServiceStatus, ErrorCode> {
        match self.send(Request::Status, None, Uuid::new_v4()).await? {
            Response::Status(status) => Ok(status), _ => Err(ErrorCode::TransportUncertain),
        }
    }

    pub async fn call(&self, request: Request) -> Result<Response, ErrorCode> {
        request.validate()?;
        let status = self.status().await?;
        if !status.ready { return Err(ErrorCode::Unavailable); }
        self.send(request, Some(status.epoch), Uuid::new_v4()).await
    }

    async fn send(&self, request: Request, epoch: Option<Uuid>, request_id: Uuid) -> Result<Response, ErrorCode> {
        let envelope = Envelope { version: VERSION, request_id, epoch, token: self.pairing.as_ref().map(|p| p.token.clone()), request };
        match ipc::exchange(&self.root.join("rpc.sock"), &envelope).await? {
            Reply::Ok(response) => Ok(response), Reply::Error(error) => Err(error),
        }
    }

    /// Only the human CLI calls pairing. The returned capability is written to
    /// an exclusive private file, never printed or included in MCP responses.
    pub async fn pair(root: PathBuf, profile: &str, label: String) -> Result<Uuid, ErrorCode> {
        if !valid_name(profile) { return Err(ErrorCode::InvalidRequest); }
        let path = profile_path(&root, profile);
        if path.symlink_metadata().is_ok() { return Err(ErrorCode::Conflict); }
        let client = Self::unpaired(root)?;
        let Response::Paired(pairing) = client.call(Request::Pair(PairRequest { label })).await? else { return Err(ErrorCode::TransportUncertain); };
        let bytes = Zeroizing::new(serde_json::to_vec(&pairing).map_err(|_| ErrorCode::Unavailable)?);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC); }
        let mut file = options.open(&path).map_err(|_| ErrorCode::PersistenceUncertain)?;
        // A partial file is deliberately retained on failure and refused on
        // reload. Do not overwrite it or automatically create another pairing.
        file.write_all(&bytes).and_then(|_| file.sync_all()).map_err(|_| ErrorCode::PersistenceUncertain)?;
        sync_parent_dir_blocking(&path).map_err(|_| ErrorCode::PersistenceUncertain)?;
        Ok(pairing.client_id)
    }
}

fn profile_path(root: &Path, profile: &str) -> PathBuf { root.join(format!("client-{profile}.json")) }
