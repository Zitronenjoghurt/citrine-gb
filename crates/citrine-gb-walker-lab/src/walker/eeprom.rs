pub const EEPROM_SIZE: usize = 0x10000;
const PAGE: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Address { write: bool, bytes: u8 },
    Read,
    Write,
    Status,
    Done,
}

#[derive(Clone)]
pub struct Eeprom {
    pub data: Vec<u8>,
    state: State,
    addr: u16,
    write_enabled: bool,
    pending: Vec<(u16, u8)>,
    pub writes: u64,
    pub reads: Vec<(u16, u16)>,
}

impl Eeprom {
    pub fn new(data: Vec<u8>) -> Self {
        assert_eq!(data.len(), EEPROM_SIZE);
        Self {
            data,
            state: State::Idle,
            addr: 0,
            write_enabled: false,
            pending: Vec::new(),
            writes: 0,
            reads: Vec::new(),
        }
    }

    pub fn deselect(&mut self) {
        if self.state == State::Write && !self.pending.is_empty() {
            for (a, v) in self.pending.drain(..) {
                self.data[a as usize] = v;
            }
            self.writes += 1;
            self.write_enabled = false;
        }
        self.pending.clear();
        if let State::Read = self.state
            && let Some(last) = self.reads.last_mut()
        {
            last.1 = self.addr.wrapping_sub(last.0);
        }
        self.state = State::Idle;
    }

    pub fn transfer(&mut self, mosi: u8) -> u8 {
        match self.state {
            State::Idle => {
                self.state = match mosi {
                    0x06 => {
                        self.write_enabled = true;
                        State::Done
                    }
                    0x04 => {
                        self.write_enabled = false;
                        State::Done
                    }
                    0x05 => State::Status,
                    0x03 => State::Address {
                        write: false,
                        bytes: 0,
                    },
                    0x02 if self.write_enabled => State::Address {
                        write: true,
                        bytes: 0,
                    },
                    _ => State::Done,
                };
                0xFF
            }
            State::Address { write, bytes } => {
                self.addr = if bytes == 0 {
                    (mosi as u16) << 8
                } else {
                    self.addr | mosi as u16
                };
                self.state = if bytes == 0 {
                    State::Address { write, bytes: 1 }
                } else if write {
                    State::Write
                } else {
                    if self.reads.len() < 100_000 {
                        self.reads.push((self.addr, 0));
                    }
                    State::Read
                };
                0xFF
            }
            State::Read => {
                let v = self.data[self.addr as usize];
                self.addr = self.addr.wrapping_add(1);
                v
            }
            State::Write => {
                self.pending.push((self.addr, mosi));
                let page = self.addr as usize & !(PAGE - 1);
                self.addr = (page | ((self.addr as usize + 1) & (PAGE - 1))) as u16;
                0xFF
            }
            State::Status => (self.write_enabled as u8) << 1,
            State::Done => 0xFF,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(e: &mut Eeprom, bytes: &[u8]) -> Vec<u8> {
        let out = bytes.iter().map(|&b| e.transfer(b)).collect();
        e.deselect();
        out
    }

    #[test]
    fn reads_stream_from_the_address() {
        let mut data = vec![0; EEPROM_SIZE];
        data[0x1234..0x1237].copy_from_slice(&[1, 2, 3]);
        let mut e = Eeprom::new(data);
        assert_eq!(
            command(&mut e, &[0x03, 0x12, 0x34, 0, 0, 0])[3..],
            [1, 2, 3]
        );
    }

    #[test]
    fn writes_need_wren_and_land_on_deselect() {
        let mut e = Eeprom::new(vec![0xFF; EEPROM_SIZE]);
        command(&mut e, &[0x02, 0x00, 0x10, 0xAA]);
        assert_eq!(e.data[0x10], 0xFF);
        command(&mut e, &[0x06]);
        assert_eq!(command(&mut e, &[0x05, 0])[1], 0x02);
        command(&mut e, &[0x02, 0x00, 0x7F, 0xAA, 0xBB]);
        assert_eq!(e.data[0x7F], 0xAA);
        assert_eq!(e.data[0x00], 0xBB);
        assert_eq!(command(&mut e, &[0x05, 0])[1], 0x00);
    }
}
