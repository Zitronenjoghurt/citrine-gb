pub mod clipboard;
pub mod display;
pub mod files;
pub mod keyboard;
pub mod lifecycle;
pub mod pads;
pub mod push;
pub mod sensors;
#[cfg(feature = "nds")]
pub mod steps;

use android_activity::AndroidApp;
use jni::objects::{JObject, JString};
use jni::{JNIEnv, JavaVM};
use std::backtrace::Backtrace;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

static APP: OnceLock<AndroidApp> = OnceLock::new();
static VM: OnceLock<JavaVM> = OnceLock::new();
static CTX: OnceLock<egui::Context> = OnceLock::new();
static STARTED: AtomicBool = AtomicBool::new(false);

#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    crate::game_report::log_ring::install(
        Some(Box::new(android_logger::AndroidLogger::new(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Info)
                .with_tag("citrine"),
        ))),
        log::LevelFilter::Info,
    );
    if STARTED.swap(true, Ordering::Relaxed) {
        write_crash("Citrine started a second time in one process, so it closed to start fresh");
        std::process::exit(0);
    }
    let _ = APP.set(app.clone());
    install_crash_hook();

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        android_app: Some(app),
        persistence_path: data_dir().map(|dir| dir.join("app.ron")),
        persist_window: false,
        wgpu_options: crate::platform::wgpu_options(),
        ..Default::default()
    };
    if let Err(err) = eframe::run_native(
        "Citrine",
        options,
        Box::new(|cc| {
            let _ = CTX.set(cc.egui_ctx.clone());
            Ok(Box::new(crate::Citrine::new(cc)))
        }),
    ) {
        write_crash(&format!("Citrine stopped: {err}"));
    }
}

fn install_crash_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("unnamed");
        write_crash(&format!(
            "thread {name}: {info}\n\n{}",
            Backtrace::force_capture()
        ));
        previous(info);
    }));
}

fn write_crash(report: &str) {
    log::error!("{report}");
    if let Some(dir) = CACHE.get().cloned().flatten().or_else(fallback_cache_dir)
        && let Err(err) = std::fs::write(dir.join("crash-rust.txt"), report)
    {
        log::error!("could not write the crash report: {err}");
    }
}

pub fn data_dir() -> Option<PathBuf> {
    APP.get()?.internal_data_path()
}

static CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();

fn fallback_cache_dir() -> Option<PathBuf> {
    Some(data_dir()?.parent()?.join("cache"))
}

pub fn cache_dir() -> Option<PathBuf> {
    CACHE
        .get_or_init(|| {
            java(|env, activity| {
                let dir = env
                    .call_method(activity, "getCacheDir", "()Ljava/io/File;", &[])?
                    .l()?;
                let path = env
                    .call_method(&dir, "getAbsolutePath", "()Ljava/lang/String;", &[])?
                    .l()?;
                let path: String = env.get_string(&JString::from(path))?.into();
                Ok(PathBuf::from(path))
            })
            .or_else(fallback_cache_dir)
        })
        .clone()
}

fn repaint() {
    if let Some(ctx) = CTX.get() {
        ctx.request_repaint();
    }
}

fn vm() -> Option<&'static JavaVM> {
    if let Some(vm) = VM.get() {
        return Some(vm);
    }
    let pointer = APP.get()?.vm_as_ptr();
    let vm = unsafe { JavaVM::from_raw(pointer.cast()) }.ok()?;
    Some(VM.get_or_init(|| vm))
}

pub fn java<T>(call: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let activity = APP.get()?.activity_as_ptr();
    let mut env = vm()?.attach_current_thread_permanently().ok()?;
    let activity = unsafe { JObject::from_raw(activity.cast()) };
    let result = env.with_local_frame(16, |env| call(env, &activity));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }
    result
        .map_err(|err| log::error!("a call into the activity failed: {err}"))
        .ok()
}

pub fn is_tv() -> bool {
    static TV: AtomicU8 = AtomicU8::new(0);
    match TV.load(Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    let found = java(|env, activity| {
        let manager = env
            .call_method(
                activity,
                "getPackageManager",
                "()Landroid/content/pm/PackageManager;",
                &[],
            )?
            .l()?;
        let feature = env.new_string("android.software.leanback")?;
        env.call_method(
            &manager,
            "hasSystemFeature",
            "(Ljava/lang/String;)Z",
            &[(&feature).into()],
        )?
        .z()
    });
    let Some(tv) = found else {
        return false;
    };
    TV.store(if tv { 2 } else { 1 }, Ordering::Relaxed);
    tv
}

pub fn haptic() {
    java(|env, activity| env.call_method(activity, "haptic", "()V", &[]).map(drop));
}

pub fn input(ctx: &egui::Context, raw_input: &mut egui::RawInput) {
    lifecycle::input(ctx, raw_input);
    keyboard::input(raw_input);
}

pub fn frame_end(ctx: &egui::Context) {
    keyboard::follow(ctx);
    clipboard::mirror_copies(ctx);
    lifecycle::follow_theme(ctx);
    files::flush_folders();
}
