use magicvault_effect::{
    canonical_origin,
    cdp::{validate_endpoint, CdpBrowser},
    origin_from_url, BrowserAdapter, MaterialField,
};
use magicvault_protocol::{ErrorCode, FieldState};
use magicvault_test_support::{CdpFixture, CANARY};
use tokio_util::sync::CancellationToken;

fn fields() -> Vec<MaterialField> {
    vec![MaterialField {
        css: "#password".into(),
        value: CANARY.into(),
    }]
}

#[test]
fn endpoint_and_origin_policies_reject_remote_ambient_and_credential_bearing_routes() {
    for endpoint in [
        "ws://example.com:9222/devtools/browser/a",
        "ws://localhost:9222/devtools/browser/a",
        "http://127.0.0.1:9222/json",
        "ws://127.0.0.1:9222/devtools/page/a",
        "ws://user:password@127.0.0.1:9222/devtools/browser/a",
        "ws://127.0.0.1:9222/devtools/browser/a?token=x",
    ] {
        assert!(validate_endpoint(endpoint).is_err());
    }
    assert!(validate_endpoint("ws://127.0.0.1:9222/devtools/browser/a-b").is_ok());
    for origin in [
        "http://example.com",
        "https://user:password@example.com",
        "https://example.com/path",
        "https://example.com?token=x",
        "https://example.com#fragment",
        "file:///tmp/a",
        "https://*.example.com",
    ] {
        assert!(canonical_origin(origin).is_err());
    }
    assert_eq!(
        canonical_origin("https://EXAMPLE.com:443/").unwrap(),
        "https://example.com"
    );
    assert_eq!(
        origin_from_url("https://example.com/login?token=SYNTHETIC-URL-CANARY").unwrap(),
        "https://example.com"
    );
}

#[tokio::test]
async fn dedicated_cdp_connection_binds_document_and_delivers_without_returning_values() {
    let fixture = CdpFixture::start().await;
    let browser = CdpBrowser::connect(&fixture.endpoint).await.unwrap();
    let targets = browser.targets(CancellationToken::new()).await.unwrap();
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].origin, "https://example.com");
    assert!(!serde_json::to_string(&targets).unwrap().contains("CANARY"));
    let outcome = browser
        .fill(&targets[0], fields(), CancellationToken::new())
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled]);
    assert_eq!(outcome.error, None);
    assert_eq!(
        fixture.state.lock().unwrap().delivered,
        [vec![CANARY.to_owned()]]
    );
    assert!(!serde_json::to_string(&outcome).unwrap().contains(CANARY));
    browser.disconnect();
    assert!(!browser.connected());
    assert!(!fixture
        .state
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|method| method == "Browser.close" || method == "Page.navigate"));
}

#[tokio::test]
async fn document_navigation_prevents_material_delivery() {
    let fixture = CdpFixture::start().await;
    let browser = CdpBrowser::connect(&fixture.endpoint).await.unwrap();
    let targets = browser.targets(CancellationToken::new()).await.unwrap();
    fixture.state.lock().unwrap().navigated = true;
    let outcome = browser
        .fill(&targets[0], fields(), CancellationToken::new())
        .await;
    assert_eq!(outcome.error, Some(ErrorCode::StaleTarget));
    assert_eq!(outcome.fields, [FieldState::NotFilled]);
    assert!(fixture.state.lock().unwrap().delivered.is_empty());
}

#[tokio::test]
async fn lost_or_malformed_reply_is_uncertain_and_never_replayed() {
    for malformed in [false, true] {
        let fixture = CdpFixture::start().await;
        let browser = CdpBrowser::connect(&fixture.endpoint).await.unwrap();
        let targets = browser.targets(CancellationToken::new()).await.unwrap();
        {
            let mut state = fixture.state.lock().unwrap();
            state.lose_fill_reply = !malformed;
            state.malformed_fill_reply = malformed;
        }
        let outcome = browser
            .fill(&targets[0], fields(), CancellationToken::new())
            .await;
        assert_eq!(outcome.fields, [FieldState::Uncertain]);
        assert_eq!(outcome.error, Some(ErrorCode::TransportUncertain));
        assert!(!serde_json::to_string(&outcome).unwrap().contains("CANARY"));
        let _ = browser
            .fill(&targets[0], fields(), CancellationToken::new())
            .await;
        assert_eq!(fixture.state.lock().unwrap().delivered.len(), 1);
    }
}

#[tokio::test]
async fn no_numeric_context_fallback_or_delivery_after_pre_cancel() {
    let fixture = CdpFixture::start().await;
    let browser = CdpBrowser::connect(&fixture.endpoint).await.unwrap();
    let targets = browser.targets(CancellationToken::new()).await.unwrap();
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let outcome = browser.fill(&targets[0], fields(), cancelled).await;
    assert_eq!(outcome.fields, [FieldState::NotFilled]);
    fixture.state.lock().unwrap().omit_unique_context = true;
    let _ = browser
        .fill(&targets[0], fields(), CancellationToken::new())
        .await;
    assert!(fixture.state.lock().unwrap().delivered.is_empty());
}
