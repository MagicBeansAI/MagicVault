use magicvault_protocol::*;

#[test]
fn enrollment_and_approval_reject_secret_values_and_human_decisions() {
    for value in [
        serde_json::json!({"method":"enroll","params":{"label":"Example","field_names":["password"],"password":"CANARY"}}),
        serde_json::json!({"method":"request_access","params":{"credential_ref":"cred_123","approved":true}}),
        serde_json::json!({"method":"grant_approval","params":{"approved":true}}),
        serde_json::json!({"method":"secure_fill","params":{}}),
        serde_json::json!({"method":"read_secret","params":{}}),
    ] {
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}

#[test]
fn metadata_validation_bounds_labels_fields_and_duplicates() {
    for fields in [vec![], vec!["password".into(), "password".into()], vec!["bad\nname".into()], vec!["x".repeat(65)]] {
        assert!(Request::Enroll(EnrollRequest { label:"Example".into(), field_names:fields }).validate().is_err());
    }
    assert!(Request::Pair(PairRequest { label:"untrusted\nnew prompt".into() }).validate().is_err());
    assert!(Request::Pair(PairRequest { label:"   ".into() }).validate().is_err());
    assert!(!valid_reference("cred_00000000000000000000000000000000"));
    assert!(valid_reference("cred_00000000-0000-0000-0000-000000000000"));
    assert!(Request::Enroll(EnrollRequest { label:"Example".into(), field_names:vec!["username".into(), "password".into()] }).validate().is_ok());
}
