use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

pub fn client_address(headers: &HeaderMap, peer: SocketAddr, hops: usize) -> IpAddr {
    let peer = peer.ip();
    if hops == 0 {
        return peer;
    }

    let lines: Vec<&str> = match headers
        .get_all("x-forwarded-for")
        .iter()
        .map(|value| value.to_str())
        .collect()
    {
        Ok(lines) => lines,
        Err(_) => return peer,
    };
    if !lines.is_empty() {
        let chain: Vec<&str> = lines
            .iter()
            .flat_map(|line| line.split(','))
            .map(str::trim)
            .collect();
        return chain
            .len()
            .checked_sub(hops)
            .and_then(|at| chain.get(at))
            .and_then(|raw| raw.parse().ok())
            .unwrap_or(peer);
    }

    headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(peer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use axum::http::header::HeaderName;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        headers
    }

    fn peer(text: &str) -> SocketAddr {
        SocketAddr::new(text.parse().unwrap(), 40_000)
    }

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn with_nothing_in_front_the_socket_is_the_answer_and_the_header_is_noise() {
        let sent = headers(&[("x-forwarded-for", "203.0.113.7")]);
        assert_eq!(
            client_address(&sent, peer("198.51.100.4"), 0),
            ip("198.51.100.4")
        );
    }

    #[test]
    fn behind_one_proxy_the_entry_that_proxy_wrote_is_the_client() {
        let sent = headers(&[("x-forwarded-for", "1.1.1.1, 203.0.113.7")]);
        assert_eq!(
            client_address(&sent, peer("10.0.0.1"), 1),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn a_spoofed_chain_cannot_reach_past_the_proxies_we_run() {
        let sent = headers(&[("x-forwarded-for", "9.9.9.9, 8.8.8.8, 203.0.113.7")]);
        assert_eq!(
            client_address(&sent, peer("10.0.0.1"), 1),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn two_proxies_of_our_own_read_two_from_the_right() {
        let sent = headers(&[("x-forwarded-for", "9.9.9.9, 203.0.113.7, 10.0.0.2")]);
        assert_eq!(
            client_address(&sent, peer("10.0.0.1"), 2),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn a_chain_shorter_than_our_proxies_falls_back_rather_than_believing_it() {
        let sent = headers(&[("x-forwarded-for", "203.0.113.7")]);
        assert_eq!(client_address(&sent, peer("10.0.0.1"), 2), ip("10.0.0.1"));
    }

    #[test]
    fn a_missing_or_unreadable_chain_falls_back_to_the_proxys_own_header_then_the_socket() {
        assert_eq!(
            client_address(
                &headers(&[("x-real-ip", "203.0.113.7")]),
                peer("10.0.0.1"),
                1
            ),
            ip("203.0.113.7")
        );
        assert_eq!(
            client_address(
                &headers(&[("x-forwarded-for", "junk")]),
                peer("10.0.0.1"),
                1
            ),
            ip("10.0.0.1")
        );
        assert_eq!(
            client_address(&HeaderMap::new(), peer("10.0.0.1"), 1),
            ip("10.0.0.1")
        );
    }

    #[test]
    fn a_forged_first_header_line_cannot_hide_the_line_our_proxy_added() {
        let sent = headers(&[
            ("x-forwarded-for", "9.9.9.9"),
            ("x-forwarded-for", "203.0.113.7"),
        ]);
        assert_eq!(
            client_address(&sent, peer("10.0.0.1"), 1),
            ip("203.0.113.7")
        );
    }

    #[test]
    fn a_forged_real_ip_is_ignored_when_a_chain_was_sent() {
        let sent = headers(&[("x-forwarded-for", "junk"), ("x-real-ip", "9.9.9.9")]);
        assert_eq!(client_address(&sent, peer("10.0.0.1"), 1), ip("10.0.0.1"));
    }

    #[test]
    fn an_ipv6_client_survives_the_chain() {
        let sent = headers(&[("x-forwarded-for", "1.1.1.1, 2001:db8::1")]);
        assert_eq!(
            client_address(&sent, peer("10.0.0.1"), 1),
            ip("2001:db8::1")
        );
    }
}
