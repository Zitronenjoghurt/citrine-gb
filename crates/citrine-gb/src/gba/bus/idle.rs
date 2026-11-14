use super::{Bus, EEPROM_REGION};
use crate::gba::cart::Cart;
use crate::gba::open_bus::OpenBus;
use crate::gba::prefetch::Prefetch;
use crate::gba::region::Region;
use crate::gba::wait::Width;

impl Bus {
    pub(crate) fn idle_span(&self) -> u32 {
        let quiet = if self.owed == 0 {
            self.quiet_span()
        } else {
            self.quiet.saturating_sub(self.owed)
        };
        quiet.min(self.dma_check.saturating_sub(self.owed))
    }

    pub(crate) fn clear_of_dma(&self) -> bool {
        self.step_cycles() < self.dma_check
    }

    pub(crate) fn plain(&self, addr: u32, width: Width) -> bool {
        let region = Region::of(addr);
        match region {
            Region::Ewram | Region::Iwram => true,
            Region::Rom(_) if addr >> 24 != EEPROM_REGION => {
                let offset = region.offset(addr) & !(width.bytes() - 1);
                offset + width.bytes() <= self.rom.len() && !Cart::in_gpio(offset)
            }
            _ => false,
        }
    }

    pub(crate) fn plain_value(&self, addr: u32, width: Width) -> Option<u32> {
        self.plain(addr, width).then(|| self.read(addr, width))
    }

    pub(crate) fn fetch_state(&self) -> (Prefetch, OpenBus) {
        (self.prefetch, self.open_bus)
    }

    pub(crate) fn restore_fetch_state(&mut self, (prefetch, open_bus): (Prefetch, OpenBus)) {
        self.prefetch = prefetch;
        self.open_bus = open_bus;
    }
}
