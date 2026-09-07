use magicvault_effect::{delivery::DeliveryMaterial, http};
use magicvault_protocol::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

const CANARY: &str = "SYNTHETIC-HTTP-SECRET";
fn config(url: String) -> (HttpDestination, DeliveryMaterial) {
    let reference = format!("cred_{}", Uuid::new_v4());
    let value = InputValue::Credential {
        credential_ref: reference.clone(),
        credential_field: "token".into(),
        prefix: "Bearer ".into(),
        suffix: String::new(),
    };
    let mut material = DeliveryMaterial::default();
    material.insert(reference, "token".into(), Zeroizing::new(CANARY.into()));
    (
        HttpDestination {
            url,
            method: "GET".into(),
            headers: vec![NamedValue {
                name: "Authorization".into(),
                value,
            }],
            query: vec![],
            body: None,
            timeout_secs: 2,
        },
        material,
    )
}

async fn receive(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            let n = stream.read(&mut buffer).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
            assert!(bytes.len() <= 128 * 1024);
            if let Some(header_end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..header_end]);
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end + 4 + length {
                    break;
                }
            }
        }
        bytes
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn real_http_verbs_deliver_credentials_but_echo_bodies_headers_and_status_are_withheld() {
    for method in [
        "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "PROPFIND",
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut config, material) =
            config(format!("http://{}/api", listener.local_addr().unwrap()));
        config.method = method.into();
        let method = method.to_owned();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = receive(&mut stream).await;
            assert!(request.starts_with(format!("{method} /api ").as_bytes()));
            assert!(String::from_utf8_lossy(&request).contains(&format!("Bearer {CANARY}")));
            let encoded = CANARY
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let body = if method == "HEAD" {
                String::new()
            } else {
                format!("{CANARY}-{encoded}")
            };
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Echo: {CANARY}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
        });
        let result = http::execute(&config, material, CancellationToken::new()).await;
        server.await.unwrap();
        assert_eq!(result.state, DeliveryState::Completed);
        let status = DeliveryStatus {
            operation_id: Uuid::new_v4(),
            kind: DeliveryKind::Http,
            state: result.state,
            may_have_run: result.may_have_run,
            error: result.error,
        };
        let output = serde_json::to_string(&status).unwrap();
        assert!(!output.contains(CANARY));
        let projection = serde_json::to_value(&status).unwrap();
        assert!(projection.get("http_status").is_none());
    }
}

#[tokio::test]
async fn form_json_text_and_query_placement_use_context_encoding() {
    for kind in ["form", "json", "text"] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (mut config, material) =
            config(format!("http://{}/body", listener.local_addr().unwrap()));
        let value = config.headers[0].value.clone();
        config.method = "POST".into();
        config.query = vec![NamedValue {
            name: "token".into(),
            value: value.clone(),
        }];
        let row = NamedValue {
            name: "token".into(),
            value: value.clone(),
        };
        config.body = Some(match kind {
            "form" => HttpBody::Form(vec![row]),
            "json" => HttpBody::Json(vec![row]),
            _ => HttpBody::Text(value),
        });
        let kind = kind.to_owned();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = receive(&mut stream).await;
            let text = String::from_utf8(request).unwrap();
            assert!(text.starts_with(&format!("POST /body?token=Bearer+{CANARY} HTTP/1.1")));
            let body = text.split_once("\r\n\r\n").unwrap().1;
            match kind.as_str() {
                "form" => assert_eq!(body, format!("token=Bearer+{CANARY}")),
                "json" => assert_eq!(
                    serde_json::from_str::<serde_json::Value>(body).unwrap()["token"],
                    format!("Bearer {CANARY}")
                ),
                _ => assert_eq!(body, format!("Bearer {CANARY}")),
            }
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });
        assert_eq!(
            http::execute(&config, material, CancellationToken::new())
                .await
                .state,
            DeliveryState::Completed
        );
        server.await.unwrap();
    }
}

#[tokio::test]
async fn redirect_and_lost_reply_never_trigger_another_request() {
    for redirect in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (config, material) = config(format!("http://{address}/first"));
        let count = Arc::new(AtomicUsize::new(0));
        let received = count.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            receive(&mut stream).await;
            received.fetch_add(1, Ordering::SeqCst);
            if redirect {
                stream.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{address}/second\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            }
            drop(stream);
            if tokio::time::timeout(Duration::from_millis(200), listener.accept())
                .await
                .is_ok()
            {
                received.fetch_add(1, Ordering::SeqCst);
            }
        });
        let result = http::execute(&config, material, CancellationToken::new()).await;
        server.await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(result.may_have_run);
        assert_ne!(result.state, DeliveryState::Completed);
        if !redirect {
            assert_eq!(result.state, DeliveryState::Uncertain);
        }
    }
}

#[tokio::test]
async fn oversized_response_and_cancellation_are_uncertain_after_dispatch() {
    for flood in [true, false] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (config, material) = config(format!("http://{}/", listener.local_addr().unwrap()));
        let cancel = CancellationToken::new();
        let signal = cancel.clone();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            receive(&mut stream).await;
            if flood {
                let body = vec![b'X'; MAX_DELIVERY_OUTPUT_BYTES + 1];
                stream
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len())
                            .as_bytes(),
                    )
                    .await
                    .unwrap();
                let _ = stream.write_all(&body).await;
            } else {
                signal.cancel();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });
        let result = http::execute(&config, material, cancel).await;
        server.await.unwrap();
        assert_eq!(result.state, DeliveryState::Uncertain);
        assert!(result.may_have_run);
    }
}

#[tokio::test]
async fn nonpublic_dns_and_pre_cancel_never_dispatch() {
    let (destination, material) = config("https://localhost/".into());
    let result = http::execute(&destination, material, CancellationToken::new()).await;
    assert!(!result.may_have_run);
    assert_ne!(result.state, DeliveryState::Completed);
    let (config, material) = config("http://127.0.0.1:1/".into());
    let cancel = CancellationToken::new();
    cancel.cancel();
    let result = http::execute(&config, material, cancel).await;
    assert!(!result.may_have_run);
    assert_eq!(result.state, DeliveryState::Cancelled);
}

#[test]
fn unsafe_endpoints_and_transport_header_overrides_fail_closed() {
    for url in [
        "http://example.com/",
        "https://127.0.0.1/",
        "https://169.254.169.254/",
        "https://[::ffff:127.0.0.1]/",
        "https://user:pass@example.com/",
        "https://example.com/#fragment",
        "https://example.com/?token=literal",
    ] {
        assert!(http::validate(&config(url.into()).0).is_err(), "{url}");
    }
    for name in [
        "Host",
        "Content-Length",
        "Transfer-Encoding",
        "Proxy-Authorization",
        "Connection",
        "Upgrade",
        "Expect",
    ] {
        let (mut config, _) = config("https://example.com/".into());
        config.headers[0].name = name.into();
        assert!(http::validate(&config).is_err());
    }
    let (mut config, _) = config("https://example.com/".into());
    let mut duplicate = config.headers[0].clone();
    duplicate.name = "authorization".into();
    config.headers.push(duplicate);
    assert!(http::validate(&config).is_err());
}

#[test]
fn direct_rust_http_entry_rejects_unbounded_or_ambiguous_profiles() {
    for method in ["CONNECT", "get", "GET\r\nX: evil", "VERYLONGCUSTOMMETHOD"] {
        let (mut c, _) = config("https://example.com/".into());
        c.method = method.into();
        assert!(http::validate(&c).is_err());
    }
    let (mut c, _) = config("https://example.com/".into());
    c.timeout_secs = u64::MAX;
    assert!(http::validate(&c).is_err());
    c.timeout_secs = 1;
    c.body = Some(HttpBody::Text(c.headers[0].value.clone()));
    c.headers.push(NamedValue {
        name: "Content-Type".into(),
        value: InputValue::Literal {
            value: "application/json".into(),
        },
    });
    assert!(http::validate(&c).is_err());
    c.headers.clear();
    c.body = None;
    assert!(http::validate(&c).is_err());
}

#[tokio::test]
async fn stalled_body_deadline_and_invalid_secret_header_do_not_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (mut c, material) = config(format!("http://{}/", listener.local_addr().unwrap()));
    c.timeout_secs = 1;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        receive(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nx")
            .await
            .unwrap();
        let mut byte = [0u8; 1];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    });
    let result = http::execute(&c, material, CancellationToken::new()).await;
    assert_eq!(result.state, DeliveryState::Uncertain);
    assert!(result.may_have_run);
    server.await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let (c, mut material) = config(format!("http://{}/", listener.local_addr().unwrap()));
    let (reference, field) = c.headers[0].value.credential().unwrap();
    material.insert(
        reference.into(),
        field.into(),
        Zeroizing::new("synthetic\r\nX-Injected: value".into()),
    );
    let result = http::execute(&c, material, CancellationToken::new()).await;
    assert!(!result.may_have_run);
    assert_eq!(result.error, Some(ErrorCode::InvalidRequest));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}
