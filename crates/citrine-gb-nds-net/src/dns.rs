use std::net::Ipv4Addr;

const HEADER_BYTES: usize = 12;
const TYPE_A: u16 = 1;
const CLASS_IN: u16 = 1;
const TTL_SECONDS: u32 = 60;
const NAME_ERROR: u16 = 3;
const NOT_IMPLEMENTED: u16 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub name: String,
    pub wants_address: bool,
    bytes: Vec<u8>,
}

impl Query {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let header = bytes.get(..HEADER_BYTES)?;
        if header[2] & 0x80 != 0 || u16::from_be_bytes([header[4], header[5]]) != 1 {
            return None;
        }
        let mut at = HEADER_BYTES;
        let mut labels = Vec::new();
        loop {
            let length = usize::from(*bytes.get(at)?);
            at += 1;
            if length == 0 {
                break;
            }
            if length > 63 {
                return None;
            }
            labels.push(std::str::from_utf8(bytes.get(at..at + length)?).ok()?);
            at += length;
        }
        let kind = u16::from_be_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]);
        let class = u16::from_be_bytes([*bytes.get(at + 2)?, *bytes.get(at + 3)?]);
        Some(Self {
            name: labels.join("."),
            wants_address: kind == TYPE_A && class == CLASS_IN,
            bytes: bytes[..at + 4].to_vec(),
        })
    }

    pub fn answer(&self, addresses: &[Ipv4Addr]) -> Vec<u8> {
        let mut bytes = self.bytes.clone();
        let code = match (self.wants_address, addresses.is_empty()) {
            (false, _) => NOT_IMPLEMENTED,
            (true, true) => NAME_ERROR,
            (true, false) => 0,
        };
        let answers = if self.wants_address { addresses } else { &[] };
        let recursion = u16::from(bytes[2] & 1) << 8;
        bytes[2..4].copy_from_slice(&(0x8080 | recursion | code).to_be_bytes());
        bytes[6..8].copy_from_slice(&(answers.len() as u16).to_be_bytes());
        bytes[8..12].fill(0);
        for address in answers {
            bytes.extend_from_slice(&[0xC0, HEADER_BYTES as u8]);
            bytes.extend_from_slice(&TYPE_A.to_be_bytes());
            bytes.extend_from_slice(&CLASS_IN.to_be_bytes());
            bytes.extend_from_slice(&TTL_SECONDS.to_be_bytes());
            bytes.extend_from_slice(&4u16.to_be_bytes());
            bytes.extend_from_slice(&address.octets());
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(name: &str, kind: u16) -> Vec<u8> {
        let mut bytes = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in name.split('.') {
            bytes.push(label.len() as u8);
            bytes.extend_from_slice(label.as_bytes());
        }
        bytes.push(0);
        bytes.extend_from_slice(&kind.to_be_bytes());
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes
    }

    #[test]
    fn an_a_query_reads_its_name_and_answers_with_each_address() {
        let asked = query("conntest.nintendowifi.net", 1);
        let parsed = Query::parse(&asked).unwrap();
        assert_eq!(parsed.name, "conntest.nintendowifi.net");
        assert!(parsed.wants_address);
        let answer = parsed.answer(&[Ipv4Addr::new(10, 77, 0, 1), Ipv4Addr::new(1, 2, 3, 4)]);
        assert_eq!(&answer[..2], &[0x12, 0x34]);
        assert_eq!(&answer[2..4], &[0x81, 0x80]);
        assert_eq!(&answer[4..12], &[0, 1, 0, 2, 0, 0, 0, 0]);
        assert_eq!(&answer[12..asked.len()], &asked[12..]);
        let first = &answer[asked.len()..asked.len() + 16];
        assert_eq!(
            first,
            &[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 10, 77, 0, 1]
        );
        assert_eq!(&answer[answer.len() - 4..], &[1, 2, 3, 4]);
    }

    #[test]
    fn no_address_is_a_name_error_and_other_types_are_not_implemented() {
        let missing = Query::parse(&query("nowhere.invalid", 1))
            .unwrap()
            .answer(&[]);
        assert_eq!(missing[3] & 0xF, 3);
        assert_eq!(&missing[6..8], &[0, 0]);
        let other = Query::parse(&query("example.com", 28)).unwrap();
        assert!(!other.wants_address);
        let answer = other.answer(&[Ipv4Addr::LOCALHOST]);
        assert_eq!(answer[3] & 0xF, 4);
        assert_eq!(&answer[6..8], &[0, 0]);
    }

    #[test]
    fn answers_and_broken_queries_are_refused() {
        let mut answer = query("example.com", 1);
        answer[2] |= 0x80;
        assert_eq!(Query::parse(&answer), None);
        let short = query("example.com", 1);
        assert_eq!(Query::parse(&short[..short.len() - 3]), None);
        assert_eq!(Query::parse(&[0; 5]), None);
    }
}
