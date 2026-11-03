use crate::audio::Audio;
use crate::autosave::LastGood;
use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use jni::sys::{jboolean, jint};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering};

static LAST_GOOD: OnceLock<LastGood> = OnceLock::new();
static HIDDEN: AtomicBool = AtomicBool::new(false);
static BACKS: AtomicUsize = AtomicUsize::new(0);
static AT_ROOT: AtomicBool = AtomicBool::new(false);
static SAFE_AREA: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];
static KEYBOARD: AtomicI32 = AtomicI32::new(0);
static BARS: AtomicU8 = AtomicU8::new(0);
static IMMERSIVE: AtomicU8 = AtomicU8::new(0);

pub fn save_on_pause(last_good: LastGood) {
    let _ = LAST_GOOD.set(last_good);
}

pub fn follow_visibility(audio: &mut Audio) {
    if HIDDEN.load(Ordering::Relaxed) {
        audio.pause();
    } else {
        audio.resume();
    }
}

pub fn at_root(root: bool) {
    AT_ROOT.store(root, Ordering::Relaxed);
}

fn keyboard_height(ctx: &egui::Context) -> f32 {
    KEYBOARD.load(Ordering::Relaxed) as f32 / ctx.pixels_per_point()
}

fn leave() {
    super::java(|env, activity| env.call_method(activity, "leave", "()V", &[]).map(drop));
}

fn back(events: &mut Vec<egui::Event>) {
    if AT_ROOT.swap(false, Ordering::Relaxed) {
        leave();
        return;
    }
    for pressed in [true, false] {
        events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
}

pub fn input(ctx: &egui::Context, raw_input: &mut egui::RawInput) {
    let points = ctx.pixels_per_point();
    let [left, top, right, bottom] = SAFE_AREA
        .each_ref()
        .map(|inset| inset.load(Ordering::Relaxed) as f32 / points);
    raw_input.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
        left,
        right,
        top,
        bottom: bottom + keyboard_height(ctx),
    }));
    let mut backs = BACKS.swap(0, Ordering::Relaxed);
    raw_input.events.retain(|event| match event {
        egui::Event::Key {
            key: egui::Key::BrowserBack,
            pressed,
            ..
        } => {
            backs += usize::from(*pressed);
            false
        }
        _ => true,
    });
    for _ in 0..backs {
        back(&mut raw_input.events);
    }
}

pub fn immersive(on: bool) {
    let want = if on { 2 } else { 1 };
    if IMMERSIVE.swap(want, Ordering::Relaxed) == want {
        return;
    }
    super::java(|env, activity| {
        env.call_method(activity, "immersive", "(Z)V", &[JValue::Bool(u8::from(on))])
            .map(drop)
    });
}

pub fn follow_theme(ctx: &egui::Context) {
    let light = !ctx.style().visuals.dark_mode;
    let want = if light { 2 } else { 1 };
    if BARS.swap(want, Ordering::Relaxed) == want {
        return;
    }
    super::java(|env, activity| {
        env.call_method(
            activity,
            "lightBars",
            "(Z)V",
            &[JValue::Bool(u8::from(light))],
        )
        .map(drop)
    });
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSafeArea(
    _env: JNIEnv,
    _activity: JObject,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
    keyboard: jint,
) {
    for (inset, pixels) in SAFE_AREA.iter().zip([left, top, right, bottom]) {
        inset.store(pixels, Ordering::Relaxed);
    }
    KEYBOARD.store((keyboard - bottom).max(0), Ordering::Relaxed);
    super::keyboard::shown(keyboard > bottom);
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePause(
    _env: JNIEnv,
    _activity: JObject,
) {
    if let Some(last_good) = LAST_GOOD.get()
        && let Err(err) = last_good.write_out()
    {
        log::error!("could not autosave when the app was paused: {err}");
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeVisible(
    _env: JNIEnv,
    _activity: JObject,
    visible: jboolean,
) {
    HIDDEN.store(visible == 0, Ordering::Relaxed);
    if visible != 0 {
        BARS.store(0, Ordering::Relaxed);
        IMMERSIVE.store(0, Ordering::Relaxed);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeBack(
    _env: JNIEnv,
    _activity: JObject,
) {
    BACKS.fetch_add(1, Ordering::Relaxed);
    super::repaint();
}
