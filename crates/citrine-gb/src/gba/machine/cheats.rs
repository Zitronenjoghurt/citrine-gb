use super::Gba;
use crate::cheats::CheatTarget;
use crate::gba::region::Region;
use crate::gba::wait::Width;

fn width(bytes: u8) -> Width {
    match bytes {
        1 => Width::Byte,
        2 => Width::Half,
        _ => Width::Word,
    }
}

impl CheatTarget for Gba {
    fn cheat_read(&mut self, address: u32, bytes: u8) -> u32 {
        self.bus.read(address, width(bytes))
    }

    fn cheat_write(&mut self, address: u32, bytes: u8, value: u32) {
        self.bus.write(address, width(bytes), value);
    }

    fn cheat_rom(&self) -> &[u8] {
        self.bus.pristine_rom()
    }

    fn rom_offsets(&self, address: u32, out: &mut Vec<usize>) {
        if let region @ Region::Rom(_) = Region::of(address) {
            out.push(region.offset(address));
        }
    }

    fn set_rom_patches(&mut self, patches: &[(usize, u8)]) {
        if self.bus.set_rom_patches(patches) {
            self.idle.forget();
        }
    }
}

#[cfg(all(test, jit))]
mod tests {
    use super::*;

    const STORED: u32 = 0x0300_0000;
    const LITERAL: u32 = 0x1234_5678;
    const PATCHED: u32 = 0x1234_565A;

    fn rom(patched: bool) -> Vec<u8> {
        let mut rom = vec![0u8; 0x400];
        let words = [
            (0x000, 0xEA00_00BE),
            (0x300, 0xE28F_0001),
            (0x304, 0xE12F_FF10),
            (0x314, STORED),
            (0x318, LITERAL),
        ];
        for (at, word) in words {
            rom[at..at + 4].copy_from_slice(&u32::to_le_bytes(word));
        }
        let halves: [u16; 6] = [0x4A02, 0x4903, 0x6011, 0x3301, 0x6053, 0xE7FA];
        for (index, half) in halves.into_iter().enumerate() {
            let at = 0x308 + 2 * index;
            rom[at..at + 2].copy_from_slice(&half.to_le_bytes());
        }
        if patched {
            rom[0x318] = 0x5A;
        }
        rom
    }

    fn frames(gba: &mut Gba, count: usize) {
        for _ in 0..count {
            gba.run_frame();
        }
        assert_eq!(gba.fault(), None);
    }

    #[test]
    fn a_rom_patch_to_a_literal_pool_reaches_jit_blocks() {
        for jit in [false, true] {
            let mut gba = Gba::new(&rom(false));
            gba.set_jit(jit);
            frames(&mut gba, 3);
            assert_eq!(gba.bus().read32(STORED), LITERAL, "jit {jit}");
            let passes = gba.bus().read32(STORED + 4);
            assert!(passes > 0, "jit {jit}");
            gba.set_rom_patches(&[(0x318, 0x5A)]);
            frames(&mut gba, 2);
            assert_eq!(gba.bus().read32(STORED), PATCHED, "jit {jit}");
            assert_ne!(gba.bus().read32(STORED + 4), passes, "jit {jit}");
            if jit {
                let stats = gba.jit_stats().expect("the JIT is on");
                assert!(stats.compiles > 0, "{stats:?}");
            }
        }
    }

    #[cfg(not(feature = "jit-lockstep"))]
    #[test]
    fn a_cached_block_is_not_reused_for_another_literal() {
        use crate::jit_cache::{JitCache, KEY_BYTES};
        use std::sync::Arc;
        let (first, second) = (rom(false), rom(true));
        let cache = Arc::new(JitCache::for_rom(&first, [0; KEY_BYTES]));
        let run = |rom: &[u8]| {
            let mut gba = Gba::new(rom);
            gba.set_jit_cache(Some(Arc::clone(&cache)));
            gba.set_jit(true);
            frames(&mut gba, 3);
            gba.bus().read32(STORED)
        };
        assert_eq!(run(&first), LITERAL);
        let kept = cache.blocks();
        assert!(kept > 0);
        assert_eq!(run(&second), PATCHED);
        assert!(
            cache.blocks() > kept,
            "{} blocks after {kept}",
            cache.blocks()
        );
    }
}
