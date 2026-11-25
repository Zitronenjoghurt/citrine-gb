use crate::gba::wait::Width;
#[cfg(jit)]
use crate::jit::{CodeMarks, PAGE_BITS, Window};
use crate::paged::{PAGE_SHIFT, Paged};

pub const PAGE: usize = 0x1000;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Ram {
    bytes: Paged,
    #[cfg(jit)]
    #[cfg_attr(feature = "serde", serde(skip))]
    code: CodeMarks,
}

impl Clone for Ram {
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes.clone(),
            #[cfg(jit)]
            code: self.code.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.bytes.clone_from(&source.bytes);
        #[cfg(jit)]
        self.code.clone_from(&source.code);
    }
}

impl Ram {
    pub fn new(len: usize) -> Self {
        debug_assert!(len.is_power_of_two());
        Self {
            bytes: Paged::zeroed(len),
            #[cfg(jit)]
            code: CodeMarks::default(),
        }
    }

    pub fn empty() -> Self {
        Self {
            bytes: Paged::zeroed(0),
            #[cfg(jit)]
            code: CodeMarks::default(),
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..]
    }

    pub fn read(&self, offset: usize, width: Width) -> u32 {
        read_at(&self.bytes, self.aligned(offset, width), width)
    }

    #[inline(always)]
    pub fn read_sized<const LEN: usize>(&self, offset: usize, width: Width) -> u32 {
        match self.get_sized::<LEN>(offset, width) {
            Some(value) => value,
            None => self.read_other(offset, width),
        }
    }

    #[inline(always)]
    pub fn get_sized<const LEN: usize>(&self, offset: usize, width: Width) -> Option<u32> {
        let bytes = <&[u8; LEN]>::try_from(&self.bytes[..]).ok()?;
        Some(read_at(
            bytes,
            offset & (LEN - 1) & !(width.bytes() - 1),
            width,
        ))
    }

    #[cold]
    #[inline(never)]
    fn read_other(&self, offset: usize, width: Width) -> u32 {
        self.read(offset, width)
    }

    pub fn inside(&self, offset: usize, len: usize) -> Option<usize> {
        let size = self.bytes.len();
        let start = offset & size.wrapping_sub(1);
        (size.is_power_of_two() && start + len <= size).then_some(start)
    }

    #[inline(always)]
    pub fn read_inside(&self, offset: usize, width: Width) -> u32 {
        read_at(&self.bytes, offset & !(width.bytes() - 1), width)
    }

    #[inline(always)]
    pub fn write(&mut self, offset: usize, width: Width, value: u32) {
        let start = self.aligned(offset, width);
        let bytes = self.bytes.touch(start);
        match width {
            Width::Byte => bytes[start] = value as u8,
            Width::Half => bytes.as_chunks_mut::<2>().0[start >> 1] = (value as u16).to_le_bytes(),
            Width::Word => bytes.as_chunks_mut::<4>().0[start >> 2] = value.to_le_bytes(),
        }
        #[cfg(jit)]
        self.code.written(start);
    }

    pub fn read_words(&self, offset: usize, values: &mut [u32]) -> bool {
        let Some(words) = self.words(offset, values.len()) else {
            return false;
        };
        for (value, word) in values.iter_mut().zip(words) {
            *value = u32::from_le_bytes(*word);
        }
        true
    }

    pub fn write_words(&mut self, offset: usize, values: &[u32]) -> bool {
        let Some(words) = self.words_mut(offset, values.len()) else {
            return false;
        };
        for (word, value) in words.iter_mut().zip(values) {
            *word = value.to_le_bytes();
        }
        true
    }

    #[inline(always)]
    pub fn words(&self, offset: usize, count: usize) -> Option<&[[u8; 4]]> {
        let start = self.run(offset, count)?;
        Some(&self.bytes.as_chunks::<4>().0[start >> 2..][..count])
    }

    #[inline(always)]
    pub fn words_mut(&mut self, offset: usize, count: usize) -> Option<&mut [[u8; 4]]> {
        let start = self.run(offset, count)?;
        let Some(last) = count.checked_sub(1).map(|index| start + index * 4) else {
            return Some(&mut []);
        };
        for page in (start >> PAGE_SHIFT) + 1..(last >> PAGE_SHIFT) + 1 {
            self.bytes.touch(page << PAGE_SHIFT);
        }
        #[cfg(jit)]
        if (start ^ last) >> PAGE_BITS == 0 {
            self.code.written(start);
        } else {
            for page in start >> PAGE_BITS..(last >> PAGE_BITS) + 1 {
                self.code.written(page << PAGE_BITS);
            }
        }
        Some(&mut self.bytes.touch(start).as_chunks_mut::<4>().0[start >> 2..][..count])
    }

    fn run(&self, offset: usize, words: usize) -> Option<usize> {
        let start = self.aligned(offset, Width::Word);
        (start + words * 4 <= self.bytes.len()).then_some(start)
    }

    #[cfg(jit)]
    pub fn window(&mut self, base: u32, size: u64, hardware: usize) -> Window {
        let (bytes, len, dirty) = self.bytes.raw();
        if len == 0 || len != hardware {
            return Window::CLOSED;
        }
        let code = self.code.pages();
        Window {
            bytes,
            dirty,
            code,
            size,
            base,
        }
    }

    #[cfg(jit)]
    pub fn code(&self) -> &CodeMarks {
        &self.code
    }

    #[cfg(jit)]
    pub fn code_mut(&mut self) -> &mut CodeMarks {
        &mut self.code
    }

    pub fn snapshot_from(&mut self, live: &mut Ram) {
        self.bytes.snapshot_from(&mut live.bytes);
        #[cfg(jit)]
        self.code.clone_from(&live.code);
    }

    pub fn copy_page(&mut self, from: &Ram, page: usize) {
        let span = page * PAGE..(page + 1) * PAGE;
        self.bytes.touch(span.start)[span.clone()].copy_from_slice(&from.bytes()[span]);
    }

    fn aligned(&self, offset: usize, width: Width) -> usize {
        offset & (self.bytes.len() - 1) & !(width.bytes() - 1)
    }
}

#[inline(always)]
fn read_at(bytes: &[u8], start: usize, width: Width) -> u32 {
    let word = u32::from_le_bytes(bytes.as_chunks::<4>().0[start >> 2]);
    let value = word >> ((start & 3) * 8);
    match width {
        Width::Byte => value & 0xFF,
        Width::Half => value & 0xFFFF,
        Width::Word => value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_reads_back_little_endian_in_every_width() {
        let mut ram = Ram::new(0x100);
        ram.write(0x10, Width::Word, 0x1122_3344);
        assert_eq!(ram.read(0x10, Width::Byte), 0x44);
        assert_eq!(ram.read(0x13, Width::Byte), 0x11);
        assert_eq!(ram.read(0x12, Width::Half), 0x1122);
        assert_eq!(ram.read(0x10, Width::Word), 0x1122_3344);
    }

    #[test]
    fn reads_inside_the_ram_match_plain_reads() {
        let mut ram = Ram::new(0x100);
        ram.write(0x40, Width::Word, 0x1122_3344);
        for offset in 0x40..0x44 {
            for width in [Width::Byte, Width::Half, Width::Word] {
                assert_eq!(ram.read_inside(offset, width), ram.read(offset, width));
            }
        }
    }

    #[test]
    fn inside_wraps_a_span_that_fits_and_refuses_one_that_does_not() {
        let ram = Ram::new(0x2000);
        assert_eq!(ram.inside(0x0200_1000, 0x1000), Some(0x1000));
        assert_eq!(ram.inside(0x0200_1800, 0x1000), None);
        assert_eq!(Ram::empty().inside(0, 0x1000), None);
    }

    #[cfg(jit)]
    #[test]
    fn a_window_stays_closed_unless_the_ram_has_its_hardware_size() {
        let mut short = Ram::new(0x1000);
        assert!(
            short
                .window(0x0200_0000, 0x0100_0000, 0x40_0000)
                .bytes
                .is_null()
        );
        let mut fitting = Ram::new(0x40_0000);
        let window = fitting.window(0x0200_0000, 0x0100_0000, 0x40_0000);
        assert_eq!(window.bytes.cast_const(), fitting.bytes().as_ptr());
        assert!(Ram::empty().window(0, 0x1000, 0x1000).bytes.is_null());
    }

    #[test]
    fn unaligned_accesses_round_down_to_their_width() {
        let mut ram = Ram::new(0x100);
        ram.write(0x23, Width::Word, 0xAABB_CCDD);
        assert_eq!(ram.read(0x20, Width::Word), 0xAABB_CCDD);
        assert_eq!(ram.read(0x21, Width::Half), 0xCCDD);
    }

    #[test]
    fn byte_and_half_writes_leave_their_neighbours() {
        let mut ram = Ram::new(0x100);
        ram.write(0x30, Width::Word, 0x1122_3344);
        ram.write(0x31, Width::Byte, 0xAA);
        assert_eq!(ram.read(0x30, Width::Word), 0x1122_AA44);
        ram.write(0x32, Width::Half, 0xBBCC);
        assert_eq!(ram.read(0x30, Width::Word), 0xBBCC_AA44);
    }

    #[test]
    fn a_word_run_reads_and_writes_what_single_words_do() {
        let mut ram = Ram::new(0x100);
        assert!(ram.write_words(0x123, &[1, 2, 3]));
        assert_eq!(ram.read(0x20, Width::Word), 1);
        assert_eq!(ram.read(0x28, Width::Word), 3);
        let mut values = [0; 3];
        assert!(ram.read_words(0x120, &mut values));
        assert_eq!(values, [1, 2, 3]);
    }

    #[test]
    fn a_word_run_across_a_page_reaches_the_next_snapshot_on_both_pages() {
        let mut live = Ram::new(2 * PAGE);
        Ram::empty().snapshot_from(&mut live);
        assert!(live.write_words(PAGE - 8, &[1, 2, 3, 4]));
        let mut snapshot = Ram::empty();
        snapshot.snapshot_from(&mut live);
        let mut back = Ram::new(2 * PAGE);
        back.clone_from(&snapshot);
        let words: Vec<u32> = (0..4)
            .map(|index| back.read(PAGE - 8 + 4 * index, Width::Word))
            .collect();
        assert_eq!(words, [1, 2, 3, 4]);
    }

    #[cfg(jit)]
    #[test]
    fn a_word_run_across_code_pages_drops_both_marks_in_order() {
        let mut ram = Ram::new(PAGE);
        ram.code_mut().mark(0, PAGE);
        ram.code_mut().mark(1, PAGE);
        let stamp = ram.code().stamp();
        assert!(ram.write_words((1 << PAGE_BITS) - 4, &[1, 2]));
        assert_eq!(ram.code().generation(0), Some(1));
        assert_eq!(ram.code().generation(1), Some(1));
        assert_eq!(ram.code().written_page(stamp), Some(0));
        assert_eq!(ram.code().written_page(stamp + 1), Some(1));
        assert!(ram.write_words(4, &[]));
        assert_eq!(ram.code().stamp(), stamp + 2);
    }

    #[test]
    fn a_word_run_across_the_mirror_end_is_refused() {
        let mut ram = Ram::new(0x100);
        assert!(ram.write_words(0xF8, &[1, 2]));
        assert!(!ram.write_words(0xFC, &[3, 4]));
        assert!(!ram.read_words(0x1FC, &mut [0; 2]));
        assert_eq!(ram.read(0xFC, Width::Word), 2);
        assert_eq!(ram.read(0x00, Width::Word), 0);
    }

    #[test]
    fn offsets_past_the_end_mirror() {
        let mut ram = Ram::new(0x100);
        ram.write(0x104, Width::Half, 0xBEEF);
        assert_eq!(ram.read(0x04, Width::Half), 0xBEEF);
    }

    #[test]
    fn sized_reads_match_plain_reads_at_any_length() {
        let mut ram = Ram::new(0x100);
        for offset in 0..0x100 {
            let value = (offset as u32).wrapping_mul(0x1D).wrapping_add(7);
            ram.write(offset, Width::Byte, value);
        }
        for offset in 0..0x300 {
            for width in [Width::Byte, Width::Half, Width::Word] {
                let plain = ram.read(offset, width);
                assert_eq!(ram.read_sized::<0x100>(offset, width), plain);
                assert_eq!(ram.read_sized::<0x200>(offset, width), plain);
                assert_eq!(ram.read_sized::<0x80>(offset, width), plain);
            }
        }
    }
}
