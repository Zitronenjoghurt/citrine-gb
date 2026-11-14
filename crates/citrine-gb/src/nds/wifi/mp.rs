use super::frame::{self, Frame, HEADER_BYTES, MP_ACK, MP_REPLY, Rate};
use super::link::{Exchange, Link, PendingReply, Purpose, Receiving, Slot};
use super::registers::*;
use super::{IRQ_MP_END, IRQ_TX_DONE, Wifi};

pub const FIRST_REPLY_GAP_US: u64 = 16;
pub const SLOT_GAP_US: u64 = 10;
pub const AIR_ROUNDING_US: u64 = 4;
pub const ACK_BYTES: usize = 32;
pub const EMPTY_REPLY_BYTES: usize = 28;
pub const REPLY_TIME: usize = 0;
pub const CLIENT_MASK: usize = 2;
const ACK_CONTROL: u16 = 0x0218;
const EMPTY_REPLY_CONTROL: u16 = 0x0158;
const CLIENTS: u16 = 0xFFFE;
const ENABLE: u16 = 1 << 15;
const KEEP_SEQUENCE: u16 = 1 << 13;
const STATUS_OK: u16 = 0x0001;
const STATUS_FAILED: u16 = 0x0005;
const ACK_ADDRESS: u16 = 0x0FC0;
const SHORT_PREAMBLE: u16 = 1 << 2;

impl Wifi {
    pub(super) fn start_exchange(&mut self) {
        let value = self.reg(W_TXBUF_CMD);
        let header = usize::from(value & 0x0FFF) * 2;
        let mut frame = self.frame_from_ram(header, value & KEEP_SEQUENCE == 0);
        let clients = frame.body_half(CLIENT_MASK) & CLIENTS;
        let exchange = Exchange {
            header,
            clients,
            pending: clients,
            reply_time: u64::from(self.reg(W_CMD_REPLYTIME)),
            cmd_end: 0,
            window_end: 0,
            rounds: 1,
        };
        let needed = self.round_us(&frame, &exchange);
        self.exchange = Some(exchange);
        if u64::from(self.cmd_count) < needed {
            self.link = Link::Failing;
            return;
        }
        self.seal(&mut frame.bytes);
        self.transmit(Slot::Cmd, Some(header), frame);
    }

    pub(super) fn cmd_sent(&mut self) {
        if self.reg(W_TXSTATCNT) & (1 << 14) != 0 {
            self.set(W_TXSTAT, 0x0800);
            self.raise(IRQ_TX_DONE);
        }
        let now = self.now;
        if let Some(exchange) = self.exchange.as_mut() {
            exchange.cmd_end = now;
            exchange.window_end = now
                + FIRST_REPLY_GAP_US
                + u64::from(exchange.clients.count_ones()) * exchange.slot_us();
        }
        self.set_rf_status(5);
        self.link = Link::Listening;
        self.frame_done();
    }

    pub(super) fn reply_arrived(&mut self, frame: Frame) {
        let Some(exchange) = self.exchange.as_ref() else {
            return;
        };
        if !frame.is_mp_reply() || !frame.fcs_ok() {
            return;
        }
        let first = exchange.slot_start(0);
        let start = frame.start + AIR_ROUNDING_US;
        if start < first {
            return;
        }
        let slot = ((start - first) / exchange.slot_us()) as u32;
        let Some(client) = nth_client(exchange.clients, slot) else {
            return;
        };
        let slot_end = exchange.slot_start(slot) + exchange.slot_us();
        if frame.end() > slot_end || exchange.pending & (1 << client) == 0 {
            return;
        }
        let flags = self.filter(&frame).unwrap_or(0);
        self.link = Link::Receiving(Receiving {
            frame,
            flags,
            purpose: Purpose::HostReply(client),
            started: false,
        });
    }

    pub(super) fn window_closed(&mut self) {
        let Some(mut exchange) = self.exchange.take() else {
            return;
        };
        if exchange.pending != 0 {
            let mut frame = self.frame_from_ram(exchange.header, false);
            frame::mark_retry(&mut frame.bytes);
            frame::set_half(
                &mut frame.bytes,
                HEADER_BYTES + CLIENT_MASK,
                exchange.pending,
            );
            exchange.clients = exchange.pending;
            if u64::from(self.cmd_count) >= self.round_us(&frame, &exchange) {
                self.seal(&mut frame.bytes);
                exchange.rounds += 1;
                let header = exchange.header;
                self.exchange = Some(exchange);
                self.transmit(Slot::Cmd, Some(header), frame);
                return;
            }
        }
        self.exchange = Some(exchange);
        self.send_ack();
    }

    pub(super) fn ack_sent(&mut self) {
        let Some(exchange) = self.exchange.take() else {
            return;
        };
        let header = exchange.header;
        let failed = exchange.pending;
        self.set_ram16(
            header,
            if failed == 0 {
                STATUS_OK
            } else {
                STATUS_FAILED
            },
        );
        self.set_ram16(header + 2, failed);
        for client in 1..16 {
            if failed & (1 << client) != 0 {
                let offset = W_CMD_STAT + (client & !1);
                let shift = 8 * (client & 1);
                let value = self.reg(offset);
                let count = ((value >> shift) as u8).wrapping_add(1);
                self.set(
                    offset,
                    (value & !(0xFF << shift)) | (u16::from(count) << shift),
                );
            }
        }
        if self.reg(W_TXSTATCNT) & (1 << 13) != 0 {
            self.set(W_TXSTAT, 0x0B01);
            self.raise(IRQ_TX_DONE);
        }
        self.finish_exchange();
        self.frame_done();
        self.try_send();
    }

    pub(super) fn exchange_failed(&mut self) {
        let Some(exchange) = self.exchange.take() else {
            return;
        };
        self.set_ram16(exchange.header, STATUS_FAILED);
        self.set_ram16(exchange.header + 2, exchange.pending);
        self.finish_exchange();
        self.try_send();
    }

    pub(super) fn cmd_received(&mut self, frame: &Frame, duplicate: bool) {
        let aid = self.reg(W_AID_LOW) & 0xF;
        let mask = frame.body_half(CLIENT_MASK);
        if aid == 0 || mask & (1 << aid) == 0 {
            return;
        }
        if !duplicate {
            let current = self.reg(W_TXBUF_REPLY2);
            if current & ENABLE != 0 {
                let header = usize::from(current & 0x0FFF) * 2;
                let done = u16::from(self.ram8(header)) << 8 | STATUS_OK;
                self.set_ram16(header, done);
            }
            let next = self.reg(W_TXBUF_REPLY1);
            self.set(W_TXBUF_REPLY2, next);
            self.set(W_TXBUF_REPLY1, 0);
        }
        let reply_time = u64::from(frame.body_half(REPLY_TIME));
        let below = u64::from((mask & CLIENTS & ((1 << aid) - 1)).count_ones());
        let at = self.now + FIRST_REPLY_GAP_US + below * (SLOT_GAP_US + reply_time);
        let reply = self.reply_frame(reply_time);
        let next = self.reg(W_TXBUF_REPLY2);
        if !duplicate && next & ENABLE != 0 {
            let header = usize::from(next & 0x0FFF) * 2;
            let count = self.ram8(header + 4).saturating_add(1);
            self.set_ram8(header + 4, count);
            self.set_ram8(header + 5, 0);
        }
        self.link = Link::Waiting(at, reply);
    }

    pub(super) fn send_reply(&mut self, reply: PendingReply) {
        let slot = if reply.header.is_some() {
            Slot::Reply
        } else {
            Slot::EmptyReply
        };
        self.transmit(slot, reply.header, reply.frame);
    }

    fn reply_frame(&mut self, reply_time: u64) -> PendingReply {
        let current = self.reg(W_TXBUF_REPLY2);
        let short = self.reg(W_PREAMBLE) & SHORT_PREAMBLE != 0;
        if current & ENABLE != 0 {
            let header = usize::from(current & 0x0FFF) * 2;
            let length = u64::from(self.ram16(header + 0xA) & 0x3FFF);
            let rate = Rate::from_code(self.ram8(header + 8));
            if rate.preamble_us(short) + length * rate.byte_us() <= reply_time {
                let mut frame = self.frame_from_ram(header, true);
                self.seal(&mut frame.bytes);
                return PendingReply {
                    frame,
                    header: Some(header),
                };
            }
        }
        let mut bytes = vec![0; EMPTY_REPLY_BYTES];
        frame::set_half(&mut bytes, 0, EMPTY_REPLY_CONTROL);
        bytes[4..10].copy_from_slice(&self.address(W_BSSID));
        bytes[10..16].copy_from_slice(&self.mac());
        bytes[16..22].copy_from_slice(&MP_REPLY);
        let number = self.next_sequence();
        frame::set_sequence(&mut bytes, number);
        self.seal(&mut bytes);
        PendingReply {
            frame: Frame {
                start: self.now,
                channel: self.channel,
                rate: Rate::Mbit2,
                short_preamble: short,
                bytes,
            },
            header: None,
        }
    }

    fn send_ack(&mut self) {
        let Some(exchange) = self.exchange.as_ref() else {
            return;
        };
        let rate = Rate::from_code(self.ram8(exchange.header + 8));
        let failed = exchange.pending;
        let mut bytes = vec![0; ACK_BYTES];
        frame::set_half(&mut bytes, 0, ACK_CONTROL);
        bytes[4..10].copy_from_slice(&MP_ACK);
        bytes[10..16].copy_from_slice(&self.address(W_BSSID));
        bytes[16..22].copy_from_slice(&self.mac());
        let number = self.next_sequence();
        frame::set_sequence(&mut bytes, number);
        frame::set_half(&mut bytes, HEADER_BYTES, self.cmd_count.div_ceil(10) as u16);
        frame::set_half(&mut bytes, HEADER_BYTES + 2, failed);
        self.seal(&mut bytes);
        let frame = Frame {
            start: self.now,
            channel: self.channel,
            rate,
            short_preamble: self.reg(W_PREAMBLE) & SHORT_PREAMBLE != 0,
            bytes,
        };
        self.set(W_RXTX_ADDR, ACK_ADDRESS);
        self.transmit(Slot::Ack, None, frame);
    }

    fn round_us(&self, frame: &Frame, exchange: &Exchange) -> u64 {
        let ack =
            frame.rate.preamble_us(frame.short_preamble) + ACK_BYTES as u64 * frame.rate.byte_us();
        frame.airtime_us()
            + FIRST_REPLY_GAP_US
            + u64::from(exchange.clients.count_ones()) * exchange.slot_us()
            + ack
    }

    fn finish_exchange(&mut self) {
        let busy = self.reg(W_TXBUSY) & !Slot::Cmd.busy_bit();
        self.set(W_TXBUSY, busy);
        let cmd = self.reg(W_TXBUF_CMD) & !ENABLE;
        self.set(W_TXBUF_CMD, cmd);
        self.set_rf_status(1);
        self.raise(IRQ_MP_END);
        self.link = Link::Idle;
    }
}

fn nth_client(mask: u16, slot: u32) -> Option<u8> {
    (1..16u8)
        .filter(|&client| mask & (1 << client) != 0)
        .nth(slot as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_follow_the_set_bits_of_the_client_mask() {
        assert_eq!(nth_client(0b1010_0110, 0), Some(1));
        assert_eq!(nth_client(0b1010_0110, 1), Some(2));
        assert_eq!(nth_client(0b1010_0110, 2), Some(5));
        assert_eq!(nth_client(0b1010_0110, 3), Some(7));
        assert_eq!(nth_client(0b1010_0110, 4), None);
    }
}
