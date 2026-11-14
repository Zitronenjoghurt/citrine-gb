use crate::h8::Fault;
use crate::ir::{
    CMD_ADVERTISE, CMD_ASSERT_MASTER, CMD_IDENTITY_REQ, CMD_SLAVE_ACK, Packet, WIRE_XOR,
};
use crate::walker::{CLOCK_HZ, Walker};

pub const MASTER_SESSION: u32 = 0x4C41_4221;
const QUIET: u64 = 1500;
const SLICE: u64 = 500;

#[derive(Debug)]
pub enum HarnessError {
    Fault(Fault),
    Timeout(&'static str),
    Unexpected(Packet),
    Garbled(Vec<u8>),
}

impl From<Fault> for HarnessError {
    fn from(f: Fault) -> Self {
        HarnessError::Fault(f)
    }
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub walker_session: u32,
    pub session: u32,
    pub reply: Packet,
}

pub fn wait_for_advert(w: &mut Walker, seconds: f64) -> Result<(), HarnessError> {
    let end = w.now() + (seconds * CLOCK_HZ as f64) as u64;
    while w.now() < end {
        w.run_cycles(SLICE)?;
        if w.ir_take()
            .iter()
            .any(|&(_, b)| b ^ WIRE_XOR == CMD_ADVERTISE)
        {
            return Ok(());
        }
    }
    Err(HarnessError::Timeout("no advertising byte"))
}

pub fn read_packet(w: &mut Walker, seconds: f64) -> Result<Packet, HarnessError> {
    let end = w.now() + (seconds * CLOCK_HZ as f64) as u64;
    let mut bytes: Vec<u8> = Vec::new();
    let mut last = 0u64;
    while w.now() < end {
        w.run_cycles(SLICE)?;
        for (t, b) in w.ir_take() {
            bytes.push(b);
            last = t;
        }
        if !bytes.is_empty() && w.now() - last > QUIET {
            return Packet::from_wire(&bytes).ok_or_else(|| {
                HarnessError::Garbled(bytes.iter().map(|b| b ^ WIRE_XOR).collect())
            });
        }
    }
    Err(HarnessError::Timeout("no packet"))
}

pub fn send(w: &mut Walker, p: &Packet) {
    w.ir_receive(&p.wire());
}

pub fn identify(w: &mut Walker) -> Result<Identity, HarnessError> {
    wait_for_advert(w, 10.0)?;
    w.run_cycles(CLOCK_HZ / 1000)?;
    send(
        w,
        &Packet::new(CMD_ASSERT_MASTER, 0x01, MASTER_SESSION, &[]),
    );
    let ack = read_packet(w, 1.0)?;
    if ack.cmd != CMD_SLAVE_ACK {
        return Err(HarnessError::Unexpected(ack));
    }
    let session = ack.session ^ MASTER_SESSION;
    send(w, &Packet::new(CMD_IDENTITY_REQ, 0x01, session, &[]));
    let reply = read_packet(w, 1.0)?;
    Ok(Identity {
        walker_session: ack.session,
        session,
        reply,
    })
}

pub fn exchange(w: &mut Walker, p: &Packet) -> Result<Packet, HarnessError> {
    send(w, p);
    read_packet(w, 1.0)
}

pub fn registered_info(info: &[u8], tid: u16, sync: u32) -> Vec<u8> {
    let mut out = info.to_vec();
    out.resize(0x68, 0);
    out[0x0C..0x0E].copy_from_slice(&tid.to_le_bytes());
    out[0x0E..0x10].copy_from_slice(&0x2A2Au16.to_le_bytes());
    for (i, b) in out[0x10..0x38].iter_mut().enumerate() {
        *b = i as u8 ^ 0x5C;
    }
    out[0x5B] |= 0x01;
    out[0x60..0x64].copy_from_slice(&sync.to_be_bytes());
    out
}

pub fn connect(
    w: &mut Walker,
    commands: &[(u8, u8, Vec<u8>)],
) -> Result<Vec<Packet>, HarnessError> {
    let id = identify(w)?;
    let mut replies = vec![id.reply.clone()];
    for (cmd, extra, payload) in commands {
        let payload = if *cmd == 0x32 && payload.is_empty() {
            registered_info(&id.reply.payload, 0x3039, 0x2000_0000)
        } else {
            payload.clone()
        };
        replies.push(exchange(
            w,
            &Packet::new(*cmd, *extra, id.session, &payload),
        )?);
    }
    Ok(replies)
}
