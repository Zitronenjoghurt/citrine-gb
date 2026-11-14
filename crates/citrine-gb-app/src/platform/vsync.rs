use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

type Callback =
    extern "C" fn(*mut c_void, *const c_void, *const c_void, u64, *mut u64, *mut c_void) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(link: *mut *mut c_void) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        link: *mut c_void,
        callback: Callback,
        info: *mut c_void,
    ) -> i32;
    fn CVDisplayLinkStart(link: *mut c_void) -> i32;
    fn CVDisplayLinkStop(link: *mut c_void) -> i32;
    fn CVDisplayLinkRelease(link: *mut c_void);
    fn CVDisplayLinkGetActualOutputVideoRefreshPeriod(link: *mut c_void) -> f64;
}

struct Shared {
    count: AtomicU64,
    wake_at: AtomicU64,
    ctx: egui::Context,
}

pub struct VsyncLink {
    link: *mut c_void,
    shared: *const Shared,
}

unsafe impl Send for VsyncLink {}

unsafe impl Sync for VsyncLink {}

impl VsyncLink {
    pub fn start(ctx: &egui::Context) -> Option<Self> {
        let mut link = std::ptr::null_mut();
        if unsafe { CVDisplayLinkCreateWithActiveCGDisplays(&mut link) } != 0 || link.is_null() {
            return None;
        }
        let shared = Arc::into_raw(Arc::new(Shared {
            count: AtomicU64::new(0),
            wake_at: AtomicU64::new(0),
            ctx: ctx.clone(),
        }));
        let this = Self { link, shared };
        let set =
            unsafe { CVDisplayLinkSetOutputCallback(link, on_vsync, shared.cast_mut().cast()) };
        if set != 0 || unsafe { CVDisplayLinkStart(link) } != 0 {
            return None;
        }
        Some(this)
    }

    fn shared(&self) -> &Shared {
        unsafe { &*self.shared }
    }

    pub fn count(&self) -> u64 {
        self.shared().count.load(Ordering::Acquire)
    }

    pub fn wake_at(&self, count: u64) {
        self.shared().wake_at.store(count, Ordering::Release);
    }

    pub fn period(&self) -> Option<f64> {
        let period = unsafe { CVDisplayLinkGetActualOutputVideoRefreshPeriod(self.link) };
        (period > 0.0).then_some(period)
    }
}

impl Drop for VsyncLink {
    fn drop(&mut self) {
        unsafe {
            CVDisplayLinkStop(self.link);
            CVDisplayLinkRelease(self.link);
            drop(Arc::from_raw(self.shared));
        }
    }
}

extern "C" fn on_vsync(
    _link: *mut c_void,
    _now: *const c_void,
    _output: *const c_void,
    _flags: u64,
    _flags_out: *mut u64,
    info: *mut c_void,
) -> i32 {
    let shared = unsafe { &*info.cast_const().cast::<Shared>() };
    let count = shared.count.fetch_add(1, Ordering::AcqRel) + 1;
    let at = shared.wake_at.load(Ordering::Acquire);
    if at != 0
        && count >= at
        && shared
            .wake_at
            .compare_exchange(at, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        shared.ctx.request_repaint();
    }
    0
}

#[cfg(test)]
mod tests {
    use super::VsyncLink;

    #[test]
    #[ignore = "needs a Mac with a display"]
    fn a_display_link_ticks_without_a_window() {
        let ctx = egui::Context::default();
        let Some(link) = VsyncLink::start(&ctx) else {
            println!("vsync probe: no display link");
            return;
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        println!(
            "vsync probe: count {} period {:?}",
            link.count(),
            link.period()
        );
    }
}
