//! Exact-destination requests with no ambient proxy, redirects, cookies or
//! retries. Response payloads and transport diagnostics never leave this module.
use crate::delivery::{DeliveryMaterial, DeliveryOutcome};
use magicvault_protocol::{
    DeliveryDestination, DeliveryProfile, ErrorCode, HttpBody, HttpDestination,
    MAX_DELIVERY_OUTPUT_BYTES,
};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Method,
};
use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

fn endpoint(config: &HttpDestination) -> Result<Url, ErrorCode> {
    let url = Url::parse(&config.url).map_err(|_| ErrorCode::InvalidRequest)?;
    let loopback = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
        || url.host_str().is_none()
        || !(url.scheme() == "https" || (url.scheme() == "http" && loopback))
        || (url.scheme() == "https"
            && match url.host() {
                Some(url::Host::Ipv4(ip)) => !public_address(IpAddr::V4(ip)),
                Some(url::Host::Ipv6(ip)) => !public_address(IpAddr::V6(ip)),
                _ => false,
            })
    {
        return Err(ErrorCode::UnsupportedTarget);
    }
    Ok(url)
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, _, _] = ip.octets();
            !ip.is_private()
                && !ip.is_loopback()
                && !ip.is_link_local()
                && !ip.is_broadcast()
                && !ip.is_documentation()
                && a != 0
                && a < 224
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 198 && (b == 18 || b == 19))
                && !(a == 192 && b == 0)
        }
        IpAddr::V6(ip) => {
            let segments = ip.segments();
            // Global unicast only; no mapped, local, multicast, transition or
            // documentation destination can smuggle a private IPv4 route.
            (segments[0] & 0xe000) == 0x2000
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && (segments[1] < 0x200 || segments[1] == 0xdb8))
                && segments[0] != 0x3fff
        }
    }
}

pub fn validate(config: &HttpDestination) -> Result<(), ErrorCode> {
    if !(DeliveryProfile {
        label: "HTTP adapter".into(),
        destination: DeliveryDestination::Http(config.clone()),
    })
    .valid()
    {
        return Err(ErrorCode::InvalidRequest);
    }
    endpoint(config)?;
    Method::from_bytes(config.method.as_bytes()).map_err(|_| ErrorCode::InvalidRequest)?;
    let mut names = BTreeSet::new();
    for header in &config.headers {
        let name = HeaderName::from_bytes(header.name.as_bytes())
            .map_err(|_| ErrorCode::InvalidRequest)?;
        if (config.body.is_some() && name == reqwest::header::CONTENT_TYPE)
            || !names.insert(name.as_str().to_owned())
            || matches!(
                name.as_str(),
                "host"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "upgrade"
                    | "proxy-authorization"
                    | "proxy-connection"
                    | "trailer"
                    | "te"
                    | "expect"
            )
        {
            return Err(ErrorCode::InvalidRequest);
        }
    }
    Ok(())
}

// A platform resolver can remain blocked after its caller's deadline. The
// permit lives INSIDE the blocking worker, not its cancellable waiter: at most
// two such workers may exist across all callers, even after repeated timeouts.
async fn resolve(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<BTreeSet<SocketAddr>, ErrorCode> {
    static RESOLVERS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = RESOLVERS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(2)))
        .clone()
        .try_acquire_owned()
        .map_err(|_| ErrorCode::Busy)?;
    let host = host.to_owned();
    let (send, receive) = tokio::sync::oneshot::channel();
    // Dedicated bounded threads also keep a stuck OS resolver from holding
    // Tokio runtime shutdown hostage. No credential material enters this work.
    std::thread::Builder::new()
        .name("magicvault-dns".into())
        .spawn(move || {
            let _permit = permit;
            let result = (|| {
                let addresses = (host.as_str(), port)
                    .to_socket_addrs()
                    .map_err(|_| ErrorCode::TransportUnavailable)?
                    .take(17)
                    .collect::<Vec<_>>();
                if addresses.is_empty()
                    || addresses.len() > 16
                    || addresses.iter().any(|a| !public_address(a.ip()))
                {
                    return Err(ErrorCode::UnsupportedTarget);
                }
                Ok(addresses.into_iter().collect())
            })();
            let _ = send.send(result);
        })
        .map_err(|_| ErrorCode::TransportUnavailable)?;
    tokio::time::timeout(Duration::from_secs(5).min(timeout), receive)
        .await
        .map_err(|_| ErrorCode::Expired)?
        .map_err(|_| ErrorCode::TransportUnavailable)?
}

fn transport_builder(timeout: Duration) -> reqwest::ClientBuilder {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .connection_verbose(false)
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10).min(timeout))
        .read_timeout(Duration::from_secs(10).min(timeout))
        .pool_max_idle_per_host(0)
}

async fn client(url: &Url, timeout: Duration) -> Result<Client, ErrorCode> {
    let mut builder = transport_builder(timeout);
    if let Some(url::Host::Domain(host)) = url.host() {
        let port = url
            .port_or_known_default()
            .ok_or(ErrorCode::UnsupportedTarget)?;
        let addresses = resolve(host, port, timeout).await?;
        // Pin the vetted addresses for this request while retaining TLS hostname
        // verification. A second resolver lookup cannot rebind the destination.
        builder = builder.resolve_to_addrs(host, &addresses.into_iter().collect::<Vec<_>>());
    }
    builder.build().map_err(|_| ErrorCode::TransportUnavailable)
}

pub async fn execute(
    config: &HttpDestination,
    material: DeliveryMaterial,
    cancel: CancellationToken,
) -> DeliveryOutcome {
    if cancel.is_cancelled() {
        return DeliveryOutcome::failed(ErrorCode::Cancelled);
    }
    if let Err(error) = validate(config) {
        return DeliveryOutcome::failed(error);
    }
    let timeout = Duration::from_secs(config.timeout_secs);
    let deadline = tokio::time::Instant::now() + timeout;
    let work = async {
        let mut url = endpoint(config)?;
        let client = client(&url, timeout).await?;
        if !config.query.is_empty() {
            let mut query = url.query_pairs_mut();
            for row in &config.query {
                query.append_pair(&row.name, &material.render(&row.value)?);
            }
        }
        let mut headers = HeaderMap::new();
        for row in &config.headers {
            let mut value = HeaderValue::from_str(&material.render(&row.value)?)
                .map_err(|_| ErrorCode::InvalidRequest)?;
            value.set_sensitive(true);
            headers.insert(
                HeaderName::from_bytes(row.name.as_bytes())
                    .map_err(|_| ErrorCode::InvalidRequest)?,
                value,
            );
        }
        let method =
            Method::from_bytes(config.method.as_bytes()).map_err(|_| ErrorCode::InvalidRequest)?;
        let mut request = client.request(method, url).headers(headers);
        if let Some(body) = &config.body {
            let (encoded, content_type) = match body {
                HttpBody::Text(value) => (material.render(value)?, "text/plain; charset=utf-8"),
                HttpBody::Form(rows) => {
                    let mut encoded = url::form_urlencoded::Serializer::new(String::new());
                    for row in rows {
                        encoded.append_pair(&row.name, &material.render(&row.value)?);
                    }
                    (
                        Zeroizing::new(encoded.finish()),
                        "application/x-www-form-urlencoded",
                    )
                }
                HttpBody::Json(rows) => {
                    // Serialize one string at a time: do not retain a generic
                    // JSON tree containing cloned secret values after encoding.
                    let mut encoded = Zeroizing::new(String::from("{"));
                    for (index, row) in rows.iter().enumerate() {
                        if index > 0 {
                            encoded.push(',');
                        }
                        let key = serde_json::to_string(&row.name)
                            .map_err(|_| ErrorCode::InvalidRequest)?;
                        let value = Zeroizing::new(
                            serde_json::to_string(&*material.render(&row.value)?)
                                .map_err(|_| ErrorCode::InvalidRequest)?,
                        );
                        encoded.push_str(&key);
                        encoded.push(':');
                        encoded.push_str(&value);
                    }
                    encoded.push('}');
                    (encoded, "application/json")
                }
            };
            request = request
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(encoded.as_bytes().to_vec());
        }
        let request = request.build().map_err(|_| ErrorCode::InvalidRequest)?;
        Ok::<_, ErrorCode>((client, request))
    };
    let ready = tokio::select! {
        biased;
        _ = cancel.cancelled() => return DeliveryOutcome::failed(ErrorCode::Cancelled),
        result = tokio::time::timeout_at(deadline, work) => result.unwrap_or(Err(ErrorCode::Expired)),
    };
    let (client, request) = match ready {
        Ok(v) => v,
        Err(e) => return DeliveryOutcome::failed(e),
    };
    if cancel.is_cancelled() {
        return DeliveryOutcome::failed(ErrorCode::Cancelled);
    }
    // Beyond this point the server might act even if cancellation, a timeout or
    // a lost reply prevents completion. Never classify it as safe to retry.
    let exchange = exchange(client, request);
    tokio::select! {
        biased;
        _ = cancel.cancelled() => DeliveryOutcome::uncertain(ErrorCode::Cancelled),
        result = tokio::time::timeout_at(deadline, exchange) => match result {
            Ok(Ok(true)) => DeliveryOutcome::completed(),
            Ok(Ok(false)) => DeliveryOutcome::failed(ErrorCode::Unavailable).after_dispatch(),
            Ok(Err(error)) => DeliveryOutcome::uncertain(error),
            Err(_) => DeliveryOutcome::uncertain(ErrorCode::Expired),
        },
    }
}

async fn exchange(client: Client, request: reqwest::Request) -> Result<bool, ErrorCode> {
    let mut response = client
        .execute(request)
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?;
    let success = response.status().is_success();
    let mut bytes = 0usize;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ErrorCode::TransportUncertain)?
    {
        bytes = bytes.checked_add(chunk.len()).ok_or(ErrorCode::Capacity)?;
        if bytes > MAX_DELIVERY_OUTPUT_BYTES {
            return Err(ErrorCode::Capacity);
        }
        // Deliberately discarded, including transformed/encoded echoes.
    }
    Ok(success)
}

#[cfg(test)]
mod tests;
