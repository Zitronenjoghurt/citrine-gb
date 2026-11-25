use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

pub const STUN_PORT: u16 = 3478;
pub const STUN_ID_BYTES: usize = 12;

const COOKIE: [u8; 4] = [0x21, 0x12, 0xa4, 0x42];
const HEADER: usize = 20;
const BINDING_REQUEST: u16 = 0x0001;
const BINDING_SUCCESS: u16 = 0x0101;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;

pub type StunId = [u8; STUN_ID_BYTES];

pub fn stun_request(id: &StunId) -> Vec<u8> {
    header(BINDING_REQUEST, 0, id)
}

pub fn stun_answer(packet: &[u8], from: SocketAddr) -> Option<Vec<u8>> {
    let (kind, id, _) = parse(packet)?;
    if kind != BINDING_REQUEST {
        return None;
    }
    let mut value = vec![0, 0];
    let port = from.port() ^ u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
    match from.ip().to_canonical() {
        IpAddr::V4(ip) => {
            value[1] = 1;
            value.extend_from_slice(&port.to_be_bytes());
            value.extend(ip.octets().iter().zip(COOKIE).map(|(a, b)| a ^ b));
        }
        IpAddr::V6(ip) => {
            value[1] = 2;
            value.extend_from_slice(&port.to_be_bytes());
            value.extend(ip.octets().iter().zip(mask(&id)).map(|(a, b)| a ^ b));
        }
    }
    let mut out = header(BINDING_SUCCESS, 4 + value.len(), &id);
    out.extend_from_slice(&XOR_MAPPED_ADDRESS.to_be_bytes());
    out.extend_from_slice(&(value.len() as u16).to_be_bytes());
    out.extend(value);
    Some(out)
}

pub fn stun_mapped(packet: &[u8], expect: &StunId) -> Option<SocketAddr> {
    let (kind, id, mut attrs) = parse(packet)?;
    if kind != BINDING_SUCCESS || &id != expect {
        return None;
    }
    while attrs.len() >= 4 {
        let kind = u16::from_be_bytes([attrs[0], attrs[1]]);
        let len = usize::from(u16::from_be_bytes([attrs[2], attrs[3]]));
        let value = attrs.get(4..4 + len)?;
        if kind == XOR_MAPPED_ADDRESS {
            return mapped(value, &id);
        }
        attrs = attrs
            .get((4 + len).next_multiple_of(4)..)
            .unwrap_or_default();
    }
    None
}

pub fn is_stun(packet: &[u8]) -> bool {
    packet.len() >= HEADER && packet[0] & 0xc0 == 0 && packet[4..8] == COOKIE
}

fn mapped(value: &[u8], id: &StunId) -> Option<SocketAddr> {
    let port = u16::from_be_bytes([*value.get(2)?, *value.get(3)?])
        ^ u16::from_be_bytes([COOKIE[0], COOKIE[1]]);
    let ip = match (value.get(1)?, value.get(4..)?) {
        (1, x) if x.len() == 4 => {
            let mut ip = [0; 4];
            for (out, (a, b)) in ip.iter_mut().zip(x.iter().zip(COOKIE)) {
                *out = a ^ b;
            }
            IpAddr::V4(Ipv4Addr::from(ip))
        }
        (2, x) if x.len() == 16 => {
            let mut ip = [0; 16];
            for (out, (a, b)) in ip.iter_mut().zip(x.iter().zip(mask(id))) {
                *out = a ^ b;
            }
            IpAddr::V6(Ipv6Addr::from(ip))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

fn mask(id: &StunId) -> [u8; 16] {
    let mut mask = [0; 16];
    mask[..4].copy_from_slice(&COOKIE);
    mask[4..].copy_from_slice(id);
    mask
}

fn header(kind: u16, len: usize, id: &StunId) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + len);
    out.extend_from_slice(&kind.to_be_bytes());
    out.extend_from_slice(&(len as u16).to_be_bytes());
    out.extend_from_slice(&COOKIE);
    out.extend_from_slice(id);
    out
}

fn parse(packet: &[u8]) -> Option<(u16, StunId, &[u8])> {
    if !is_stun(packet) {
        return None;
    }
    let kind = u16::from_be_bytes([packet[0], packet[1]]);
    let len = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
    if !len.is_multiple_of(4) || packet.len() != HEADER + len {
        return None;
    }
    let id = packet[8..HEADER].try_into().ok()?;
    Some((kind, id, &packet[HEADER..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_ID: StunId = [
        0xb7, 0xe7, 0xa7, 0x01, 0xbc, 0x34, 0xd6, 0x86, 0xfa, 0x87, 0xdf, 0xae,
    ];

    fn rfc_response(attr: &[u8]) -> Vec<u8> {
        let software = [
            0x80, 0x22, 0x00, 0x0b, b't', b'e', b's', b't', b' ', b'v', b'e', b'c', b't', b'o',
            b'r', 0x20,
        ];
        let mut out = header(BINDING_SUCCESS, software.len() + attr.len(), &RFC_ID);
        out.extend_from_slice(&software);
        out.extend_from_slice(attr);
        out
    }

    #[test]
    fn the_rfc_5769_ipv4_response_reads_as_its_address() {
        let packet = rfc_response(&[
            0x00, 0x20, 0x00, 0x08, 0x00, 0x01, 0xa1, 0x47, 0xe1, 0x12, 0xa6, 0x43,
        ]);
        assert_eq!(
            stun_mapped(&packet, &RFC_ID),
            Some("192.0.2.1:32853".parse().unwrap())
        );
    }

    #[test]
    fn the_rfc_5769_ipv6_response_reads_as_its_address() {
        let packet = rfc_response(&[
            0x00, 0x20, 0x00, 0x14, 0x00, 0x02, 0xa1, 0x47, 0x01, 0x13, 0xa9, 0xfa, 0xa5, 0xd3,
            0xf1, 0x79, 0xbc, 0x25, 0xf4, 0xb5, 0xbe, 0xd2, 0xb9, 0xd9,
        ]);
        assert_eq!(
            stun_mapped(&packet, &RFC_ID),
            Some(
                "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
                    .parse()
                    .unwrap()
            )
        );
    }

    #[test]
    fn an_answer_tells_the_asker_its_own_address() {
        let id = [7; STUN_ID_BYTES];
        for from in [
            "203.0.113.9:40000",
            "[2001:db8::1]:5000",
            "[::ffff:10.1.2.3]:9",
        ] {
            let from: SocketAddr = from.parse().unwrap();
            let answer = stun_answer(&stun_request(&id), from).unwrap();
            let seen = stun_mapped(&answer, &id).unwrap();
            assert_eq!(seen.port(), from.port());
            assert_eq!(seen.ip(), from.ip().to_canonical());
        }
    }

    #[test]
    fn an_answer_for_another_request_is_ignored() {
        let from = "203.0.113.9:40000".parse().unwrap();
        let answer = stun_answer(&stun_request(&[1; STUN_ID_BYTES]), from).unwrap();
        assert_eq!(stun_mapped(&answer, &[2; STUN_ID_BYTES]), None);
    }

    #[test]
    fn only_binding_requests_get_answers() {
        let from = "203.0.113.9:40000".parse().unwrap();
        let request = stun_request(&[1; STUN_ID_BYTES]);
        let answer = stun_answer(&request, from).unwrap();
        assert_eq!(stun_answer(&answer, from), None);
        assert_eq!(stun_answer(b"GET / HTTP/1.1\r\n\r\n\r\n", from), None);
        assert_eq!(stun_answer(&request[..19], from), None);
        let mut long = request.clone();
        long.extend_from_slice(&[0; 3]);
        assert_eq!(stun_answer(&long, from), None);
    }
}
