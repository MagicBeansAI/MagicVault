//! Opt-in real Chromium conformance using shared disposable profiles/pages.
use magicvault_effect::{cdp::CdpBrowser, BrowserAdapter, MaterialField, Target};
use magicvault_protocol::{ErrorCode, FieldState};
use magicvault_test_support::browser::{DisposableBrowser, BROWSER_CANARY};
use tokio_util::sync::CancellationToken;

fn fields(selectors: &[&str]) -> Vec<MaterialField> {
    selectors
        .iter()
        .map(|css| MaterialField {
            css: (*css).into(),
            value: BROWSER_CANARY.into(),
        })
        .collect()
}
async fn bound(browser: &CdpBrowser, tab: &str) -> Target {
    browser
        .targets(CancellationToken::new())
        .await
        .unwrap()
        .into_iter()
        .find(|target| target.tab == tab && target.is_main_frame)
        .expect("explicit test tab not discovered")
}
async fn qualify(headless: bool) {
    let mut owner = DisposableBrowser::start(headless).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page(&format!("{}/login", owner.origin)).await;
    let browser = CdpBrowser::connect(&owner.endpoint).await.unwrap();
    let target = bound(&browser, &tab).await;
    let outcome = browser
        .fill(
            &target,
            fields(&["#username", "#password"]),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled, FieldState::Filled]);
    assert_eq!(outcome.error, None);
    assert!(!serde_json::to_string(&outcome)
        .unwrap()
        .contains(BROWSER_CANARY));
    browser.disconnect();
    assert!(peer.evaluate_bool(&session,
        "document.querySelector('#password').value === 'SYNTHETIC-CHROMIUM-FILL-CANARY' && document.body.dataset.reactive === 'ok' && document.body.dataset.submitted !== 'yes'").await);
    assert!(owner.running());
}

#[tokio::test]
#[ignore = "explicit disposable browser qualification; never run implicitly"]
async fn headed_fill_and_original_tool_continuation() {
    qualify(false).await;
}

#[tokio::test]
#[ignore = "explicit disposable browser qualification; never run implicitly"]
async fn modern_headless_fill_and_original_tool_continuation() {
    qualify(true).await;
}

#[tokio::test]
#[ignore = "explicit disposable browser qualification; never run implicitly"]
async fn real_controls_fail_before_any_write_and_changed_fields_report_partial() {
    let owner = DisposableBrowser::start(true).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page(&format!("{}/controls", owner.origin)).await;
    let browser = CdpBrowser::connect(&owner.endpoint).await.unwrap();
    let target = bound(&browser, &tab).await;
    for css in [
        "#readonly",
        "#fieldset-disabled",
        "#hidden",
        "#file",
        "#short",
        ".duplicate",
        "#missing",
        "#shadow-input",
    ] {
        let outcome = browser
            .fill(
                &target,
                fields(&["#password", css]),
                CancellationToken::new(),
            )
            .await;
        assert_eq!(
            outcome.fields,
            [FieldState::NotFilled, FieldState::NotFilled],
            "strict preflight must precede every write"
        );
        assert!(outcome.error.is_some());
        assert!(
            peer.evaluate_bool(&session, "document.querySelector('#password').value === ''")
                .await
        );
    }
    let outcome = browser
        .fill(
            &target,
            fields(&["#replace-first", "#replace-later"]),
            CancellationToken::new(),
        )
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled, FieldState::NotFilled]);
    assert_eq!(outcome.error, Some(ErrorCode::StaleTarget));
    assert!(
        peer.evaluate_bool(
            &session,
            "document.querySelector('#replace-later').value === ''"
        )
        .await
    );
    browser.disconnect();
}

#[tokio::test]
#[ignore = "explicit disposable browser qualification; never run implicitly"]
async fn real_navigation_invalidates_the_old_document_without_writing_replacement() {
    let owner = DisposableBrowser::start(true).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page(&format!("{}/login", owner.origin)).await;
    let browser = CdpBrowser::connect(&owner.endpoint).await.unwrap();
    let target = bound(&browser, &tab).await;
    peer.navigate(&session, &format!("{}/next", owner.origin))
        .await;
    let outcome = browser
        .fill(&target, fields(&["#password"]), CancellationToken::new())
        .await;
    assert_eq!(outcome.fields, [FieldState::NotFilled]);
    assert_eq!(outcome.error, Some(ErrorCode::StaleTarget));
    assert!(
        peer.evaluate_bool(&session, "document.querySelector('#password').value === ''")
            .await
    );
}

#[tokio::test]
#[ignore = "explicit disposable browser qualification; never run implicitly"]
async fn real_same_origin_frame_fills_but_opaque_frame_is_not_discovered() {
    let owner = DisposableBrowser::start(true).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page(&format!("{}/frames", owner.origin)).await;
    let browser = CdpBrowser::connect(&owner.endpoint).await.unwrap();
    let targets = browser
        .targets(CancellationToken::new())
        .await
        .unwrap()
        .into_iter()
        .filter(|target| target.tab == tab)
        .collect::<Vec<_>>();
    assert_eq!(
        targets.len(),
        2,
        "only top page and non-opaque same-origin frame are supported"
    );
    let frame = targets
        .into_iter()
        .find(|target| !target.is_main_frame)
        .unwrap();
    let outcome = browser
        .fill(&frame, fields(&["#password"]), CancellationToken::new())
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled]);
    assert!(peer.evaluate_bool(&session,
        "document.querySelector('#same-origin-frame').contentDocument.querySelector('#password').value === 'SYNTHETIC-CHROMIUM-FILL-CANARY' && document.querySelector('#password').value === ''").await);
    browser.disconnect();
}
