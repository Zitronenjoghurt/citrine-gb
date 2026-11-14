pub mod advert;
pub mod image;
pub mod transfer;

use self::advert::{Adverts, Beacon, Offer};
use self::image::{Image, Parent};
use self::transfer::Transfer;
use crate::nds::wifi::frame::{self, FCS_BYTES, Frame, Kind, MP_REPLY, Rate, half};

pub const FIRST_REPLY_GAP_US: u64 = 16;
pub const SLOT_GAP_US: u64 = 10;
pub const OFFER_TIMEOUT_US: u64 = 3_000_000;
pub const SEND_GAP_US: u64 = 200;
pub const RETRY_US: u64 = 400_000;
pub const MAX_ATTEMPTS: u8 = 20;
pub const LEAVE_TIMEOUT_US: u64 = 50_000;
pub const BOOT_DELAY_US: u64 = 3_000_000;

const ASSOC_REQUEST: u8 = 0;
const ASSOC_RESPONSE: u8 = 1;
const DISASSOC: u8 = 10;
const AUTH: u8 = 11;
const DEAUTH: u8 = 12;
const REPLY_CONTROL: u16 = 0x0118;
const CAPABILITY: u16 = 0x0021;
const LISTEN_INTERVAL: u16 = 1;
const RATES: [u8; 4] = [0x01, 0x02, 0x82, 0x84];
const SSID_BYTES: usize = 0x20;
const LEAVING: u16 = 3;
const NAME_UNITS: usize = 10;
const SNIPPETS: u8 = 5;
const REPLY_HALVES: u8 = 4;
const REPLY_FLAGS: u8 = 0x81;
const REPLY_PAYLOAD: usize = 7;
const CMD_REPLY_TIME: usize = 0;
const CMD_CLIENTS: usize = 2;
const CMD_SIZE: usize = 4;
const CMD_COMMAND: usize = 6;
const CMD_DATA: usize = 7;
const CMD_PACKET: usize = CMD_DATA + 2;
const CMD_PACKET_DATA: usize = CMD_DATA + 4;
const PACKET_OVERHEAD: usize = 6;
const PACKET_FRAME_OVERHEAD: usize = 6;
const RSA_SIZE: u8 = 0x75;
const AID_MASK: u16 = 0x000F;
const CLIENT_BITS: u16 = 0xFFFE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Command {
    Dummy,
    NameRequest,
    Error,
    Rsa,
    Data,
    Final,
    Other(u8),
}

impl Command {
    fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Dummy,
            1 => Self::NameRequest,
            2 => Self::Error,
            3 => Self::Rsa,
            4 => Self::Data,
            5 => Self::Final,
            other => Self::Other(other),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum ReplyKind {
    Dummy = 0x00,
    Username = 0x07,
    Rsa = 0x08,
    Data = 0x09,
    GotAll = 0x0A,
    Final = 0x0B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Step {
    Auth,
    Assoc,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Joining {
    offer: Offer,
    step: Step,
    attempts: u8,
    retry_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Receiving {
    offer: Offer,
    aid: u16,
    transfer: Transfer,
    snippet: u8,
    last_cmd: Option<(u16, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Leaving {
    offer: Offer,
    image: Option<Image>,
    queued: bool,
    deadline: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
enum Stage {
    Listening,
    Joining(Joining),
    Receiving(Box<Receiving>),
    Leaving(Box<Leaving>),
    Ready(Box<Image>, Offer, u64),
    Failed(Offer),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress<'a> {
    Listening,
    Offered(&'a Offer),
    Joining(&'a Offer),
    Receiving {
        offer: &'a Offer,
        got: usize,
        total: usize,
    },
    Ready(&'a Offer),
    Failed(&'a Offer),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Download {
    mac: [u8; 6],
    name: Vec<u16>,
    color: u8,
    now: u64,
    sequence: u16,
    adverts: Adverts,
    stage: Stage,
    queued: Vec<Frame>,
    outbox: Vec<Frame>,
    #[cfg_attr(feature = "serde", serde(skip))]
    log: Vec<String>,
}

impl Download {
    pub fn new(mac: [u8; 6], name: &str, color: u8) -> Self {
        Self {
            mac,
            name: name.encode_utf16().take(NAME_UNITS).collect(),
            color,
            now: 0,
            sequence: 0,
            adverts: Adverts::new(),
            stage: Stage::Listening,
            queued: Vec::new(),
            outbox: Vec::new(),
            log: Vec::new(),
        }
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    pub fn now_us(&self) -> u64 {
        self.now
    }

    pub fn progress(&self) -> Progress<'_> {
        match &self.stage {
            Stage::Listening => match self.adverts.offers().next() {
                Some(offer) => Progress::Offered(offer),
                None => Progress::Listening,
            },
            Stage::Joining(joining) => Progress::Joining(&joining.offer),
            Stage::Receiving(receiving) => Progress::Receiving {
                offer: &receiving.offer,
                got: receiving.transfer.got_bytes(),
                total: receiving.transfer.bytes(),
            },
            Stage::Leaving(leaving) => match leaving.image {
                Some(_) => Progress::Ready(&leaving.offer),
                None => Progress::Failed(&leaving.offer),
            },
            Stage::Ready(_, offer, _) => Progress::Ready(offer),
            Stage::Failed(offer) => Progress::Failed(offer),
        }
    }

    pub fn offers(&self) -> impl Iterator<Item = &Offer> {
        self.adverts.offers()
    }

    pub fn accept(&mut self) {
        if let Stage::Failed(_) = self.stage {
            self.stage = Stage::Listening;
        }
        let Stage::Listening = self.stage else {
            return;
        };
        let Some(offer) = self.adverts.offers().next().cloned() else {
            return;
        };
        self.stage = Stage::Joining(Joining {
            offer,
            step: Step::Auth,
            attempts: 0,
            retry_at: self.now,
        });
    }

    pub fn ready(&self) -> bool {
        matches!(self.stage, Stage::Ready(_, _, boot_at) if self.now >= boot_at)
    }

    pub fn take_image(&mut self) -> Option<Image> {
        match std::mem::replace(&mut self.stage, Stage::Listening) {
            Stage::Ready(image, _, _) => Some(*image),
            other => {
                self.stage = other;
                None
            }
        }
    }

    pub fn busy(&self) -> bool {
        !self.queued.is_empty() || !self.outbox.is_empty()
    }

    pub fn log(&self) -> &[String] {
        &self.log
    }

    pub fn take_frames(&mut self) -> Vec<Frame> {
        std::mem::take(&mut self.outbox)
    }

    pub fn run_until(&mut self, us: u64) {
        self.now = self.now.max(us);
        let now = self.now;
        if let Stage::Leaving(leaving) = &self.stage
            && !leaving.queued
            && now >= leaving.deadline
        {
            self.leave(now);
        }
        let (due, later): (Vec<Frame>, Vec<Frame>) =
            self.queued.drain(..).partition(|frame| frame.start <= now);
        self.queued = later;
        for frame in due {
            self.sent(&frame);
            self.outbox.push(frame);
        }
        if let Stage::Listening = self.stage {
            self.adverts
                .forget_older_than(now.saturating_sub(OFFER_TIMEOUT_US));
        }
    }

    pub fn receive(&mut self, frame: Frame) {
        if !frame.fcs_ok() {
            return;
        }
        if let Some(beacon) = Beacon::parse(&frame) {
            self.adverts.hear(&beacon, frame.end());
            self.beacon_heard(&beacon, frame.end());
            return;
        }
        let addressed = frame.address(1) == self.mac;
        match frame.kind() {
            Kind::Management(AUTH) if addressed => self.auth_answered(&frame),
            Kind::Management(ASSOC_RESPONSE) if addressed => self.assoc_answered(&frame),
            Kind::Management(DEAUTH | DISASSOC) if addressed => self.dropped(&frame),
            Kind::Data(_) if frame.is_mp_cmd() => self.cmd_heard(&frame),
            Kind::Data(_) if frame.is_mp_ack() => self.ack_heard(&frame),
            _ => {}
        }
    }

    fn beacon_heard(&mut self, beacon: &Beacon, end: u64) {
        let Stage::Joining(joining) = &mut self.stage else {
            return;
        };
        if beacon.host != joining.offer.host || end < joining.retry_at {
            return;
        }
        if joining.attempts >= MAX_ATTEMPTS {
            self.log.push(format!("join gave up at {:?}", joining.step));
            self.stage = Stage::Listening;
            return;
        }
        joining.attempts += 1;
        joining.retry_at = end + RETRY_US;
        joining.offer.channel = beacon.channel;
        let step = joining.step;
        let offer = joining.offer.clone();
        self.send_step(step, &offer, end + SEND_GAP_US);
    }

    fn send_step(&mut self, step: Step, offer: &Offer, at: u64) {
        let (subtype, body) = match step {
            Step::Auth => (
                AUTH,
                [0u16, 1, 0].iter().flat_map(|v| v.to_le_bytes()).collect(),
            ),
            Step::Assoc => (ASSOC_REQUEST, assoc_body(offer)),
        };
        self.queue_management(subtype, offer, body, at);
    }

    fn queue_management(&mut self, subtype: u8, offer: &Offer, body: Vec<u8>, at: u64) {
        let mut bytes = Vec::with_capacity(24 + body.len() + FCS_BYTES);
        bytes.extend_from_slice(&(u16::from(subtype) << 4).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&offer.host);
        bytes.extend_from_slice(&self.mac);
        bytes.extend_from_slice(&offer.host);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&body);
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        self.queue(bytes, offer.channel, false, at);
    }

    fn queue(&mut self, mut bytes: Vec<u8>, channel: u8, short_preamble: bool, at: u64) {
        let number = self.sequence;
        self.sequence = (self.sequence + 1) & 0x0FFF;
        frame::set_sequence(&mut bytes, number);
        frame::seal(&mut bytes);
        self.queued.push(Frame {
            start: at,
            channel,
            rate: Rate::Mbit2,
            short_preamble,
            bytes,
        });
    }

    fn sent(&mut self, frame: &Frame) {
        if let (Stage::Leaving(..), Kind::Management(DEAUTH)) = (&self.stage, frame.kind()) {
            let Stage::Leaving(leaving) = std::mem::replace(&mut self.stage, Stage::Listening)
            else {
                unreachable!("matched above")
            };
            self.log.push(format!("left at {}", frame.start));
            let Leaving { offer, image, .. } = *leaving;
            self.stage = match image {
                Some(image) => Stage::Ready(Box::new(image), offer, frame.end() + BOOT_DELAY_US),
                None => Stage::Failed(offer),
            };
        }
    }

    fn ack_heard(&mut self, frame: &Frame) {
        let Stage::Leaving(leaving) = &self.stage else {
            return;
        };
        if frame.address(2) == leaving.offer.host && !leaving.queued {
            self.leave(frame.end() + SEND_GAP_US);
        }
    }

    fn leave(&mut self, at: u64) {
        let Stage::Leaving(leaving) = &mut self.stage else {
            return;
        };
        leaving.queued = true;
        let offer = leaving.offer.clone();
        self.queue_management(DEAUTH, &offer, LEAVING.to_le_bytes().to_vec(), at);
    }

    fn auth_answered(&mut self, frame: &Frame) {
        let Stage::Joining(joining) = &mut self.stage else {
            return;
        };
        let body = body(frame);
        if frame.address(2) != joining.offer.host
            || joining.step != Step::Auth
            || half(body, 2) != 2
            || half(body, 4) != 0
        {
            return;
        }
        joining.step = Step::Assoc;
        joining.retry_at = frame.end() + RETRY_US;
        let offer = joining.offer.clone();
        self.log.push(format!("auth ok at {}", frame.end()));
        self.send_step(Step::Assoc, &offer, frame.end() + SEND_GAP_US);
    }

    fn assoc_answered(&mut self, frame: &Frame) {
        let Stage::Joining(joining) = &self.stage else {
            return;
        };
        let body = body(frame);
        if frame.address(2) != joining.offer.host || half(body, 2) != 0 {
            return;
        }
        let aid = half(body, 4) & AID_MASK;
        if aid == 0 {
            return;
        }
        let offer = joining.offer.clone();
        let packet_bytes = usize::from(offer.cmd_bytes).saturating_sub(PACKET_OVERHEAD);
        self.log.push(format!("assoc aid {aid} at {}", frame.end()));
        self.stage = Stage::Receiving(Box::new(Receiving {
            offer,
            aid,
            transfer: Transfer::new(packet_bytes),
            snippet: 0,
            last_cmd: None,
        }));
    }

    fn dropped(&mut self, frame: &Frame) {
        let host = match &self.stage {
            Stage::Joining(joining) => joining.offer.host,
            Stage::Receiving(receiving) => receiving.offer.host,
            _ => return,
        };
        if frame.address(2) == host {
            self.log.push(format!("dropped by host at {}", frame.end()));
            self.stage = Stage::Listening;
        }
    }

    fn cmd_heard(&mut self, frame: &Frame) {
        let Stage::Receiving(receiving) = &mut self.stage else {
            return;
        };
        if frame.address(2) != receiving.offer.host {
            return;
        }
        let body = body(frame);
        let clients = half(body, CMD_CLIENTS) & CLIENT_BITS;
        let aid = receiving.aid;
        if clients & (1 << aid) == 0 {
            return;
        }
        let reply_time = u64::from(half(body, CMD_REPLY_TIME));
        let below = u64::from((clients & ((1 << aid) - 1)).count_ones());
        let at = frame.end() + FIRST_REPLY_GAP_US + below * (SLOT_GAP_US + reply_time);
        let sequence = frame.sequence();
        let payload = match &receiving.last_cmd {
            Some((last, payload)) if *last == sequence && frame.retry() => payload.clone(),
            _ => {
                let payload = answer(receiving, body, &self.name, self.color, &mut self.log);
                receiving.last_cmd = Some((sequence, payload.clone()));
                payload
            }
        };
        let channel = receiving.offer.channel;
        let host = receiving.offer.host;
        let finished = payload[2] == ReplyKind::Final as u8;
        let mut bytes = Vec::with_capacity(24 + payload.len() + FCS_BYTES);
        bytes.extend_from_slice(&REPLY_CONTROL.to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&host);
        bytes.extend_from_slice(&self.mac);
        bytes.extend_from_slice(&MP_REPLY);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        self.queue(bytes, channel, true, at);
        if finished {
            self.finish(at);
        }
    }

    fn finish(&mut self, reply_at: u64) {
        let Stage::Receiving(receiving) = std::mem::replace(&mut self.stage, Stage::Listening)
        else {
            return;
        };
        let offer = receiving.offer;
        let Some([header, arm9, arm7]) = receiving.transfer.parts() else {
            self.stage = Stage::Failed(offer);
            return;
        };
        let image = Image {
            header: header.to_vec(),
            arm9: arm9.to_vec(),
            arm7: arm7.to_vec(),
            parent: Parent::of(&offer),
        };
        let image = match image.check() {
            Ok(()) => Some(image),
            Err(fault) => {
                self.log.push(format!("image refused: {fault:?}"));
                None
            }
        };
        self.stage = Stage::Leaving(Box::new(Leaving {
            offer,
            image,
            queued: false,
            deadline: reply_at + LEAVE_TIMEOUT_US,
        }));
    }
}

fn answer(
    receiving: &mut Receiving,
    body: &[u8],
    name: &[u16],
    color: u8,
    log: &mut Vec<String>,
) -> Vec<u8> {
    let size = body.get(CMD_SIZE).copied().unwrap_or(0);
    let command = if size == 0 || body.len() <= CMD_COMMAND {
        Command::Dummy
    } else {
        Command::from_code(body[CMD_COMMAND])
    };
    let (kind, payload) = match command {
        Command::NameRequest => {
            let snippet = receiving.snippet;
            receiving.snippet = (snippet + 1) % SNIPPETS;
            (
                ReplyKind::Username,
                username(receiving.offer.game_id, name, color, snippet),
            )
        }
        Command::Rsa => {
            if size == RSA_SIZE
                && let Some(rsa) = body.get(CMD_DATA..)
                && receiving.transfer.take_rsa(rsa)
            {
                log.push(format!(
                    "rsa: {} packets of {} bytes",
                    receiving.transfer.total(),
                    usize::from(receiving.offer.cmd_bytes).saturating_sub(PACKET_OVERHEAD)
                ));
            }
            if receiving.transfer.has_rsa() {
                (ReplyKind::Rsa, [0; REPLY_PAYLOAD])
            } else {
                let snippet = receiving.snippet;
                receiving.snippet = (snippet + 1) % SNIPPETS;
                (
                    ReplyKind::Username,
                    username(receiving.offer.game_id, name, color, snippet),
                )
            }
        }
        Command::Data => {
            let length = (usize::from(size) * 2).saturating_sub(PACKET_FRAME_OVERHEAD);
            if let (Some(number), Some(data)) = (
                body.get(CMD_PACKET..CMD_PACKET + 2),
                body.get(CMD_PACKET_DATA..CMD_PACKET_DATA + length),
            ) {
                let number = u16::from_le_bytes([number[0], number[1]]);
                if !receiving.transfer.take_packet(number, data) {
                    log.push(format!("packet {number} of {length} bytes refused"));
                }
            }
            transfer_reply(&receiving.transfer)
        }
        Command::Final if receiving.transfer.complete() => (ReplyKind::Final, [0; REPLY_PAYLOAD]),
        Command::Final | Command::Dummy | Command::Error | Command::Other(_) => {
            if !receiving.transfer.has_rsa() {
                (ReplyKind::Dummy, [0; REPLY_PAYLOAD])
            } else if receiving.transfer.got() == 0 {
                (ReplyKind::Rsa, [0; REPLY_PAYLOAD])
            } else {
                transfer_reply(&receiving.transfer)
            }
        }
    };
    let mut reply = vec![REPLY_HALVES, REPLY_FLAGS, kind as u8];
    reply.extend_from_slice(&payload);
    reply
}

fn transfer_reply(transfer: &Transfer) -> (ReplyKind, [u8; REPLY_PAYLOAD]) {
    if transfer.complete() {
        return (ReplyKind::GotAll, [0; REPLY_PAYLOAD]);
    }
    let mut payload = [0; REPLY_PAYLOAD];
    payload[..2].copy_from_slice(&transfer.next_missing().to_le_bytes());
    payload[2..4].copy_from_slice(&(transfer.got() as u16).to_le_bytes());
    (ReplyKind::Data, payload)
}

fn username(game_id: u32, name: &[u16], color: u8, snippet: u8) -> [u8; REPLY_PAYLOAD] {
    let mut payload = [0; REPLY_PAYLOAD];
    payload[0] = snippet;
    let unit = |index: usize| -> [u8; 2] { name.get(index).copied().unwrap_or(0).to_le_bytes() };
    match snippet {
        0 => {
            payload[1..5].copy_from_slice(&game_id.to_le_bytes());
            payload[5] = color;
            payload[6] = name.len() as u8;
        }
        1..=3 => {
            let first = usize::from(snippet - 1) * 3;
            for (slot, index) in (first..first + 3).enumerate() {
                payload[1 + slot * 2..3 + slot * 2].copy_from_slice(&unit(index));
            }
        }
        _ => {
            payload[1..3].copy_from_slice(&unit(9));
            payload[3..5].copy_from_slice(&1u16.to_le_bytes());
        }
    }
    payload
}

fn assoc_body(offer: &Offer) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&CAPABILITY.to_le_bytes());
    body.extend_from_slice(&LISTEN_INTERVAL.to_le_bytes());
    body.extend_from_slice(&[0, SSID_BYTES as u8]);
    let mut ssid = [0; SSID_BYTES];
    ssid[..4].copy_from_slice(&offer.game_id.to_le_bytes());
    ssid[4..6].copy_from_slice(&offer.stream.to_le_bytes());
    body.extend_from_slice(&ssid);
    body.extend_from_slice(&RATES);
    body
}

fn body(frame: &Frame) -> &[u8] {
    let end = frame.bytes.len().saturating_sub(FCS_BYTES);
    frame.bytes.get(24..end).unwrap_or(&[])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::advert::tests::{BEACON_GAP_US, GGID, HOST, TGID, advert, beacon, round};
    use super::image::tests::image;
    use super::*;
    use crate::nds::wifi::frame::{MP_ACK, MP_CMD};

    pub(crate) const MAC: [u8; 6] = [0x00, 0x09, 0xBF, 0xAB, 0xCD, 0xEF];
    pub(crate) const CMD_BYTES: u16 = 0x0100;
    const PACKET: usize = CMD_BYTES as usize - PACKET_OVERHEAD;
    const REPLY_TIME: u16 = 0x0106;
    const STEP_US: u64 = 5_000;

    struct Rig {
        download: Download,
        now: u64,
        sequence: u16,
    }

    impl Rig {
        fn offered() -> Self {
            let mut download = Download::new(MAC, "Clara", 3);
            let mut now = 0;
            for frame in round(&advert("Mario Kart DS", "Host"), CMD_BYTES, 0) {
                now = frame.end();
                download.receive(frame);
                download.run_until(now);
            }
            Self {
                download,
                now,
                sequence: 0,
            }
        }

        fn joined(aid: u16) -> Self {
            let mut rig = Self::offered();
            rig.download.accept();
            let auth = rig.hear(beacon(
                &advert("Mario Kart DS", "Host"),
                0,
                0,
                CMD_BYTES,
                rig.now + BEACON_GAP_US,
            ));
            assert_eq!(auth.len(), 1);
            let assoc = rig.hear(auth_answer(MAC, rig.now + 300));
            assert_eq!(assoc.len(), 1);
            let none = rig.hear(assoc_answer(MAC, aid, rig.now + 300));
            assert!(none.is_empty());
            rig
        }

        fn hear(&mut self, frame: Frame) -> Vec<Frame> {
            self.now = frame.end();
            self.download.receive(frame);
            self.now += STEP_US;
            self.download.run_until(self.now);
            self.download.take_frames()
        }

        fn cmd(&mut self, clients: u16, command: u8, data: &[u8]) -> Frame {
            let frame = cmd(self.sequence, clients, command, data, self.now + 1_000);
            self.sequence = (self.sequence + 2) & 0x0FFF;
            frame
        }

        fn ask(&mut self, command: u8, data: &[u8]) -> [u8; 10] {
            let cmd = self.cmd(1 << AID, command, data);
            let replies = self.hear(cmd.clone());
            let reply = replies
                .iter()
                .find(|frame| frame.control() == REPLY_CONTROL)
                .unwrap();
            assert_eq!(reply.start, cmd.end() + FIRST_REPLY_GAP_US);
            assert_eq!(
                (reply.address(1), reply.address(2), reply.address(3)),
                (HOST, MAC, MP_REPLY)
            );
            assert!(reply.short_preamble && reply.fcs_ok());
            body(reply).try_into().unwrap()
        }

        fn send(&mut self, payload: &[u8]) -> Vec<[u8; 10]> {
            let rsa = rsa(payload);
            let mut replies = vec![self.ask(3, &[rsa.as_slice(), &[0; 5]].concat())];
            for number in 0..packets(payload) {
                replies.push(self.ask(4, &packet(payload, number)));
            }
            replies
        }
    }

    const AID: u16 = 1;

    pub(crate) fn auth_answer(to: [u8; 6], start: u64) -> Frame {
        management(to, AUTH, &[0, 0, 2, 0, 0, 0], start)
    }

    pub(crate) fn assoc_answer(to: [u8; 6], aid: u16, start: u64) -> Frame {
        let id = (0xC000 | aid).to_le_bytes();
        let body = [0x21, 0, 0, 0, id[0], id[1], 1, 2, 0x82, 0x84];
        management(to, ASSOC_RESPONSE, &body, start)
    }

    fn management(to: [u8; 6], subtype: u8, body: &[u8], start: u64) -> Frame {
        let mut bytes = (u16::from(subtype) << 4).to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&to);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(body);
        sealed(bytes, false, start)
    }

    pub(crate) fn cmd(sequence: u16, clients: u16, command: u8, data: &[u8], start: u64) -> Frame {
        let mut bytes = 0x0228u16.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&MP_CMD);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&REPLY_TIME.to_le_bytes());
        bytes.extend_from_slice(&clients.to_le_bytes());
        bytes.push((1 + data.len()).div_ceil(2) as u8);
        bytes.push(0x11);
        bytes.push(command);
        bytes.extend_from_slice(data);
        bytes.extend_from_slice(&clients.to_le_bytes());
        frame::set_sequence(&mut bytes, sequence);
        sealed(bytes, true, start)
    }

    pub(crate) fn ack(start: u64) -> Frame {
        let mut bytes = 0x0218u16.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&MP_ACK);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&HOST);
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&[0x5D, 0, 0, 0]);
        sealed(bytes, true, start)
    }

    fn sealed(mut bytes: Vec<u8>, short_preamble: bool, start: u64) -> Frame {
        bytes.extend_from_slice(&[0; FCS_BYTES]);
        frame::seal(&mut bytes);
        Frame {
            start,
            channel: 7,
            rate: Rate::Mbit2,
            short_preamble,
            bytes,
        }
    }

    fn sections(payload: &[u8]) -> [&[u8]; 3] {
        let arm9 = u32::from_le_bytes(payload[0x2C..0x30].try_into().unwrap()) as usize;
        let (header, rest) = payload.split_at(0x160);
        let (arm9, arm7) = rest.split_at(arm9);
        [header, arm9, arm7]
    }

    pub(crate) fn rsa(payload: &[u8]) -> Vec<u8> {
        let mut rsa = vec![0; 0xE4];
        for (at, section) in [0x14, 0x24, 0x34].into_iter().zip(sections(payload)) {
            rsa[at..at + 4].copy_from_slice(&(section.len() as u32).to_le_bytes());
        }
        rsa
    }

    fn chunks(payload: &[u8]) -> Vec<&[u8]> {
        sections(payload)
            .into_iter()
            .flat_map(|section| section.chunks(PACKET))
            .collect()
    }

    pub(crate) fn packets(payload: &[u8]) -> u16 {
        chunks(payload).len() as u16
    }

    pub(crate) fn packet(payload: &[u8], number: u16) -> Vec<u8> {
        let mut data = vec![0, 0];
        data.extend_from_slice(&number.to_le_bytes());
        data.extend_from_slice(chunks(payload)[usize::from(number)]);
        data.push(0);
        data
    }

    pub(crate) fn payload_of(image: &Image) -> Vec<u8> {
        [image.header.as_slice(), &image.arm9, &image.arm7].concat()
    }

    #[test]
    fn an_offer_shows_the_game_and_host_before_joining() {
        let rig = Rig::offered();
        let Progress::Offered(offer) = rig.download.progress() else {
            panic!("no offer: {:?}", rig.download.progress());
        };
        assert_eq!(
            (offer.title.as_str(), offer.host_name.as_str()),
            ("Mario Kart DS", "Host")
        );
        assert_eq!((offer.game_id, offer.stream), (GGID, TGID));
    }

    #[test]
    fn a_whole_child_comes_down_and_the_receiver_leaves_the_host() {
        let child = image(0x253, 0x120);
        let payload = payload_of(&child);
        let mut rig = Rig::joined(AID);
        let name = rig.ask(1, &[0; 5]);
        assert_eq!(
            &name[..4],
            &[REPLY_HALVES, REPLY_FLAGS, ReplyKind::Username as u8, 0]
        );
        assert_eq!(&name[4..], &[0xA0, 0x00, 0x40, 0x00, 3, 5]);
        let replies = rig.send(&payload);
        assert_eq!(replies[0][2], ReplyKind::Rsa as u8);
        assert_eq!(packets(&payload), 2 + 3 + 2);
        for (got, reply) in replies[1..replies.len() - 1].iter().enumerate() {
            assert_eq!(reply[2], ReplyKind::Data as u8);
            assert_eq!(
                (half(reply, 3), half(reply, 5)),
                (got as u16 + 1, got as u16 + 1)
            );
        }
        assert_eq!(replies.last().unwrap()[2], ReplyKind::GotAll as u8);
        let Progress::Receiving { got, total, .. } = rig.download.progress() else {
            panic!("not receiving: {:?}", rig.download.progress());
        };
        assert_eq!((got, total), (payload.len(), payload.len()));
        let cmd = rig.cmd(1 << AID, 5, &[0; 5]);
        let frames = rig.hear(cmd);
        assert_eq!(frames.len(), 1);
        assert_eq!(body(&frames[0])[2], ReplyKind::Final as u8);
        assert!(!rig.download.ready());
        let ack = ack(rig.now + 1_000);
        let frames = rig.hear(ack.clone());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind(), Kind::Management(DEAUTH));
        assert_eq!(frames[0].start, ack.end() + SEND_GAP_US);
        assert_eq!((frames[0].address(1), frames[0].address(2)), (HOST, MAC));
        assert_eq!(body(&frames[0]), &LEAVING.to_le_bytes());
        rig.download.run_until(frames[0].end() + BOOT_DELAY_US);
        assert!(rig.download.ready());
        assert_eq!(rig.download.take_image(), Some(child));
    }

    #[test]
    fn the_child_boots_only_once_the_host_has_restarted() {
        let mut rig = Rig::joined(AID);
        rig.send(&payload_of(&image(0x100, 0x40)));
        let cmd = rig.cmd(1 << AID, 5, &[0; 5]);
        rig.hear(cmd);
        let deauth = rig.hear(ack(rig.now + 1_000)).remove(0);
        rig.download.run_until(deauth.end() + BOOT_DELAY_US - 1);
        assert!(!rig.download.ready());
        assert!(matches!(rig.download.progress(), Progress::Ready(_)));
        rig.download.run_until(deauth.end() + BOOT_DELAY_US);
        assert!(rig.download.ready());
    }

    #[test]
    fn the_receiver_leaves_by_itself_when_no_ack_comes() {
        let payload = payload_of(&image(0x100, 0x40));
        let mut rig = Rig::joined(AID);
        rig.send(&payload);
        let cmd = rig.cmd(1 << AID, 5, &[0; 5]);
        let replied = rig.hear(cmd).last().unwrap().start;
        rig.download.run_until(replied + LEAVE_TIMEOUT_US);
        let frames = rig.download.take_frames();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].kind(), Kind::Management(DEAUTH));
        rig.download.run_until(frames[0].end() + BOOT_DELAY_US);
        assert!(rig.download.ready());
    }

    #[test]
    fn a_failed_download_joins_the_host_again_when_a_is_pressed() {
        let mut child = image(0x100, 0x40);
        child.header[0] ^= 0xFF;
        let mut rig = Rig::joined(AID);
        rig.send(&payload_of(&child));
        let cmd = rig.cmd(1 << AID, 5, &[0; 5]);
        let replied = rig.hear(cmd).last().unwrap().start;
        rig.download.run_until(replied + LEAVE_TIMEOUT_US);
        let frames = rig.download.take_frames();
        assert_eq!(frames[0].kind(), Kind::Management(DEAUTH));
        rig.download.run_until(frames[0].end() + BOOT_DELAY_US);
        assert!(matches!(rig.download.progress(), Progress::Failed(_)));
        rig.download.accept();
        assert!(matches!(rig.download.progress(), Progress::Joining(_)));
    }

    #[test]
    fn a_lost_packet_is_asked_for_by_number() {
        let child = image(0x300, 0x40);
        let payload = payload_of(&child);
        let mut rig = Rig::joined(AID);
        rig.ask(3, &[rsa(&payload).as_slice(), &[0; 5]].concat());
        let total = packets(&payload);
        for number in (0..total).filter(|&number| number != 2) {
            let reply = rig.ask(4, &packet(&payload, number));
            assert_eq!(reply[2], ReplyKind::Data as u8);
        }
        let reply = rig.ask(4, &packet(&payload, total - 1));
        assert_eq!((half(&reply, 3), half(&reply, 5)), (2, total - 1));
        let early_final = rig.ask(5, &[0; 5]);
        assert_eq!(
            (early_final[2], half(&early_final, 3)),
            (ReplyKind::Data as u8, 2)
        );
        assert_eq!(rig.ask(4, &packet(&payload, 2))[2], ReplyKind::GotAll as u8);
    }

    #[test]
    fn the_receiver_keeps_accepting_the_file_info_until_data_comes() {
        let child = image(0x300, 0x40);
        let payload = payload_of(&child);
        let mut rig = Rig::joined(AID);
        assert_eq!(rig.ask(0, &[0; 5])[2], ReplyKind::Dummy as u8);
        rig.ask(3, &[rsa(&payload).as_slice(), &[0; 5]].concat());
        for _ in 0..3 {
            assert_eq!(rig.ask(0, &[0; 5])[2], ReplyKind::Rsa as u8);
        }
        rig.ask(4, &packet(&payload, 0));
        let reply = rig.ask(0, &[0; 5]);
        assert_eq!(
            (reply[2], half(&reply, 3), half(&reply, 5)),
            (ReplyKind::Data as u8, 1, 1)
        );
    }

    #[test]
    fn a_retried_cmd_gets_the_same_reply_again() {
        let mut rig = Rig::joined(AID);
        let first = rig.cmd(1 << AID, 1, &[0; 5]);
        let mut again = first.clone();
        frame::mark_retry(&mut again.bytes);
        frame::seal(&mut again.bytes);
        again.start = rig.now + 1_000;
        let one = rig.hear(first);
        let two = rig.hear(again);
        assert_eq!(body(&one[0])[3], 0);
        assert_eq!(body(&one[0]), body(&two[0]));
        assert_eq!(rig.ask(1, &[0; 5])[3], 1);
    }

    #[test]
    fn a_reply_waits_for_the_slot_after_lower_players() {
        let mut rig = Rig::joined(3);
        let cmd = rig.cmd(0b1110, 1, &[0; 5]);
        let end = cmd.end();
        let reply = rig.hear(cmd);
        assert_eq!(
            reply[0].start,
            end + FIRST_REPLY_GAP_US + 2 * (SLOT_GAP_US + u64::from(REPLY_TIME))
        );
        let other = rig.cmd(0b0110, 1, &[0; 5]);
        assert!(rig.hear(other).is_empty());
    }

    #[test]
    fn the_receiver_retries_a_join_the_host_did_not_answer() {
        let mut rig = Rig::offered();
        rig.download.accept();
        let advert = advert("Mario Kart DS", "Host");
        let first = rig.hear(beacon(&advert, 0, 0, CMD_BYTES, rig.now + BEACON_GAP_US));
        assert_eq!(first[0].kind(), Kind::Management(AUTH));
        let soon = rig.hear(beacon(&advert, 1, 0, CMD_BYTES, rig.now + BEACON_GAP_US));
        assert!(soon.is_empty());
        let later = rig.hear(beacon(&advert, 2, 0, CMD_BYTES, rig.now + RETRY_US));
        assert_eq!(later[0].kind(), Kind::Management(AUTH));
        assert!(matches!(rig.download.progress(), Progress::Joining(_)));
    }
}
