use std::collections::HashMap;
use magicvault_core::{InMemoryKeyProvider, InjectionTarget, policy::SecretPolicy, store::SecretStore};

#[test]
fn metadata_projection_has_sorted_names_and_no_values_or_lengths() {
    let root = tempfile::tempdir().unwrap();
    let store = SecretStore::new_empty(Box::new(InMemoryKeyProvider::new()), root.path().to_owned());
    store.store_provisioned("fixture", "Example", HashMap::from([
        ("username".into(), "SYNTHETIC-USER-CANARY".into()),
        ("password".into(), "SYNTHETIC-PASSWORD-CANARY".into()),
    ]), InjectionTarget::FormFields(HashMap::new()), SecretPolicy::default()).unwrap();
    let metadata = store.provisioned_metadata("fixture").unwrap();
    assert_eq!(metadata.field_names, ["password", "username"]);
    let wire = serde_json::to_value(&metadata).unwrap();
    assert_eq!(wire, serde_json::json!({"id":"fixture","label":"Example","field_names":["password","username"]}));
    assert!(!format!("{metadata:?}").contains("CANARY"));
    assert!(store.provisioned_metadata("missing").is_none());
    // Existing trusted callers still receive the same record/API semantics.
    assert_eq!(store.get_provisioned("fixture").unwrap().fields["password"], "SYNTHETIC-PASSWORD-CANARY");
}
