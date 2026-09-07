use magicvault_protocol::*;
use serde_json::json;
use uuid::Uuid;

fn fill() -> SecureFill {
    SecureFill {
        operation_id: Uuid::new_v4(),
        browser_handle: Uuid::new_v4(),
        target_handle: Uuid::new_v4(),
        fields: vec![FillField {
            css: "input[name=password]".into(),
            credential_ref: format!("cred_{}", Uuid::new_v4()),
            credential_field: "password".into(),
        }],
    }
}

#[test]
fn discovery_narrowing_is_optional_and_never_accepted_by_disconnect_or_fill() {
    let handle = Uuid::new_v4();
    let old = json!({"method":"browser_targets","params":{"browser_handle":handle}});
    let request: Request = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), old);
    let narrowed = json!({"method":"browser_targets","params":{"browser_handle":handle,"top_origin":"https://example.com","tab_id":"12"}});
    let request: Request = serde_json::from_value(narrowed.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), narrowed);
    for key in ["value", "approved", "origin", "javascript", "snapshot_ref"] {
        let mut bad = narrowed.clone();
        bad["params"][key] = json!("SYNTHETIC");
        assert!(serde_json::from_value::<Request>(bad).is_err());
    }
    let mut disconnect = narrowed;
    disconnect["method"] = json!("disconnect_browser");
    assert!(serde_json::from_value::<Request>(disconnect).is_err());
}

#[test]
fn fill_wire_is_closed_reference_only_and_has_no_caller_decision_or_javascript() {
    let request = fill();
    assert!(request.valid());
    let value = serde_json::to_value(Request::SecureFill(request)).unwrap();
    for (location, key, payload) in [
        ("request", "approved", json!(true)),
        ("request", "javascript", json!("return secret")),
        ("field", "value", json!("SYNTHETIC-SECRET")),
        ("field", "nodeId", json!(12)),
        ("field", "snapshot_ref", json!("@e12")),
    ] {
        let mut invalid = value.clone();
        if location == "request" {
            invalid["params"][key] = payload;
        } else {
            invalid["params"]["fields"][0][key] = payload;
        }
        assert!(serde_json::from_value::<Request>(invalid).is_err());
    }
    for name in [
        "read_secret",
        "grant_fill",
        "deliver_material",
        "evaluate",
        "secure_new_http",
        "secure_new_process",
    ] {
        assert!(serde_json::from_value::<Request>(json!({"method":name,"params":{}})).is_err());
    }
}

#[test]
fn fill_bounds_duplicate_locators_and_invalid_references_are_rejected() {
    let base = fill();
    for mutation in 0..7 {
        let mut request = base.clone();
        match mutation {
            0 => request.fields.clear(),
            1 => request.fields = vec![base.fields[0].clone(); 9],
            2 => request.fields.push(base.fields[0].clone()),
            3 => request.fields[0].css = "x".repeat(513),
            4 => request.fields[0].css = "input\nignored".into(),
            5 => request.fields[0].credential_ref = "password".into(),
            _ => request.operation_id = Uuid::nil(),
        }
        assert!(!request.valid());
    }
}

#[test]
fn response_contains_only_closed_per_field_status_and_identity() {
    let status = FillStatus {
        operation_id: Uuid::new_v4(),
        state: FillState::Partial,
        fields: vec![FieldState::Filled, FieldState::NotFilled],
        error: Some(ErrorCode::StaleTarget),
    };
    let mut value = serde_json::to_value(&status).unwrap();
    value["value"] = json!("SYNTHETIC-SECRET");
    assert!(serde_json::from_value::<FillStatus>(value).is_err());
    assert!(serde_json::from_value::<FieldState>(json!("SYNTHETIC-SECRET")).is_err());
}
