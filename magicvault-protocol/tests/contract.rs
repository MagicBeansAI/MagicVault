use magicvault_protocol::*;

#[test]
fn enrollment_and_approval_reject_secret_values_and_human_decisions() {
    for value in [
        serde_json::json!({"method":"enroll","params":{"label":"Example","field_names":["password"],"password":"CANARY"}}),
        serde_json::json!({"method":"request_access","params":{"credential_ref":"cred_123","approved":true}}),
        serde_json::json!({"method":"grant_approval","params":{"approved":true}}),
        serde_json::json!({"method":"grant_consent","params":{"always_allow":true}}),
        serde_json::json!({"method":"clear_consents","params":{"always_allow":true}}),
        serde_json::json!({"method":"secure_new_http","params":{"profile_id":"00000000-0000-0000-0000-000000000001","operation_id":"00000000-0000-0000-0000-000000000002","always_allow":true}}),
        serde_json::json!({"method":"secure_fill","params":{}}),
        serde_json::json!({"method":"read_secret","params":{}}),
    ] {
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
}

#[test]
fn consent_management_is_read_or_revoke_only_and_queries_are_bounded() {
    let query = Request::RevokeConsent(ConsentQuery {
        grant_id: uuid::Uuid::nil(),
    });
    assert_eq!(query.validate(), Err(ErrorCode::InvalidRequest));
    for request in [
        Request::ListConsents,
        Request::ClearConsents,
        Request::RevokeConsent(ConsentQuery {
            grant_id: uuid::Uuid::new_v4(),
        }),
    ] {
        let encoded = serde_json::to_vec(&request).unwrap();
        assert!(serde_json::from_slice::<Request>(&encoded)
            .unwrap()
            .validate()
            .is_ok());
    }
}

#[test]
fn metadata_validation_bounds_labels_fields_and_duplicates() {
    for fields in [
        vec![],
        vec!["password".into(), "password".into()],
        vec!["bad\nname".into()],
        vec!["x".repeat(65)],
    ] {
        assert!(Request::Enroll(EnrollRequest {
            label: "Example".into(),
            field_names: fields
        })
        .validate()
        .is_err());
    }
    assert!(Request::Pair(PairRequest {
        label: "untrusted\nnew prompt".into()
    })
    .validate()
    .is_err());
    assert!(Request::Pair(PairRequest {
        label: "   ".into()
    })
    .validate()
    .is_err());
    assert!(!valid_reference("cred_00000000000000000000000000000000"));
    assert!(valid_reference("cred_00000000-0000-0000-0000-000000000000"));
    assert!(Request::Enroll(EnrollRequest {
        label: "Example".into(),
        field_names: vec!["username".into(), "password".into()]
    })
    .validate()
    .is_ok());
}
