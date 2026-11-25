#[cfg(target_os = "android")]
#[expect(
    clippy::disallowed_types,
    reason = "Java threads write the JNI statics"
)]
pub mod android;
#[cfg(any(target_os = "android", test))]
pub mod ime;
pub mod monitor;
pub mod push;
#[cfg(target_os = "macos")]
pub mod vsync;

#[cfg(target_os = "android")]
pub fn haptic_tick() {
    android::haptic();
}

#[cfg(target_arch = "wasm32")]
pub fn haptic_tick() {
    if let Some(window) = web_sys::window() {
        let _ = window.navigator().vibrate_with_duration(15);
    }
}

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn haptic_tick() {}

#[cfg(target_os = "android")]
pub fn immersive(on: bool) {
    android::lifecycle::immersive(on);
}

#[cfg(not(target_os = "android"))]
pub fn immersive(_on: bool) {}

#[cfg(target_os = "android")]
pub fn hold_orientation(landscape: Option<bool>) -> bool {
    android::hold_orientation(landscape)
}

#[cfg(not(target_os = "android"))]
pub fn hold_orientation(_landscape: Option<bool>) -> bool {
    false
}

#[cfg(target_os = "android")]
pub fn is_tv() -> bool {
    android::is_tv()
}

#[cfg(not(target_os = "android"))]
pub fn is_tv() -> bool {
    false
}

pub const CAN_QUIT: bool = !cfg!(target_arch = "wasm32");

#[cfg(target_os = "android")]
pub fn quit(_ctx: &egui::Context) {
    android::lifecycle::leave();
}

#[cfg(not(target_os = "android"))]
pub fn quit(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
}

#[cfg(target_os = "android")]
pub fn pad_keyboard(on: bool) {
    android::keyboard::pad_keyboard(on);
}

#[cfg(not(target_os = "android"))]
pub fn pad_keyboard(_on: bool) {}

#[cfg(not(target_arch = "wasm32"))]
pub fn wgpu_options() -> eframe::egui_wgpu::WgpuConfiguration {
    use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup, WgpuSetupCreateNew};
    let mut create = WgpuSetupCreateNew::default();
    let base = create.device_descriptor;
    create.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = base(adapter);
        descriptor.required_features |= adapter.features() & wgpu::Features::TIMESTAMP_QUERY;
        descriptor
    });
    let base = WgpuConfiguration::default();
    let skip = std::sync::Arc::clone(&base.on_surface_error);
    WgpuConfiguration {
        wgpu_setup: WgpuSetup::CreateNew(create),
        on_surface_error: std::sync::Arc::new(move |error| {
            SURFACE_ERRORS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            skip(error)
        }),
        ..base
    }
}

#[cfg(not(target_arch = "wasm32"))]
static SURFACE_ERRORS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
pub fn surface_errors() -> u64 {
    SURFACE_ERRORS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(target_os = "macos")]
pub fn power() -> Option<(&'static str, bool)> {
    use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
    let info = NSProcessInfo::processInfo();
    let thermal = match info.thermalState() {
        NSProcessInfoThermalState::Nominal => "nominal",
        NSProcessInfoThermalState::Fair => "fair",
        NSProcessInfoThermalState::Serious => "serious",
        NSProcessInfoThermalState::Critical => "critical",
        _ => "unknown",
    };
    Some((thermal, info.isLowPowerModeEnabled()))
}

#[cfg(not(target_os = "macos"))]
pub fn power() -> Option<(&'static str, bool)> {
    None
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn cpu_name() -> Option<String> {
    let info = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    info.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| matches!(key.trim(), "model name" | "Hardware" | "Model"))
        .map(|(_, value)| value.trim().to_string())
}

#[cfg(target_os = "macos")]
pub fn cpu_name() -> Option<String> {
    let out = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!name.is_empty()).then_some(name)
}

#[cfg(windows)]
pub fn cpu_name() -> Option<String> {
    std::env::var("PROCESSOR_IDENTIFIER").ok()
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
pub fn cpu_name() -> Option<String> {
    None
}
