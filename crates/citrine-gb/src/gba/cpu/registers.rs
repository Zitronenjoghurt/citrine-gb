use super::psr::{Bank, MODE_MASK, Mode};
use crate::gba::wait::Access;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuState {
    pub r: [u32; 16],
    pub fiq: [u32; 7],
    pub svc: [u32; 2],
    pub abt: [u32; 2],
    pub irq: [u32; 2],
    pub und: [u32; 2],
    pub cpsr: u32,
    pub spsr: [u32; 5],
    pub pipeline: [u32; 2],
    pub next_fetch: Access,
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            r: [0; 16],
            fiq: [0; 7],
            svc: [0; 2],
            abt: [0; 2],
            irq: [0; 2],
            und: [0; 2],
            cpsr: 0xD3,
            spsr: [0; 5],
            pipeline: [0; 2],
            next_fetch: Access::NonSequential,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Registers {
    pub r: [u32; 16],
    user_high: [u32; 7],
    fiq_high: [u32; 7],
    banked: [[u32; 2]; 4],
    spsr: [u32; 5],
    cpsr: u32,
    bank: Bank,
}

impl Registers {
    pub fn from_state(state: &CpuState) -> Self {
        let mut user_high = [0; 7];
        user_high.copy_from_slice(&state.r[8..15]);
        let mut regs = Self {
            r: state.r,
            user_high,
            fiq_high: state.fiq,
            banked: [state.svc, state.abt, state.irq, state.und],
            spsr: state.spsr,
            cpsr: state.cpsr,
            bank: Bank::User,
        };
        regs.switch_bank(Bank::of(state.cpsr));
        regs
    }

    pub fn state(&self) -> CpuState {
        let mut regs = self.clone();
        regs.switch_bank(Bank::User);
        CpuState {
            r: regs.r,
            fiq: regs.fiq_high,
            svc: regs.banked[0],
            abt: regs.banked[1],
            irq: regs.banked[2],
            und: regs.banked[3],
            cpsr: self.cpsr,
            spsr: self.spsr,
            ..CpuState::default()
        }
    }

    pub fn cpsr(&self) -> u32 {
        self.cpsr
    }

    pub fn set_cpsr(&mut self, value: u32) {
        self.switch_bank(Bank::of(value));
        self.cpsr = value;
    }

    pub fn spsr(&self) -> Option<u32> {
        self.bank.spsr_index().map(|i| self.spsr[i])
    }

    pub fn set_spsr(&mut self, value: u32) {
        if let Some(i) = self.bank.spsr_index() {
            self.spsr[i] = value;
        }
    }

    pub fn bank(&self) -> Bank {
        self.bank
    }

    pub fn user_mode(&self) -> bool {
        self.cpsr & MODE_MASK == Mode::User.bits()
    }

    pub fn user_reg(&self, index: usize) -> u32 {
        match self.bank {
            Bank::User => self.r[index],
            Bank::Fiq => self.user_high[index - 8],
            _ if index >= 13 => self.user_high[index - 8],
            _ => self.r[index],
        }
    }

    pub fn switch_bank(&mut self, bank: Bank) {
        if bank == self.bank {
            return;
        }
        let low = if self.bank == Bank::Fiq {
            &mut self.fiq_high
        } else {
            &mut self.user_high
        };
        low[..5].copy_from_slice(&self.r[8..13]);
        self.store_sp_lr(self.bank);

        let low = if bank == Bank::Fiq {
            &self.fiq_high
        } else {
            &self.user_high
        };
        self.r[8..13].copy_from_slice(&low[..5]);
        let [sp, lr] = *self.sp_lr(bank);
        self.r[13] = sp;
        self.r[14] = lr;
        self.bank = bank;
    }

    fn store_sp_lr(&mut self, bank: Bank) {
        let pair = [self.r[13], self.r[14]];
        *self.sp_lr(bank) = pair;
    }

    fn sp_lr(&mut self, bank: Bank) -> &mut [u32; 2] {
        let slice: &mut [u32] = match bank {
            Bank::User => &mut self.user_high[5..],
            Bank::Fiq => &mut self.fiq_high[5..],
            Bank::Supervisor => &mut self.banked[0],
            Bank::Abort => &mut self.banked[1],
            Bank::Irq => &mut self.banked[2],
            Bank::Undefined => &mut self.banked[3],
        };
        slice
            .try_into()
            .expect("a bank holds two registers for r13 and r14")
    }
}
