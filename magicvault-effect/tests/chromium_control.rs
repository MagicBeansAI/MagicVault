//! Independent opt-in control, deliberately outside the normal five-case lane.
use magicvault_test_support::browser::DisposableBrowser;

#[tokio::test]
#[ignore = "explicit Chrome-only startup/large-tab control; no MagicVault extension, broker or custody"]
async fn two_profile_large_tab_browser_control() {
    // Ten independent trials, never retries: any failed command unwinds the
    // current browser owners and prevents the next trial. Same ordinary browser
    // flags/fixtures; no background-throttling or security-policy bypass.
    for trial in 1..=10 {
        eprintln!("browser-only control trial: {trial}/10");
        let first = DisposableBrowser::start(true).await;
        let mut a = first.peer().await;
        let (_, page_a) = a.open_page(&format!("{}/login", first.origin)).await;
        let second = DisposableBrowser::start(true).await;
        let mut b = second.peer().await;
        let (_, page_b) = b.open_page(&format!("{}/login", second.origin)).await;
        for _ in 0..65 {
            a.command(
                "Target.createTarget",
                serde_json::json!({"url":"about:blank","background":true}),
                None,
            )
            .await;
        }
        for _ in 0..21 {
            // No credential value or extension/API call: compare page-local
            // booleans and exercise both browser renderers under the tab load.
            assert!(
                a.evaluate_bool(&page_a, "document.querySelector('#password').value === ''")
                    .await
            );
            assert!(
                b.evaluate_bool(&page_b, "document.querySelector('#password').value === ''")
                    .await
            );
        }
        a.navigate(&page_a, &format!("{}/next", first.origin)).await;
        assert!(
            a.evaluate_bool(&page_a, "document.querySelector('#password').value === ''")
                .await
        );
        a.navigate(&page_a, &format!("{}/login", first.origin))
            .await;
        eprintln!("browser-only control completed: {trial}/10");
    }
}
