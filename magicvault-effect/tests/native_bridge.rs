#![cfg(unix)]
use magicvault_effect::{bridge::*, BrowserAdapter, MaterialField, Target};
use magicvault_protocol::{ErrorCode, FieldState};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::test]
async fn filtered_bridge_preserves_narrowing_and_refuses_out_of_scope_replies() {
    for mismatch in [false, true] {
        let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
        let bridge = NativeBridge::authenticated(daemon);
        bridge.initialize(b"{}").await.unwrap();
        read_frame(&mut host).await.unwrap();
        let filter = magicvault_protocol::TargetFilter {
            top_origin: Some("https://example.com".into()),
            tab_id: Some("1".into()),
        };
        let expected = filter.clone();
        let worker = tokio::spawn(async move {
            let command: BridgeCommand =
                serde_json::from_slice(&read_frame(&mut host).await.unwrap()).unwrap();
            let BridgeRequest::FilteredTargets(actual) = command.request else {
                panic!("filtered targets");
            };
            assert_eq!(actual, expected);
            let mut found = target();
            if mismatch {
                found.tab = "2".into();
            }
            write_frame(
                &mut host,
                &serde_json::to_vec(&BridgeReply {
                    request_id: command.request_id,
                    result: BridgeResult::Targets(vec![found]),
                })
                .unwrap(),
            )
            .await
            .unwrap();
            host
        });
        let result = bridge
            .targets_filtered(&filter, CancellationToken::new())
            .await;
        let _host = worker.await.unwrap();
        if mismatch {
            assert!(matches!(result, Err(ErrorCode::TransportUncertain)));
            assert!(!bridge.connected());
        } else {
            assert_eq!(result.unwrap().len(), 1);
            assert!(bridge.connected());
        }
        bridge.disconnect();
    }
}

fn target() -> Target {
    Target {
        tab: "1".into(),
        frame: "0".into(),
        document: Uuid::new_v4().to_string(),
        top_document: Uuid::new_v4().to_string(),
        origin: "https://example.com".into(),
        top_origin: "https://example.com".into(),
        is_main_frame: true,
    }
}

#[tokio::test]
async fn quiet_native_host_eof_is_detected_without_consuming_a_reply() {
    let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
    let bridge = NativeBridge::authenticated(daemon);
    assert!(bridge.connected());
    bridge.initialize(b"{}").await.unwrap();
    read_frame(&mut host).await.unwrap();
    write_frame(&mut host, b"{}").await.unwrap();
    assert!(bridge.connected()); // Peek does not consume buffered bytes.
    bridge.disconnect();
    let (daemon, host) = tokio::net::UnixStream::pair().unwrap();
    let bridge = NativeBridge::authenticated(daemon);
    drop(host);
    assert!(!bridge.connected());
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrent_health_polling_never_contends_with_delivery() {
    let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
    let bridge = NativeBridge::authenticated(daemon);
    bridge.initialize(b"{}").await.unwrap();
    read_frame(&mut host).await.unwrap();
    let worker = tokio::spawn(async move {
        for _ in 0..200 {
            let request: BridgeCommand =
                serde_json::from_slice(&read_frame(&mut host).await.unwrap()).unwrap();
            write_frame(
                &mut host,
                &serde_json::to_vec(&BridgeReply {
                    request_id: request.request_id,
                    result: BridgeResult::Targets(vec![]),
                })
                .unwrap(),
            )
            .await
            .unwrap();
        }
        host
    });
    let monitor = bridge.clone();
    let polling = std::thread::spawn(move || {
        for _ in 0..100_000 {
            assert!(monitor.connected());
        }
    });
    for _ in 0..200 {
        assert!(bridge
            .targets(CancellationToken::new())
            .await
            .unwrap()
            .is_empty());
    }
    let mut host = worker.await.unwrap();
    polling.join().unwrap();
    bridge.disconnect();
    // Shutdown must reach the peer even while the monitor descriptor is owned.
    assert!(read_frame(&mut host).await.is_err());
}

#[tokio::test]
async fn trusted_bridge_has_separate_material_requests_and_value_free_replies() {
    let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
    let bridge = NativeBridge::authenticated(daemon);
    bridge.initialize(b"{}").await.unwrap();
    read_frame(&mut host).await.unwrap();
    let bound = target();
    let remote = bound.clone();
    let worker = tokio::spawn(async move {
        let bytes = read_frame(&mut host).await.unwrap();
        let request: BridgeCommand = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(request.request, BridgeRequest::Targets));
        write_frame(
            &mut host,
            &serde_json::to_vec(&BridgeReply {
                request_id: request.request_id,
                result: BridgeResult::Targets(vec![remote]),
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let bytes = read_frame(&mut host).await.unwrap();
        let request: BridgeCommand = serde_json::from_slice(&bytes).unwrap();
        let BridgeRequest::Fill { fields, .. } = &request.request else {
            panic!("fill");
        };
        assert_eq!(fields[0].value, "SYNTHETIC-NATIVE-CANARY");
        write_frame(
            &mut host,
            &serde_json::to_vec(&BridgeReply {
                request_id: request.request_id,
                result: BridgeResult::Filled(magicvault_effect::Outcome {
                    fields: vec![FieldState::Filled],
                    error: None,
                }),
            })
            .unwrap(),
        )
        .await
        .unwrap();
    });
    let discovered = bridge.targets(CancellationToken::new()).await.unwrap();
    assert!(discovered[0] == bound);
    let outcome = bridge
        .fill(
            &bound,
            vec![MaterialField {
                css: "#password".into(),
                value: "SYNTHETIC-NATIVE-CANARY".into(),
            }],
            CancellationToken::new(),
        )
        .await;
    assert_eq!(outcome.fields, [FieldState::Filled]);
    assert!(!serde_json::to_string(&outcome).unwrap().contains("CANARY"));
    worker.await.unwrap();
    bridge.disconnect();
    assert!(!bridge.connected());
}

#[tokio::test]
async fn wrong_reply_id_disconnect_and_cancellation_are_uncertain_after_delivery() {
    for wrong_id in [false, true] {
        let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
        let bridge = NativeBridge::authenticated(daemon);
        bridge.initialize(b"{}").await.unwrap();
        read_frame(&mut host).await.unwrap();
        let worker = tokio::spawn(async move {
            let bytes = read_frame(&mut host).await.unwrap();
            let request: BridgeCommand = serde_json::from_slice(&bytes).unwrap();
            assert!(matches!(request.request, BridgeRequest::Fill { .. }));
            if wrong_id {
                write_frame(
                    &mut host,
                    &serde_json::to_vec(&BridgeReply {
                        request_id: Uuid::new_v4(),
                        result: BridgeResult::Filled(magicvault_effect::Outcome {
                            fields: vec![FieldState::Filled],
                            error: None,
                        }),
                    })
                    .unwrap(),
                )
                .await
                .unwrap();
            }
        });
        let outcome = bridge
            .fill(
                &target(),
                vec![MaterialField {
                    css: "#password".into(),
                    value: "SYNTHETIC-NATIVE-CANARY".into(),
                }],
                CancellationToken::new(),
            )
            .await;
        assert_eq!(outcome.fields, [FieldState::Uncertain]);
        assert_eq!(outcome.error, Some(ErrorCode::TransportUncertain));
        assert!(!bridge.connected());
        worker.await.unwrap();
    }
}

#[tokio::test]
async fn oversized_bridge_frames_and_uninitialized_adapters_fail_closed() {
    use tokio::io::AsyncWriteExt;
    let (daemon, mut host) = tokio::net::UnixStream::pair().unwrap();
    let bridge = NativeBridge::authenticated(daemon);
    assert!(matches!(
        bridge.targets(CancellationToken::new()).await,
        Err(ErrorCode::Busy)
    ));
    let (mut receiver, mut sender) = tokio::net::UnixStream::pair().unwrap();
    sender
        .write_u32_le((MAX_BRIDGE_BYTES + 1) as u32)
        .await
        .unwrap();
    assert!(matches!(
        read_frame(&mut receiver).await,
        Err(ErrorCode::Capacity)
    ));
    bridge.initialize(b"{}").await.unwrap();
    read_frame(&mut host).await.unwrap();
    bridge.disconnect();
}
