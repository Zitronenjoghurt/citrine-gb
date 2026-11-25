use super::native;
use super::{Fast, JitBus};
use crate::gba::cpu::Core;
use crate::jit_cache::{LINK, Reloc};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const CHUNK_BYTES: usize = 1 << 20;
const ALIGN: usize = 16;

type Entry<const V5: bool, B> =
    unsafe extern "C" fn(*mut Core<V5>, *mut B, u32, u32, *mut Fast, *const u32) -> u64;

pub unsafe fn call<const V5: bool, B: JitBus>(
    entry: usize,
    core: &mut Core<V5>,
    bus: &mut B,
    budget: u32,
    start: u32,
    fast: *mut Fast,
    words: *const u32,
) -> u64 {
    let code: Entry<V5, B> = unsafe { std::mem::transmute::<usize, Entry<V5, B>>(entry) };
    unsafe { code(core, bus, budget, start, fast, words) }
}

struct Chunk {
    base: *mut u8,
    view: *mut u8,
    sealed: AtomicBool,
}

unsafe impl Send for Chunk {}
unsafe impl Sync for Chunk {}

impl Chunk {
    fn map() -> Option<Self> {
        let (base, view) = unsafe { os::map(CHUNK_BYTES) }?;
        Some(Self {
            base,
            view,
            sealed: AtomicBool::new(false),
        })
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe { os::unmap(self.base, self.view, CHUNK_BYTES) };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Linked {
    pub bytes: Vec<u8>,
    pub links: Vec<Reloc>,
}

impl Linked {
    pub fn plain(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            links: Vec::new(),
        }
    }
}

pub struct Prelude {
    bytes: Box<[u8]>,
    targets: Box<[usize]>,
}

impl Prelude {
    pub fn new(bytes: &[u8], targets: Vec<usize>) -> Option<Self> {
        targets
            .iter()
            .all(|&target| target < bytes.len())
            .then(|| Self {
                bytes: bytes.into(),
                targets: targets.into(),
            })
    }
}

pub fn relocate(code: &[u8], relocs: &[Reloc], table: &[usize]) -> Option<Linked> {
    let mut placed = Linked::plain(code.to_vec());
    for reloc in relocs {
        let at = reloc.offset as usize;
        if reloc.helper & LINK != 0 {
            placed.bytes.get(at..at + 4)?;
            placed.links.push(*reloc);
            continue;
        }
        let address = *table.get(reloc.helper as usize)? as u64;
        placed
            .bytes
            .get_mut(at..at + 8)?
            .copy_from_slice(&address.to_le_bytes());
    }
    Some(placed)
}

#[derive(Clone)]
pub struct Code {
    entry: usize,
    _chunk: Arc<Chunk>,
}

impl Code {
    pub fn entry(&self) -> usize {
        self.entry
    }
}

#[derive(Default)]
pub struct Arena {
    chunk: Option<Arc<Chunk>>,
    used: usize,
    allocated: usize,
    targets: Vec<usize>,
}

impl Arena {
    pub fn allocated(&self) -> usize {
        self.allocated
    }

    pub fn seal(&self) {
        if let Some(chunk) = &self.chunk {
            chunk.sealed.store(true, Ordering::Release);
        }
    }

    pub fn place(&mut self, mut code: Linked, prelude: Option<&Prelude>) -> Option<Code> {
        let linked = !code.links.is_empty();
        let head = prelude.map_or(0, |prelude| prelude.bytes.len().next_multiple_of(ALIGN));
        if (linked && prelude.is_none()) || head + code.bytes.len() > CHUNK_BYTES {
            return None;
        }
        let start = self.used.next_multiple_of(ALIGN);
        let fits = start + code.bytes.len() <= CHUNK_BYTES;
        let chunk = match &self.chunk {
            Some(chunk)
                if fits
                    && !(linked && self.targets.is_empty())
                    && !chunk.sealed.load(Ordering::Acquire) =>
            {
                Arc::clone(chunk)
            }
            _ => {
                let chunk = Arc::new(Chunk::map()?);
                self.chunk = Some(Arc::clone(&chunk));
                self.used = 0;
                self.allocated += CHUNK_BYTES;
                self.targets.clear();
                if let Some(prelude) = prelude {
                    unsafe { os::write(chunk.base, chunk.view, &prelude.bytes) }?;
                    self.used = prelude.bytes.len();
                    let base = chunk.base as usize;
                    self.targets = prelude.targets.iter().map(|&at| base + at).collect();
                }
                chunk
            }
        };
        let start = self.used.next_multiple_of(ALIGN);
        let at = unsafe { chunk.base.add(start) };
        let view = unsafe { chunk.view.add(start) };
        for link in &code.links {
            let site = link.offset as usize;
            let target = *self.targets.get((link.helper & !LINK) as usize)?;
            native::link(
                code.bytes.get_mut(site..site + 4)?,
                at as usize + site,
                target,
            )?;
        }
        unsafe { os::write(at, view, &code.bytes) }?;
        self.used = start + code.bytes.len();
        Some(Code {
            entry: at as usize,
            _chunk: chunk,
        })
    }
}

#[cfg(all(unix, not(all(target_vendor = "apple", target_arch = "aarch64"))))]
mod os {
    use std::ptr;

    fn page() -> usize {
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }

    pub unsafe fn map(len: usize) -> Option<(*mut u8, *mut u8)> {
        #[cfg(target_os = "linux")]
        if let Some(twin) = unsafe { twin(len) } {
            return Some(twin);
        }
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        (base != libc::MAP_FAILED).then_some((base.cast(), base.cast()))
    }

    #[cfg(target_os = "linux")]
    unsafe fn twin(len: usize) -> Option<(*mut u8, *mut u8)> {
        let fd = unsafe { libc::memfd_create(c"citrine-jit".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return None;
        }
        let share =
            |protect| unsafe { libc::mmap(ptr::null_mut(), len, protect, libc::MAP_SHARED, fd, 0) };
        let mut twin = None;
        if unsafe { libc::ftruncate(fd, len as libc::off_t) } == 0 {
            let view = share(libc::PROT_READ | libc::PROT_WRITE);
            let base = share(libc::PROT_READ | libc::PROT_EXEC);
            if view != libc::MAP_FAILED && base != libc::MAP_FAILED {
                twin = Some((base.cast(), view.cast()));
            } else {
                for mapped in [view, base] {
                    if mapped != libc::MAP_FAILED {
                        unsafe { libc::munmap(mapped, len) };
                    }
                }
            }
        }
        unsafe { libc::close(fd) };
        twin
    }

    pub unsafe fn unmap(base: *mut u8, view: *mut u8, len: usize) {
        unsafe { libc::munmap(base.cast(), len) };
        if view != base {
            unsafe { libc::munmap(view.cast(), len) };
        }
    }

    pub unsafe fn write(at: *mut u8, view: *mut u8, bytes: &[u8]) -> Option<()> {
        if view != at {
            unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), view, bytes.len()) };
            unsafe { flush(view, at, bytes.len()) };
            return Some(());
        }
        let page = page();
        let first = at as usize & !(page - 1);
        let end = (at as usize + bytes.len()).next_multiple_of(page);
        let protect = |flags| unsafe { libc::mprotect(first as *mut _, end - first, flags) == 0 };
        if !protect(libc::PROT_READ | libc::PROT_WRITE) {
            return None;
        }
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len()) };
        if !protect(libc::PROT_READ | libc::PROT_EXEC) {
            return None;
        }
        unsafe { flush(at, at, bytes.len()) };
        Some(())
    }

    #[cfg(target_arch = "aarch64")]
    unsafe fn flush(view: *mut u8, at: *mut u8, len: usize) {
        use std::arch::asm;
        let ctr: u64;
        unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr) };
        let data_line = 4usize << ((ctr >> 16) & 0xF);
        let code_line = 4usize << (ctr & 0xF);
        let end = view as usize + len;
        let mut line = view as usize & !(data_line - 1);
        while line < end {
            unsafe { asm!("dc cvau, {}", in(reg) line) };
            line += data_line;
        }
        unsafe { asm!("dsb ish") };
        let end = at as usize + len;
        let mut line = at as usize & !(code_line - 1);
        while line < end {
            unsafe { asm!("ic ivau, {}", in(reg) line) };
            line += code_line;
        }
        unsafe { asm!("dsb ish", "isb") };
    }

    #[cfg(not(target_arch = "aarch64"))]
    unsafe fn flush(_view: *mut u8, _at: *mut u8, _len: usize) {}
}

#[cfg(all(target_vendor = "apple", target_arch = "aarch64"))]
mod os {
    use std::ffi::c_void;
    use std::ptr;

    unsafe extern "C" {
        fn sys_icache_invalidate(start: *mut c_void, len: usize);
    }

    pub unsafe fn map(len: usize) -> Option<(*mut u8, *mut u8)> {
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE | libc::PROT_EXEC,
                libc::MAP_PRIVATE | libc::MAP_ANON | libc::MAP_JIT,
                -1,
                0,
            )
        };
        (base != libc::MAP_FAILED).then_some((base.cast(), base.cast()))
    }

    pub unsafe fn unmap(base: *mut u8, _view: *mut u8, len: usize) {
        unsafe { libc::munmap(base.cast(), len) };
    }

    pub unsafe fn write(at: *mut u8, _view: *mut u8, bytes: &[u8]) -> Option<()> {
        unsafe {
            libc::pthread_jit_write_protect_np(0);
            ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len());
            libc::pthread_jit_write_protect_np(1);
            sys_icache_invalidate(at.cast(), bytes.len());
        }
        Some(())
    }
}

#[cfg(windows)]
mod os {
    use std::ptr;
    use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_PROTECTION_FLAGS,
        PAGE_READONLY, PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
    };
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    fn page() -> usize {
        let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut info) };
        info.dwPageSize as usize
    }

    pub unsafe fn map(len: usize) -> Option<(*mut u8, *mut u8)> {
        let base =
            unsafe { VirtualAlloc(ptr::null(), len, MEM_COMMIT | MEM_RESERVE, PAGE_READONLY) };
        (!base.is_null()).then_some((base.cast(), base.cast()))
    }

    pub unsafe fn unmap(base: *mut u8, _view: *mut u8, _len: usize) {
        unsafe { VirtualFree(base.cast(), 0, MEM_RELEASE) };
    }

    pub unsafe fn write(at: *mut u8, _view: *mut u8, bytes: &[u8]) -> Option<()> {
        let page = page();
        let first = at as usize & !(page - 1);
        let end = (at as usize + bytes.len()).next_multiple_of(page);
        let protect = |flags| {
            let mut old: PAGE_PROTECTION_FLAGS = 0;
            unsafe { VirtualProtect(first as *const _, end - first, flags, &mut old) != 0 }
        };
        if !protect(PAGE_READWRITE) {
            return None;
        }
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len()) };
        if !protect(PAGE_EXECUTE_READ) {
            return None;
        }
        let flushed = unsafe { FlushInstructionCache(GetCurrentProcess(), at.cast(), bytes.len()) };
        (flushed != 0).then_some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "x86_64")]
    const RETURN_SEVEN: &[u8] = &[0xB8, 0x07, 0x00, 0x00, 0x00, 0xC3];

    #[cfg(target_arch = "aarch64")]
    const RETURN_SEVEN: &[u8] = &[0xE0, 0x00, 0x80, 0xD2, 0xC0, 0x03, 0x5F, 0xD6];

    #[cfg(target_arch = "x86_64")]
    const NOP: &[u8] = &[0x90];

    #[cfg(target_arch = "aarch64")]
    const NOP: &[u8] = &[0x1F, 0x20, 0x03, 0xD5];

    fn padded(nops: usize) -> Vec<u8> {
        let mut bytes = NOP.repeat(nops);
        bytes.extend_from_slice(RETURN_SEVEN);
        bytes
    }

    fn run(code: &Code) -> u64 {
        let function: extern "C" fn() -> u64 = unsafe { std::mem::transmute(code.entry()) };
        function()
    }

    #[test]
    fn placed_code_runs_and_a_second_block_lands_after_it() {
        let mut arena = Arena::default();
        let first = arena
            .place(Linked::plain(RETURN_SEVEN.to_vec()), None)
            .expect("the arena maps");
        let second = arena
            .place(Linked::plain(RETURN_SEVEN.to_vec()), None)
            .expect("the arena maps");
        assert_eq!(run(&first), 7);
        assert_eq!(run(&second), 7);
        assert_eq!(
            second.entry() - first.entry(),
            RETURN_SEVEN.len().next_multiple_of(ALIGN)
        );
    }

    #[cfg(unix)]
    fn page_size() -> usize {
        unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
    }

    #[cfg(windows)]
    fn page_size() -> usize {
        use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
        let mut info: SYSTEM_INFO = unsafe { std::mem::zeroed() };
        unsafe { GetSystemInfo(&mut info) };
        info.dwPageSize as usize
    }

    #[test]
    fn blocks_across_a_page_edge_run_with_the_systems_page_size() {
        let page = page_size();
        let mut arena = Arena::default();
        let first_bytes = padded(64);
        let first = arena
            .place(Linked::plain(first_bytes.clone()), None)
            .expect("the arena maps");
        let next = (first.entry() + first_bytes.len()).next_multiple_of(ALIGN) & (page - 1);
        let filler = (page - 128 - next - RETURN_SEVEN.len()) / NOP.len();
        let filled = arena
            .place(Linked::plain(padded(filler)), None)
            .expect("the filler fits");
        let across = arena
            .place(Linked::plain(padded(256 / NOP.len())), None)
            .expect("the block fits");
        let edge = across.entry().next_multiple_of(page);
        assert!(across.entry() < edge && edge < across.entry() + 256);
        let later = arena
            .place(Linked::plain(padded(2 * page / NOP.len())), None)
            .expect("the block fits");
        for code in [&first, &filled, &across, &later] {
            assert_eq!(run(code), 7);
        }
    }

    #[cfg(windows)]
    #[test]
    fn placed_code_is_execute_read_and_never_writable() {
        use windows_sys::Win32::System::Memory::{
            MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, VirtualQuery,
        };
        let mut arena = Arena::default();
        let code = arena.place(RETURN_SEVEN).expect("the arena maps");
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info);
        let read = unsafe { VirtualQuery(code.entry() as *const _, &mut info, size) };
        assert_eq!(read, size);
        assert_eq!(info.Protect, PAGE_EXECUTE_READ);
        assert_eq!(run(&code), 7);
    }

    #[test]
    fn relocations_write_each_helper_address_and_leave_the_rest() {
        let code = [0xAAu8; 24];
        let relocs = [
            Reloc {
                offset: 2,
                helper: 1,
            },
            Reloc {
                offset: 14,
                helper: 0,
            },
        ];
        let placed = relocate(
            &code,
            &relocs,
            &[0x1122_3344_5566_7788, 0x0102_0304_0506_0708],
        )
        .expect("the relocations fit");
        assert!(placed.links.is_empty());
        let placed = placed.bytes;
        assert_eq!(&placed[..2], &[0xAA; 2]);
        assert_eq!(&placed[2..10], &0x0102_0304_0506_0708u64.to_le_bytes());
        assert_eq!(&placed[10..14], &[0xAA; 4]);
        assert_eq!(&placed[14..22], &0x1122_3344_5566_7788u64.to_le_bytes());
        assert_eq!(&placed[22..], &[0xAA; 2]);
    }

    #[test]
    fn a_relocation_past_the_code_or_the_table_is_refused() {
        let code = [0u8; 8];
        let past = Reloc {
            offset: 1,
            helper: 0,
        };
        assert_eq!(relocate(&code, &[past], &[1]), None);
        let unknown = Reloc {
            offset: 0,
            helper: 3,
        };
        assert_eq!(relocate(&code, &[unknown], &[1]), None);
    }

    #[test]
    fn links_pass_through_relocation_untouched() {
        let code = [0xAAu8; 12];
        let link = Reloc {
            offset: 8,
            helper: LINK | 2,
        };
        let placed = relocate(&code, &[link], &[]).expect("the link fits");
        assert_eq!(placed.bytes, code);
        assert_eq!(placed.links, [link]);
        let past = Reloc {
            offset: 10,
            helper: LINK,
        };
        assert_eq!(relocate(&code, &[past], &[]), None);
    }

    #[test]
    fn linked_code_needs_a_prelude() {
        let mut arena = Arena::default();
        let code = Linked {
            bytes: vec![0; 4],
            links: vec![Reloc {
                offset: 0,
                helper: LINK,
            }],
        };
        assert!(arena.place(code, None).is_none());
        assert_eq!(arena.allocated(), 0);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn linked_branches_reach_the_prelude_of_their_chunk() {
        let prelude = Prelude::new(RETURN_SEVEN, vec![0]).expect("the target fits");
        let mut arena = Arena::default();
        let mut entries = Vec::new();
        for _ in 0..2 {
            let code = Linked {
                bytes: 0x1400_0000u32.to_le_bytes().to_vec(),
                links: vec![Reloc {
                    offset: 0,
                    helper: LINK,
                }],
            };
            entries.push(arena.place(code, Some(&prelude)).expect("the arena maps"));
            arena.seal();
        }
        assert_eq!(arena.allocated(), 2 * CHUNK_BYTES);
        for code in &entries {
            assert_eq!(run(code), 7);
        }
    }

    #[test]
    fn a_sealed_chunk_is_never_written_again() {
        let mut arena = Arena::default();
        let first = arena
            .place(Linked::plain(RETURN_SEVEN.to_vec()), None)
            .expect("the arena maps");
        arena.seal();
        let second = arena
            .place(Linked::plain(RETURN_SEVEN.to_vec()), None)
            .expect("the arena maps");
        assert_eq!(arena.allocated(), 2 * CHUNK_BYTES);
        assert_eq!(run(&first), 7);
        assert_eq!(run(&second), 7);
    }
}
