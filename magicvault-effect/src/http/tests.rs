use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{rustls, TlsAcceptor};

#[test]
fn special_use_addresses_cannot_bypass_the_destination_fence() {
    for address in [
        "0.1.2.3",
        "10.0.0.1",
        "100.64.0.1",
        "127.1.2.3",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "255.255.255.255",
        "::1",
        "::ffff:8.8.8.8",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "2001::1",
        "3fff::1",
    ] {
        assert!(!public_address(address.parse().unwrap()), "{address}");
    }
    for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(public_address(address.parse().unwrap()));
    }
}

#[tokio::test]
async fn real_tls_verifies_certificate_and_hostname_and_withholds_echo() {
    // Local deterministic TLS qualification. Custom root and loopback DNS pin
    // are test-only: the shipped API exposes neither an insecure switch nor a
    // caller-selected trust store/resolver. Production uses native roots.
    for case in ["trusted", "untrusted", "wrong-host"] {
        let certificate =
            rcgen::generate_simple_self_signed(vec!["mv-fixture.invalid".into()]).unwrap();
        let der = certificate.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certificate.key_pair.serialize_der());
        let server_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![der.clone()], key.into())
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let accepted = TlsAcceptor::from(Arc::new(server_config))
                .accept(stream)
                .await;
            if case != "trusted" {
                assert!(accepted.is_err());
                return;
            }
            let mut stream = accepted.unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 2048];
            while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buffer[..n]);
                assert!(bytes.len() < 16384);
            }
            assert!(String::from_utf8_lossy(&bytes).contains("SYNTHETIC-TLS-CANARY"));
            let body = "SYNTHETIC-TLS-CANARY";
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nX-Echo: {body}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
        });
        let host = if case == "wrong-host" {
            "different.invalid"
        } else {
            "mv-fixture.invalid"
        };
        let mut builder = transport_builder(Duration::from_secs(2))
            .tls_built_in_root_certs(false)
            .resolve_to_addrs(host, &[address]);
        if case != "untrusted" {
            builder = builder.add_root_certificate(reqwest::Certificate::from_der(&der).unwrap());
        }
        let client = builder.build().unwrap();
        let request = client
            .get(format!("https://{host}:{}/", address.port()))
            .header("Authorization", "SYNTHETIC-TLS-CANARY")
            .build()
            .unwrap();
        let outcome = exchange(client, request).await;
        if case == "trusted" {
            assert_eq!(outcome, Ok(true));
        } else {
            assert_eq!(outcome, Err(ErrorCode::TransportUncertain));
        }
        tokio::time::timeout(Duration::from_secs(3), server)
            .await
            .unwrap()
            .unwrap();
    }
}
