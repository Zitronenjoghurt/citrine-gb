pub(crate) const HANDSHAKE: u8 = 0x01;
pub(crate) const HANDSHAKE_REPLY: u8 = 0x02;
pub(crate) const PREAMBLE: u8 = 0xFD;
pub(crate) const NO_DATA: u8 = 0xFE;
pub(crate) const CANCEL: u8 = 0x0F;
pub(crate) const CONFIRM: u8 = 0x02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sync {
    Preamble,
    FirstByte,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Block {
    pub data: Vec<u8>,
    pub sync: Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speech {
    Gen1,
    Gen2,
}

impl Speech {
    fn room_nybble(self, sent: u8) -> u8 {
        match self {
            Speech::Gen1 => 0,
            Speech::Gen2 => sent,
        }
    }

    fn room_sync(self) -> u8 {
        match self {
            Speech::Gen1 => 0xD4,
            Speech::Gen2 => 0xD1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Waiting,
    Linked,
    Exchanging,
    Choosing,
    Confirming,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Room,
    Blocks {
        index: usize,
        pos: usize,
        ignoring: bool,
    },
    Select,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Happening {
    Linked,
    Exchanged,
    Chosen { slot: u8 },
    Canceled,
    Traded { slot: u8 },
}

#[derive(Debug, Clone)]
pub(crate) struct Machine {
    speech: Speech,
    state: State,
    episode: Option<u8>,
    chosen: u8,
    received: Vec<Vec<u8>>,
    happened: Vec<Happening>,
}

impl Machine {
    pub fn new(speech: Speech) -> Self {
        Self {
            speech,
            state: State::Idle,
            episode: None,
            chosen: 0,
            received: Vec::new(),
            happened: Vec::new(),
        }
    }

    pub fn stage(&self) -> Stage {
        match self.state {
            State::Idle => Stage::Waiting,
            State::Room => Stage::Linked,
            State::Blocks { .. } => Stage::Exchanging,
            State::Select => Stage::Choosing,
            State::Confirm => Stage::Confirming,
        }
    }

    pub fn received(&self) -> &[Vec<u8>] {
        &self.received
    }

    pub fn take_happenings(&mut self) -> Vec<Happening> {
        std::mem::take(&mut self.happened)
    }

    pub fn exchange(&mut self, sent: u8, blocks: &[Block]) -> u8 {
        if let State::Blocks {
            index,
            pos,
            ignoring,
        } = self.state
        {
            return self.block(sent, blocks, index, pos, ignoring);
        }
        if sent == HANDSHAKE {
            self.state = State::Room;
            self.episode = None;
            self.happened.push(Happening::Linked);
            return HANDSHAKE_REPLY;
        }
        if self.state == State::Idle {
            return 0x00;
        }
        if sent == PREAMBLE && self.state == State::Room {
            self.received = blocks
                .iter()
                .map(|b| Vec::with_capacity(b.data.len()))
                .collect();
            return self.block(sent, blocks, 0, 0, true);
        }
        match sent & 0xF0 {
            0xD0 => self.speech.room_sync(),
            0x60 | 0x70 => {
                let nybble = sent & 0x0F;
                self.episode = Some(nybble);
                (sent & 0xF0) | self.answer(nybble)
            }
            _ => {
                if sent == 0x00
                    && let Some(nybble) = self.episode.take()
                {
                    self.finish_episode(nybble);
                }
                0x00
            }
        }
    }

    fn answer(&self, nybble: u8) -> u8 {
        match self.state {
            State::Select if nybble == CANCEL => CANCEL,
            State::Select => 0,
            State::Confirm => nybble,
            _ => self.speech.room_nybble(nybble),
        }
    }

    fn finish_episode(&mut self, nybble: u8) {
        match self.state {
            State::Select if nybble == CANCEL => {
                self.state = State::Room;
                self.happened.push(Happening::Canceled);
            }
            State::Select => {
                self.chosen = nybble;
                self.state = State::Confirm;
                self.happened.push(Happening::Chosen { slot: nybble });
            }
            State::Confirm if nybble == CONFIRM => {
                self.state = State::Room;
                self.happened.push(Happening::Traded { slot: self.chosen });
            }
            State::Confirm => {
                self.state = State::Select;
                self.happened.push(Happening::Canceled);
            }
            _ => {}
        }
    }

    fn block(
        &mut self,
        sent: u8,
        blocks: &[Block],
        index: usize,
        pos: usize,
        ignoring: bool,
    ) -> u8 {
        let block = &blocks[index];
        let reply = block.data[pos];
        if ignoring {
            let synced = match block.sync {
                Sync::Preamble => sent == PREAMBLE,
                Sync::FirstByte => true,
            };
            self.state = State::Blocks {
                index,
                pos,
                ignoring: !synced,
            };
            return reply;
        }
        if let Some(received) = self.received.get_mut(index) {
            received.push(sent);
        }
        let pos = pos + 1;
        self.state = if pos < block.data.len() {
            State::Blocks {
                index,
                pos,
                ignoring: false,
            }
        } else if index + 1 < blocks.len() {
            State::Blocks {
                index: index + 1,
                pos: 0,
                ignoring: true,
            }
        } else {
            self.episode = None;
            self.happened.push(Happening::Exchanged);
            State::Select
        };
        reply
    }
}
