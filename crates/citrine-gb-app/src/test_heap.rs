use std::ffi::c_int;

const M_MMAP_THRESHOLD: c_int = -3;
const MMAP_THRESHOLD: c_int = 128 * 1024;

unsafe extern "C" {
    fn mallopt(param: c_int, value: c_int) -> c_int;
}

extern "C" fn fix_mmap_threshold() {
    unsafe { mallopt(M_MMAP_THRESHOLD, MMAP_THRESHOLD) };
}

#[used]
#[unsafe(link_section = ".init_array")]
static FIX_MMAP_THRESHOLD: extern "C" fn() = fix_mmap_threshold;

#[cfg(test)]
mod tests {
    use std::process::Command;

    const IS_MMAPPED: usize = 0x2;
    const IN_A_FRESH_PROCESS: &str = "CITRINE_TEST_HEAP_FRESH";
    const THIS_TEST: &str =
        "test_heap::tests::a_big_buffer_goes_back_to_the_system_after_a_bigger_one_was_freed";

    fn chunk_is_mapped(buffer: &[u8]) -> bool {
        let size_field = unsafe { buffer.as_ptr().cast::<usize>().sub(1).read() };
        size_field & IS_MMAPPED != 0
    }

    fn a_big_buffer_after_a_bigger_one_is_mapped() -> bool {
        drop(std::hint::black_box(vec![1u8; 8 << 20]));
        let buffer = std::hint::black_box(vec![1u8; 1 << 20]);
        chunk_is_mapped(&buffer)
    }

    fn leave_a_free_hole_in_this_threads_heap() -> Vec<u8> {
        let blocks: Vec<Vec<u8>> = (0..16).map(|_| vec![1u8; 100 << 10]).collect();
        let guard = std::hint::black_box(vec![1u8; 50 << 10]);
        drop(blocks);
        guard
    }

    #[test]
    fn a_big_buffer_goes_back_to_the_system_after_a_bigger_one_was_freed() {
        if std::env::var_os(IN_A_FRESH_PROCESS).is_some() {
            assert!(a_big_buffer_after_a_bigger_one_is_mapped());
            return;
        }
        let _hole = leave_a_free_hole_in_this_threads_heap();
        let fresh = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", THIS_TEST, "--test-threads=1"])
            .env(IN_A_FRESH_PROCESS, "1")
            .env_remove("MALLOC_MMAP_THRESHOLD_")
            .env_remove("MALLOC_ARENA_MAX")
            .env_remove("GLIBC_TUNABLES")
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&fresh.stdout);
        assert!(fresh.status.success(), "{out}");
        assert!(out.contains("1 passed"), "{out}");
    }
}
