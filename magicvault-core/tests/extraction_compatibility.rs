//! Synthetic baseline-wire characterization. Do not replace the legacy codec
//! with MagicVault's encryption helpers: that would test the candidate against
//! itself and miss a nonce/framing/AAD regression during extraction.
use std::{collections::HashMap, path::Path};

use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use magicvault_core::{
    encryption::{MasterKeyProvider, SecretEncryptionError},
    policy::SecretPolicy,
    store::{AuditEvent, AuditReceipt, SecretStore},
    InjectionTarget,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const FIXTURE_KEY: [u8; 32] = [0x53; 32];
const VALUE: &str = "SYNTHETIC-EXTRACTION-CANARY-NOT-A-CREDENTIAL";
const RECORD_ID: &str = "credentials:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct FixtureKey;
impl MasterKeyProvider for FixtureKey {
    fn get_or_create_key(&self) -> Result<[u8; 32], SecretEncryptionError> {
        Ok(FIXTURE_KEY)
    }
    fn delete_key(&self) -> Result<(), SecretEncryptionError> { Ok(()) }
    fn provider_name(&self) -> &str { "synthetic-extraction-fixture" }
}

// Baseline aef928c: 12-byte nonce || AES-256-GCM ciphertext+tag, empty AAD.
// Each fixture write uses its own nonce. Fixed nonces belong ONLY in this
// disposable synthetic compatibility harness, never in a production writer.
fn baseline_write(path: &Path, value: &Value, nonce: [u8; 12]) {
    let cipher = Aes256Gcm::new_from_slice(&FIXTURE_KEY).unwrap();
    let body = serde_json::to_vec(value).unwrap();
    let encrypted = cipher.encrypt(&Nonce::from(nonce), body.as_slice()).unwrap();
    let mut bytes = nonce.to_vec();
    bytes.extend_from_slice(&encrypted);
    std::fs::write(path, bytes).unwrap();
}

fn baseline_read(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() >= 28, "nonce and authentication tag are present");
    let (nonce, encrypted) = bytes.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(&FIXTURE_KEY).unwrap();
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let body = cipher.decrypt(&Nonce::from(nonce), encrypted).unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn provisioned_entry() -> Value {
    json!({
        "id": "fixture", "label": "Fixture", "fields": {"value": VALUE},
        "source": "Provisioned",
        "injection": {"Header": {"name": "Authorization", "prefix": "Bearer "}},
        "policy": {"allowed_tools": [], "allowed_domains": [], "requires_approval": false},
        "created_at": 1234
    })
}

#[test]
fn candidate_reads_baseline_provisioned_captured_and_oauth_wire() {
    let temp = tempfile::tempdir().unwrap();
    baseline_write(&temp.path().join("provisioned_secrets.vault"), &json!({
        "version": 1, "entries": {"fixture": provisioned_entry()}
    }), [1; 12]);
    baseline_write(&temp.path().join("captured_secrets.vault"), &json!({
        "version": 1, "entries": {"https://example.test": {
            "id": "https://example.test", "label": "Captured fixture",
            "fields": {"header:Authorization": VALUE},
            "source": {"Captured": {
                "origin": "https://example.test", "generation": 7,
                "stale": false, "expires_hint": null
            }},
            "injection": {"Cookies": []}, "created_at": 1234
        }}
    }), [2; 12]);
    let document = br#"{"access_token":"SYNTHETIC-OAUTH-CANARY"}"#;
    baseline_write(&temp.path().join("mcp_oauth.vault"), &json!({
        "version": 1, "records": {(RECORD_ID): STANDARD.encode(document)}
    }), [3; 12]);

    let store = SecretStore::new(Box::new(FixtureKey), temp.path().to_path_buf()).unwrap();
    let entry = store.get_provisioned("fixture").unwrap();
    assert_eq!(serde_json::to_value(entry).unwrap(), provisioned_entry());
    let (session, lease) = store.get_session("https://example.test", "https://example.test/path").unwrap();
    assert_eq!(session.auth_headers["Authorization"], VALUE);
    assert_eq!(lease.stale_mark_targets()[0].generation, 7);
    assert_eq!(store.read_mcp_oauth(RECORD_ID).unwrap().unwrap(), document);
}

#[test]
fn baseline_codec_reads_candidate_writes_without_a_storage_migration() {
    let temp = tempfile::tempdir().unwrap();
    let store = SecretStore::new_empty(Box::new(FixtureKey), temp.path().to_path_buf());
    store.store_provisioned("fixture", "Fixture", HashMap::from([("value".into(), VALUE.into())]),
        InjectionTarget::Header { name: "Authorization".into(), prefix: Some("Bearer ".into()) },
        SecretPolicy::default()).unwrap();
    store.store_captured("https://example.test", HashMap::from([("Authorization".into(), VALUE.into())]),
        Vec::new(), HashMap::new(), HashMap::new()).unwrap();
    let document = br#"{"access_token":"SYNTHETIC-OAUTH-CANARY"}"#;
    store.write_mcp_oauth(RECORD_ID, document, true).unwrap();

    let provisioned = baseline_read(&temp.path().join("provisioned_secrets.vault"));
    assert_eq!(provisioned["version"], 1);
    let mut entry = provisioned["entries"]["fixture"].clone();
    assert!(entry["created_at"].as_i64().is_some());
    entry["created_at"] = json!(1234);
    assert_eq!(entry, provisioned_entry());
    let captured = baseline_read(&temp.path().join("captured_secrets.vault"));
    assert_eq!(captured["version"], 1);
    assert_eq!(captured["entries"]["https://example.test"]["fields"]["header:Authorization"], VALUE);
    assert_eq!(captured["entries"]["https://example.test"]["source"]["Captured"]["generation"], 1);
    let oauth = baseline_read(&temp.path().join("mcp_oauth.vault"));
    assert_eq!(oauth, json!({"version": 1, "records": {(RECORD_ID): STANDARD.encode(document)}}));
}

// Deliberately no Default implementation: absent extension metadata must not
// make downstream applications invent a receipt just to deserialize an event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct MetadataReceipt { call_id: String }
impl AuditReceipt for MetadataReceipt {}

#[test]
fn typed_audit_extension_preserves_the_existing_optional_wire() {
    let legacy = json!({"timestamp": 1234, "event": "approval_granted"});
    let event: AuditEvent<MetadataReceipt> = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(event.runtime_credential_receipt(), None);
    assert_eq!(serde_json::to_value(&event).unwrap(), legacy);
    let event = event.with_runtime_credential_receipt(MetadataReceipt { call_id: "fixture-call".into() });
    let temp = tempfile::tempdir().unwrap();
    let store = SecretStore::new_empty(Box::new(FixtureKey), temp.path().to_path_buf());
    store.try_audit_event(event).unwrap();
    let journal = std::fs::read_to_string(temp.path().join("secret_audit.jsonl")).unwrap();
    assert!(journal.ends_with('\n'));
    assert_eq!(journal.lines().count(), 1);
    assert_eq!(serde_json::from_str::<Value>(&journal).unwrap(), json!({
        "timestamp": 1234, "event": "approval_granted",
        "runtime_credential_receipt": {"call_id": "fixture-call"}
    }));
}
