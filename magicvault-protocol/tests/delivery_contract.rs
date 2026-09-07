use magicvault_protocol::*;
use serde_json::json;
use uuid::Uuid;

fn profile() -> DeliveryProfile {
    serde_json::from_value(json!({"label":"Fixture", "destination":{"kind":"process","config":{
        "executable":"/usr/bin/env", "arguments":[], "working_directory":"/private/tmp",
        "environment":[{"name":"TOKEN","value":{"kind":"credential","credential_ref":format!("cred_{}",Uuid::new_v4()),"credential_field":"token"}}],
        "stdin":null,"timeout_secs":10
    }}})).unwrap()
}

#[test]
fn only_reference_invocations_are_admitted_and_unknown_overrides_fail() {
    let request = json!({"profile_id":Uuid::new_v4(),"operation_id":Uuid::new_v4()});
    let parsed: SecureDelivery = serde_json::from_value(request.clone()).unwrap();
    Request::SecureNewProcess(parsed.clone())
        .validate()
        .unwrap();
    Request::SecureNewHttp(parsed).validate().unwrap();
    for name in [
        "value",
        "command",
        "arguments",
        "url",
        "headers",
        "approved",
        "stdout",
    ] {
        let mut hostile = request.clone();
        hostile[name] = json!("SYNTHETIC-UNTRUSTED-OVERRIDE");
        assert!(serde_json::from_value::<SecureDelivery>(hostile).is_err());
    }
    assert!(Request::SecureNewHttp(SecureDelivery {
        profile_id: Uuid::nil(),
        operation_id: Uuid::new_v4()
    })
    .validate()
    .is_err());
}

#[test]
fn profile_bounds_names_paths_and_credentials_are_closed() {
    assert!(profile().valid());
    for change in 0..7 {
        let mut p = profile();
        let DeliveryDestination::Process(c) = &mut p.destination else {
            unreachable!()
        };
        match change {
            0 => c.executable = "relative-command".into(),
            1 => c.working_directory = "/private/../etc".into(),
            2 => c.arguments = vec!["x".repeat(513)],
            3 => c.environment.push(c.environment[0].clone()),
            4 => c.timeout_secs = 121,
            5 => c.environment.clear(),
            _ => c.arguments = vec!["hidden\u{202e}argument".into()],
        }
        assert!(!p.valid(), "mutation {change}");
    }
    let mut value = serde_json::to_value(profile()).unwrap();
    value["destination"]["config"]["environment"][0]["value"]["password"] = json!("SYNTHETIC-RAW");
    assert!(serde_json::from_value::<DeliveryProfile>(value).is_err());
}

#[test]
fn replies_have_only_closed_receipts_and_explicit_dispatch_evidence() {
    let status = DeliveryStatus {
        operation_id: Uuid::new_v4(),
        kind: DeliveryKind::Http,
        state: DeliveryState::Uncertain,
        may_have_run: true,
        error: Some(ErrorCode::TransportUncertain),
    };
    let mut value = serde_json::to_value(&status).unwrap();
    for name in [
        "stdout",
        "stderr",
        "body",
        "headers",
        "exit_code",
        "http_status",
        "diagnostic",
    ] {
        assert!(value.get(name).is_none());
        value[name] = json!("SYNTHETIC-RECIPIENT-ECHO");
        assert!(serde_json::from_value::<DeliveryStatus>(value.clone()).is_err());
        value.as_object_mut().unwrap().remove(name);
    }
}
