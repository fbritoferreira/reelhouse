//! Finds Reelhouse servers on the local network by asking every address in the device's /24 for
//! `GET /api/v1/server`. Works with the server in a Docker bridge network, where mDNS does not.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 8420;
const PROBE_TIMEOUT: Duration = Duration::from_millis(800);

/// The device's LAN IPv4 address. Connecting a UDP socket sends nothing; it only picks a route.
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_unspecified() => Some(ip),
        _ => None,
    }
}

/// Every host in `local`'s /24 except network, broadcast and `local` itself.
pub fn neighbours(local: Ipv4Addr) -> Vec<IpAddr> {
    let [a, b, c, own] = local.octets();
    (1..=254u8)
        .filter(|&d| d != own)
        .map(|d| IpAddr::V4(Ipv4Addr::new(a, b, c, d)))
        .collect()
}

/// Base URLs (`http://host:port`) of the hosts that answer as a Reelhouse server.
pub async fn probe(hosts: &[IpAddr], port: u16) -> Vec<String> {
    let Ok(http) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return Vec::new();
    };
    let checks = hosts.iter().map(|host| {
        let http = http.clone();
        let base = format!("http://{host}:{port}");
        async move {
            let res = http
                .get(format!("{base}/api/v1/server"))
                .send()
                .await
                .ok()?;
            let info: reelhouse_api::ServerInfo = res.json().await.ok()?;
            info.features
                .iter()
                .any(|f| f == "libraries")
                .then_some(base)
        }
    });
    let mut found: Vec<String> = futures::future::join_all(checks)
        .await
        .into_iter()
        .flatten()
        .collect();
    found.sort();
    found
}

/// Servers on this device's network, on the default port.
pub async fn discover() -> Vec<String> {
    match local_ipv4() {
        Some(ip) => probe(&neighbours(ip), DEFAULT_PORT).await,
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn neighbours_cover_the_subnet_except_self() {
        let n = neighbours(Ipv4Addr::new(192, 168, 70, 5));
        assert_eq!(n.len(), 253);
        assert!(n.contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 70, 3))));
        assert!(!n.contains(&IpAddr::V4(Ipv4Addr::new(192, 168, 70, 5))));
    }

    #[tokio::test]
    async fn probe_finds_reelhouse_and_ignores_other_servers() {
        let reelhouse = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/v1/server"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"version": "0.1.0", "features": ["libraries"]})),
            )
            .mount(&reelhouse)
            .await;
        let other = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>router</html>"))
            .mount(&other)
            .await;

        let localhost = [IpAddr::V4(Ipv4Addr::LOCALHOST)];
        let port = reelhouse.address().port();
        assert_eq!(
            probe(&localhost, port).await,
            vec![format!("http://127.0.0.1:{port}")]
        );
        assert!(probe(&localhost, other.address().port()).await.is_empty());
    }
}
