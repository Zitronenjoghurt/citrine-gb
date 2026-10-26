use crate::gb::ic::{ICInterface, Interrupt};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Serial {
    pub sb: u8,
    sc: u8,
    bits: u8,
    clock_high: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    sent: Option<u8>,
}

impl Default for Serial {
    fn default() -> Self {
        Self {
            sb: 0x00,
            sc: 0x02,
            bits: 0,
            clock_high: false,
            sent: None,
        }
    }
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_sc(&self, cgb_mode: bool) -> u8 {
        if cgb_mode {
            self.sc | 0x7C
        } else {
            self.sc | 0x7E
        }
    }

    pub fn write_sc(&mut self, value: u8, cgb_mode: bool, ic: &mut impl ICInterface) {
        self.bits = 0;
        if self.clock_high {
            self.edge(ic);
        }
        let value = if cgb_mode { value } else { value | 0x02 };
        self.sc = value & 0x83;
        if self.sc & 0x81 == 0x81 {
            self.sent = Some(self.sb);
        }
    }

    pub fn take_sent(&mut self) -> Option<u8> {
        self.sent.take()
    }

    pub fn transfer_active(&self) -> bool {
        self.sc & 0x80 != 0
    }

    pub fn div_changed(&mut self, old: u16, new: u16, cgb_mode: bool, ic: &mut impl ICInterface) {
        let mask = if cgb_mode && self.sc & 0x02 != 0 {
            0x04
        } else {
            0x80
        };
        if old & !new & mask != 0 {
            self.edge(ic);
        }
    }

    fn edge(&mut self, ic: &mut impl ICInterface) {
        self.clock_high = !self.clock_high;
        if self.clock_high || self.sc & 0x81 != 0x81 {
            return;
        }

        self.bits += 1;
        if self.bits == 8 {
            self.bits = 0;
            self.sc &= !0x80;
            ic.request_interrupt(Interrupt::Serial);
        }
        self.sb = (self.sb << 1) | 1;
    }
}
