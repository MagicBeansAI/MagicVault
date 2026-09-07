//! Optional network compatibility smoke, never a default/CI requirement.
use magicvault_effect::{cdp::CdpBrowser, BrowserAdapter, MaterialField};
use magicvault_protocol::FieldState;
use magicvault_test_support::browser::{DisposableBrowser, BROWSER_CANARY};
use tokio_util::sync::CancellationToken;

#[tokio::test]
#[ignore = "explicit public-demo network qualification; synthetic input only, never submit"]
async fn public_documentation_and_selenium_demo_without_submission() {
    assert_eq!(
        std::env::var("MAGICVAULT_PUBLIC_WEB").as_deref(),
        Ok("1"),
        "explicit public network opt-in required"
    );
    let owner = DisposableBrowser::start(true).await;
    let mut peer = owner.peer().await;
    let (tab, session) = peer.open_page("https://example.com/").await;
    assert!(
        peer.evaluate_bool(&session, "document.title === 'Example Domain'")
            .await
    );
    peer.navigate(
        &session,
        "https://www.selenium.dev/selenium/web/web-form.html",
    )
    .await;
    // Additional guard: even an unexpected synthetic submit is blocked locally.
    assert!(peer.evaluate_bool(&session,
        "(() => { const form = document.querySelector('form'); if (!form) return false; form.addEventListener('submit', e => { e.preventDefault(); document.body.dataset.fixtureSubmitted = 'yes'; }); return true; })()"
    ).await);
    let browser = CdpBrowser::connect(&owner.endpoint).await.unwrap();
    let target = browser
        .targets(CancellationToken::new())
        .await
        .unwrap()
        .into_iter()
        .find(|target| {
            target.tab == tab && target.is_main_frame && target.origin == "https://www.selenium.dev"
        })
        .unwrap();
    let outcome = browser
        .fill(
            &target,
            vec![MaterialField {
                css: "input[type=password]".into(),
                value: BROWSER_CANARY.into(),
            }],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled]);
    assert_eq!(outcome.error, None);
    assert!(peer.evaluate_bool(&session,
        "document.querySelector('input[type=password]').value === 'SYNTHETIC-CHROMIUM-FILL-CANARY' && document.body.dataset.fixtureSubmitted !== 'yes'"
    ).await);
    browser.disconnect();
}
