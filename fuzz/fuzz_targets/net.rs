#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use citrine_gb_net::{
    ClientMsg, STUN_ID_BYTES, Sealed, ServerMsg, StunId, check_screen, is_secure_start, is_stun,
    peer_hello, screen_size, stun_answer, stun_mapped,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = ClientMsg::decode(data);
    let _ = ServerMsg::decode(data);
    let _ = Sealed::from_bytes(data);
    let _ = is_stun(data);
    let v4 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)), 40000);
    let v6 = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 3478);
    let _ = stun_answer(data, v4);
    let _ = stun_answer(data, v6);
    let expect: StunId = data
        .get(8..8 + STUN_ID_BYTES)
        .and_then(|id| id.try_into().ok())
        .unwrap_or([0; STUN_ID_BYTES]);
    let _ = stun_mapped(data, &expect);
    let _ = peer_hello(data);
    let _ = is_secure_start(data);
    let _ = screen_size(data);
    let _ = check_screen(data);
});
