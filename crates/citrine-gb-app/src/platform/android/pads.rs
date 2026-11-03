use crate::controller_debug::PadInfo;
use crate::pad_input::android_map::{AXIS_COUNT, AndroidInput, Device, Translator};
use crate::pad_input::{PadEvent, PadSource};
use jni::JNIEnv;
use jni::objects::{JFloatArray, JObject, JString};
use jni::sys::{jboolean, jint};
use std::sync::{LazyLock, Mutex, MutexGuard};

static PADS: LazyLock<Mutex<Translator>> = LazyLock::new(Mutex::default);

fn pads() -> MutexGuard<'static, Translator> {
    PADS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn push(id: jint, input: AndroidInput) {
    pads().push(id, input);
    super::repaint();
}

pub struct AndroidPads;

impl PadSource for AndroidPads {
    fn next_event(&mut self) -> Option<PadEvent> {
        pads().next_event()
    }

    fn pads(&self, seen: &[usize]) -> Vec<PadInfo> {
        pads().pads(seen)
    }

    fn api(&self) -> String {
        "Android input".to_string()
    }
}

fn string(env: &mut JNIEnv, text: &JString) -> String {
    env.get_string(text).map(String::from).unwrap_or_default()
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePadAdded(
    mut env: JNIEnv,
    _activity: JObject,
    id: jint,
    name: JString,
    vendor: jint,
    product: jint,
    descriptor: JString,
) {
    let device = Device {
        name: string(&mut env, &name),
        vendor,
        product,
        descriptor: string(&mut env, &descriptor),
    };
    push(id, AndroidInput::Added(device));
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePadRemoved(
    _env: JNIEnv,
    _activity: JObject,
    id: jint,
) {
    push(id, AndroidInput::Removed);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePadKey(
    _env: JNIEnv,
    _activity: JObject,
    id: jint,
    code: jint,
    down: jboolean,
    repeat: jboolean,
) {
    let input = if down != 0 {
        AndroidInput::KeyDown(code, repeat != 0)
    } else {
        AndroidInput::KeyUp(code)
    };
    push(id, input);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePadAxes(
    env: JNIEnv,
    _activity: JObject,
    id: jint,
    values: JFloatArray,
) {
    let mut axes = [0.0; AXIS_COUNT];
    if env.get_float_array_region(&values, 0, &mut axes).is_ok() {
        push(id, AndroidInput::Axes(axes));
    }
}
