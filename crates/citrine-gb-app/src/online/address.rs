use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerAddress {
    secure: bool,
    host: String,
    port: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("Enter the server's address")]
    Empty,
    #[error("That isn't a server address")]
    Invalid,
}

impl ServerAddress {
    pub fn parse(input: &str) -> Result<Self, AddressError> {
        let text = input.trim().to_lowercase();
        if text.is_empty() {
            return Err(AddressError::Empty);
        }
        let (scheme, rest) = match text.split_once("://") {
            Some((scheme, rest)) => (Some(scheme), rest),
            None => (None, text.as_str()),
        };
        let secure = match scheme {
            Some("https" | "wss") => Some(true),
            Some("http" | "ws") => Some(false),
            Some(_) => return Err(AddressError::Invalid),
            None => None,
        };
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .rsplit('@')
            .next()
            .unwrap_or_default();
        let (host, port) = split_port(authority)?;
        if !valid_host(&host) {
            return Err(AddressError::Invalid);
        }
        let secure = secure.unwrap_or_else(|| !local_host(&host));
        let default_port = if secure { 443 } else { 80 };
        Ok(Self {
            secure,
            host,
            port: port.filter(|port| *port != default_port),
        })
    }

    pub fn key(&self) -> String {
        let scheme = match (self.secure, local_host(&self.host)) {
            (true, true) => "https://",
            (false, false) => "http://",
            _ => "",
        };
        match self.port {
            Some(port) => format!("{scheme}{}:{port}", self.host),
            None => format!("{scheme}{}", self.host),
        }
    }

    pub fn relay_url(&self) -> String {
        let scheme = if self.secure { "wss" } else { "ws" };
        match self.port {
            Some(port) => format!("{scheme}://{}:{port}/relay", self.host),
            None => format!("{scheme}://{}/relay", self.host),
        }
    }

    pub fn web_url(&self, path: &str) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        match self.port {
            Some(port) => format!("{scheme}://{}:{port}{path}", self.host),
            None => format!("{scheme}://{}{path}", self.host),
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

impl fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

fn split_port(authority: &str) -> Result<(String, Option<u16>), AddressError> {
    let (host, port) = if let Some(rest) = authority.strip_prefix('[') {
        let (inner, after) = rest.split_once(']').ok_or(AddressError::Invalid)?;
        let host = format!("[{inner}]");
        match after.strip_prefix(':') {
            Some(port) => (host, Some(port)),
            None if after.is_empty() => (host, None),
            None => return Err(AddressError::Invalid),
        }
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (authority.to_owned(), None),
        }
    };
    let port = match port {
        Some(port) => Some(port.parse::<u16>().map_err(|_| AddressError::Invalid)?),
        None => None,
    };
    if port == Some(0) {
        return Err(AddressError::Invalid);
    }
    Ok((host, port))
}

fn valid_host(host: &str) -> bool {
    if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return inner.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !host.is_empty()
        && host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_alphanumeric() || c == '-')
        })
}

fn local_host(host: &str) -> bool {
    host == "localhost"
        || host.ends_with(".local")
        || host.parse::<std::net::Ipv4Addr>().is_ok()
        || host.starts_with('[')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ServerAddress {
        ServerAddress::parse(text).unwrap_or_else(|err| panic!("{text}: {err}"))
    }

    #[test]
    fn a_bare_domain_means_wss() {
        let address = parse("gb.lemon.industries");
        assert_eq!(address.relay_url(), "wss://gb.lemon.industries/relay");
        assert_eq!(address.key(), "gb.lemon.industries");
    }

    #[test]
    fn the_same_server_written_differently_has_one_key() {
        for text in [
            "gb.lemon.industries",
            " GB.Lemon.Industries ",
            "https://gb.lemon.industries/",
            "wss://gb.lemon.industries/relay",
            "gb.lemon.industries:443",
            "https://gb.lemon.industries:443/play?x=1",
        ] {
            assert_eq!(parse(text).key(), "gb.lemon.industries", "{text}");
        }
    }

    #[test]
    fn a_port_is_kept() {
        let address = parse("gb.lemon.industries:8443");
        assert_eq!(address.key(), "gb.lemon.industries:8443");
        assert_eq!(address.relay_url(), "wss://gb.lemon.industries:8443/relay");
    }

    #[test]
    fn local_and_ip_addresses_use_plain_ws() {
        assert_eq!(
            parse("localhost:13707").relay_url(),
            "ws://localhost:13707/relay"
        );
        assert_eq!(
            parse("192.168.1.5:13707").relay_url(),
            "ws://192.168.1.5:13707/relay"
        );
        assert_eq!(parse("[::1]:13707").relay_url(), "ws://[::1]:13707/relay");
        assert_eq!(parse("192.168.1.5:13707").key(), "192.168.1.5:13707");
    }

    #[test]
    fn a_scheme_wins_over_the_guess() {
        let plain = parse("http://relay.example.org:13707");
        assert_eq!(plain.relay_url(), "ws://relay.example.org:13707/relay");
        assert_eq!(plain.key(), "http://relay.example.org:13707");
        assert_ne!(plain.key(), parse("relay.example.org:13707").key());
        let secure = parse("https://localhost");
        assert_eq!(secure.relay_url(), "wss://localhost/relay");
        assert_eq!(secure.key(), "https://localhost");
    }

    #[test]
    fn a_key_reads_back_as_the_same_address() {
        for text in [
            "gb.lemon.industries",
            "x.org:8443",
            "http://x.org",
            "localhost:13707",
            "https://localhost",
            "[::1]",
        ] {
            let address = parse(text);
            assert_eq!(parse(&address.key()), address, "{text}");
        }
    }

    #[test]
    fn nonsense_is_refused() {
        assert_eq!(ServerAddress::parse("  "), Err(AddressError::Empty));
        for text in [
            "ftp://x.org",
            "a b.org",
            "x.org:port",
            "x.org:0",
            "x.org:70000",
            "-x.org",
            "x..org",
            "[nope]",
        ] {
            assert_eq!(
                ServerAddress::parse(text),
                Err(AddressError::Invalid),
                "{text}"
            );
        }
    }
}
