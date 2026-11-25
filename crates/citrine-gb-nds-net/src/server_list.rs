use std::cmp::Ordering;
use std::net::{Ipv4Addr, SocketAddrV4};

const PRIVATE_IP: u8 = 0x02;
const CONNECT_NEGOTIATE: u8 = 0x04;
const NONSTANDARD_PORT: u8 = 0x10;
const NONSTANDARD_PRIVATE_PORT: u8 = 0x20;
const HAS_KEYS: u8 = 0x40;
const WHOLE_STRING: u8 = 0xFF;
const EXITING: &str = "2";
const OPERATORS: [&str; 7] = ["!=", "<>", "<=", ">=", "=", "<", ">"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Host {
    pub session: [u8; 4],
    pub public: SocketAddrV4,
    keys: Vec<(String, String)>,
}

impl Host {
    pub fn from_heartbeat(public: SocketAddrV4, data: &[u8]) -> Option<Self> {
        let session = data.get(1..5)?.try_into().ok()?;
        let mut parts = data.get(5..)?.split(|&byte| byte == 0);
        let mut keys: Vec<(String, String)> = Vec::new();
        while let Some(key) = parts.next() {
            if key.is_empty() {
                break;
            }
            let key = String::from_utf8_lossy(key).into_owned();
            let value = String::from_utf8_lossy(parts.next().unwrap_or_default()).into_owned();
            match keys.iter_mut().find(|(known, _)| *known == key) {
                Some(slot) => slot.1 = value,
                None => keys.push((key, value)),
            }
        }
        Some(Self {
            session,
            public,
            keys,
        })
    }

    pub fn listed(public: SocketAddrV4, keys: Vec<(String, String)>) -> Self {
        Self {
            session: [0; 4],
            public,
            keys,
        }
    }

    pub fn keys(&self) -> &[(String, String)] {
        &self.keys
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(known, _)| known == key)
            .map(|(_, value)| value.as_str())
    }

    pub fn game(&self) -> &str {
        self.value("gamename").unwrap_or_default()
    }

    pub fn is_leaving(&self) -> bool {
        self.value("statechanged") == Some(EXITING)
    }

    pub fn matches(&self, filter: &str) -> bool {
        let filter = filter.replace(['(', ')'], " ");
        let filter = filter.trim();
        filter.is_empty()
            || clauses(filter)
                .iter()
                .all(|clause| self.clause_holds(clause))
    }

    fn clause_holds(&self, clause: &str) -> bool {
        let lower = clause.to_ascii_lowercase();
        if lower.contains(" or ") || lower.contains(" like ") {
            return false;
        }
        let Some((at, operator)) = OPERATORS
            .iter()
            .filter_map(|operator| clause.find(operator).map(|at| (at, *operator)))
            .min_by_key(|(at, _)| *at)
        else {
            return false;
        };
        let left = self.operand(&clause[..at]);
        let right = self.operand(&clause[at + operator.len()..]);
        let ordering = match (left.parse::<i64>(), right.parse::<i64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            _ => left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase()),
        };
        match operator {
            "=" => ordering == Ordering::Equal,
            "!=" | "<>" => ordering != Ordering::Equal,
            "<" => ordering == Ordering::Less,
            ">" => ordering == Ordering::Greater,
            "<=" => ordering != Ordering::Greater,
            _ => ordering != Ordering::Less,
        }
    }

    fn operand<'a>(&'a self, text: &'a str) -> &'a str {
        let text = text.trim();
        if let Some(quoted) = text
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
        {
            return quoted;
        }
        if text.parse::<i64>().is_ok() {
            return text;
        }
        self.value(text).unwrap_or_default()
    }

    pub fn write_entry(&self, fields: &[&str], out: &mut Vec<u8>) {
        let private_ip = self
            .value("localip0")
            .and_then(|text| text.parse::<Ipv4Addr>().ok())
            .unwrap_or(*self.public.ip());
        let private_port = self
            .value("localport")
            .and_then(|text| text.parse::<u16>().ok())
            .unwrap_or(self.public.port());
        let mut flags = HAS_KEYS | NONSTANDARD_PORT | PRIVATE_IP | NONSTANDARD_PRIVATE_PORT;
        if self.value("natneg") == Some("1") {
            flags |= CONNECT_NEGOTIATE;
        }
        out.push(flags);
        out.extend_from_slice(&self.public.ip().octets());
        out.extend_from_slice(&self.public.port().to_be_bytes());
        out.extend_from_slice(&private_ip.octets());
        out.extend_from_slice(&private_port.to_be_bytes());
        for field in fields {
            out.push(WHOLE_STRING);
            out.extend_from_slice(self.value(field).unwrap_or_default().as_bytes());
            out.push(0);
        }
    }
}

fn clauses(filter: &str) -> Vec<&str> {
    let lower = filter.to_ascii_lowercase();
    let mut clauses = Vec::new();
    let mut start = 0;
    while let Some(found) = lower[start..].find(" and ") {
        clauses.push(&filter[start..start + found]);
        start += found + " and ".len();
    }
    clauses.push(&filter[start..]);
    clauses
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heartbeat(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut data = vec![3, 1, 2, 3, 4];
        for (key, value) in pairs {
            data.extend_from_slice(key.as_bytes());
            data.push(0);
            data.extend_from_slice(value.as_bytes());
            data.push(0);
        }
        data.push(0);
        data
    }

    fn host(pairs: &[(&str, &str)]) -> Host {
        Host::from_heartbeat(
            SocketAddrV4::new(Ipv4Addr::new(10, 77, 1, 1), 56400),
            &heartbeat(pairs),
        )
        .unwrap()
    }

    #[test]
    fn a_heartbeat_keeps_its_session_and_the_last_value_of_each_key() {
        let host = host(&[
            ("gamename", "mariokartds"),
            ("unknown", "1"),
            ("unknown", "3"),
            ("dwc_pid", "603819417"),
        ]);
        assert_eq!(host.session, [1, 2, 3, 4]);
        assert_eq!(host.game(), "mariokartds");
        assert_eq!(host.value("unknown"), Some("3"));
        assert_eq!(host.value("dwc_pid"), Some("603819417"));
        assert!(!host.is_leaving());
    }

    #[test]
    fn a_filter_compares_numbers_strings_and_other_keys() {
        let host = host(&[
            ("dwc_pid", "603819417"),
            ("dwc_mresv", "0"),
            ("dwc_mver", "90"),
            ("numplayers", "1"),
            ("kart_region", "Europe"),
        ]);
        assert!(host.matches(""));
        assert!(host.matches("dwc_pid = 603819417"));
        assert!(!host.matches("dwc_pid != 603819417"));
        assert!(host.matches("dwc_mver = 90 AND numplayers < 3 and dwc_mresv != dwc_pid"));
        assert!(!host.matches("dwc_mver = 90 and numplayers >= 3"));
        assert!(host.matches("kart_region = 'europe'"));
        assert!(host.matches("numplayers <> 2 and numplayers <= 1"));
        assert!(!host.matches("missing > 0"));
    }

    #[test]
    fn a_filter_it_cannot_read_matches_nothing() {
        let host = host(&[("dwc_pid", "603819417")]);
        assert!(!host.matches("dwc_pid = 1 or dwc_pid = 603819417"));
        assert!(!host.matches("dwc_pid > 0 and (dwc_pid = 1 or dwc_pid = 2)"));
        assert!(!host.matches("dwc_pid"));
    }

    #[test]
    fn brackets_around_and_clauses_change_nothing() {
        let host = host(&[("dwc_pid", "603819417"), ("dwc_mver", "90")]);
        assert!(host.matches("(dwc_pid = 603819417)"));
        assert!(host.matches("dwc_mver = 90 and (dwc_pid != 1 and (dwc_mver < 91))"));
        assert!(!host.matches("dwc_mver = 90 and (dwc_pid = 1)"));
    }

    #[test]
    fn an_entry_carries_both_addresses_and_the_asked_keys() {
        let host = host(&[
            ("localip0", "192.168.0.7"),
            ("localport", "56401"),
            ("natneg", "1"),
            ("dwc_pid", "603819417"),
        ]);
        let mut entry = Vec::new();
        host.write_entry(&["dwc_pid", "kart_elo"], &mut entry);
        let mut expected = vec![0x76, 10, 77, 1, 1, 0xDC, 0x50, 192, 168, 0, 7, 0xDC, 0x51];
        expected.extend_from_slice(b"\xFF603819417\0\xFF\0");
        assert_eq!(entry, expected);
    }

    #[test]
    fn a_heartbeat_with_state_two_is_leaving() {
        assert!(host(&[("statechanged", "2")]).is_leaving());
    }
}
