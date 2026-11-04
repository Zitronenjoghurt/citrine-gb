use crate::emulator::gimmicks::camera::Feed;
use crate::senses::Wanted;
use jni::JNIEnv;
use jni::objects::{JByteArray, JObject, JValue};
use jni::sys::{jboolean, jfloat, jint};
use std::sync::{LazyLock, Mutex, MutexGuard};

#[derive(Default)]
struct Readings {
    gravity: Option<[f32; 3]>,
    gyro: [f32; 3],
    lux: Option<f32>,
    feed: Option<Feed>,
    denied: bool,
}

static READINGS: LazyLock<Mutex<Readings>> = LazyLock::new(Mutex::default);

fn readings() -> MutexGuard<'static, Readings> {
    READINGS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn want(wanted: Wanted) {
    if !wanted.motion {
        readings().gravity = None;
    }
    if !wanted.light {
        readings().lux = None;
    }
    if !wanted.camera {
        readings().denied = false;
    }
    super::java(|env, activity| {
        env.call_method(
            activity,
            "senses",
            "(ZZZ)V",
            &[
                JValue::Bool(wanted.motion.into()),
                JValue::Bool(wanted.light.into()),
                JValue::Bool(wanted.camera.into()),
            ],
        )
        .map(drop)
    });
}

pub fn motion() -> Option<([f32; 3], [f32; 3])> {
    let readings = readings();
    Some((readings.gravity?, readings.gyro))
}

pub fn light() -> Option<f32> {
    readings().lux
}

pub fn take_camera_feed() -> Option<Feed> {
    readings().feed.take()
}

pub fn camera_denied() -> bool {
    readings().denied
}

pub fn vibrate(strength: f32) {
    super::java(|env, activity| {
        env.call_method(activity, "rumble", "(F)V", &[JValue::Float(strength)])
            .map(drop)
    });
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeMotion(
    _env: JNIEnv,
    _activity: JObject,
    x: jfloat,
    y: jfloat,
    z: jfloat,
    turn: jfloat,
) {
    let mut readings = readings();
    readings.gravity = Some([x, y, z]);
    readings.gyro = [0.0, 0.0, turn];
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeLight(
    _env: JNIEnv,
    _activity: JObject,
    lux: jfloat,
) {
    readings().lux = Some(lux);
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeCameraFrame(
    env: JNIEnv,
    _activity: JObject,
    width: jint,
    height: jint,
    luma: JByteArray,
    mirror: jboolean,
) {
    let Ok(bytes) = env.convert_byte_array(&luma) else {
        return;
    };
    let (width, height) = (width as usize, height as usize);
    if width == 0 || height == 0 || bytes.len() < width * height {
        return;
    }
    readings().feed = Some(Feed::from_luma(width, height, bytes, mirror != 0));
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeCameraDenied(
    _env: JNIEnv,
    _activity: JObject,
) {
    readings().denied = true;
    super::repaint();
}
