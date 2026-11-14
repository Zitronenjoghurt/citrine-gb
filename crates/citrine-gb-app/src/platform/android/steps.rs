use crate::walker::Reading;
use jni::JNIEnv;
use jni::objects::{JObject, JValue};
use jni::sys::jlong;
use std::sync::{LazyLock, Mutex, MutexGuard};

#[derive(Default)]
struct Counter {
    wanted: Option<(bool, bool)>,
    reading: Option<Reading>,
    denied: bool,
}

static COUNTER: LazyLock<Mutex<Counter>> = LazyLock::new(Mutex::default);

fn counter() -> MutexGuard<'static, Counter> {
    COUNTER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn follow(on: bool, ask: bool) {
    {
        let mut counter = counter();
        if counter.wanted == Some((on, ask)) {
            return;
        }
        counter.wanted = Some((on, ask));
        if !on {
            counter.reading = None;
        }
        if ask {
            counter.denied = false;
        }
    }
    super::java(|env, activity| {
        env.call_method(
            activity,
            "steps",
            "(ZZ)V",
            &[JValue::Bool(on.into()), JValue::Bool(ask.into())],
        )
        .map(drop)
    });
}

pub fn reading() -> Option<Reading> {
    counter().reading
}

pub fn denied() -> bool {
    counter().denied
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSteps(
    _env: JNIEnv,
    _activity: JObject,
    count: jlong,
    boot: jlong,
) {
    counter().reading = Some(Reading {
        count: count.max(0) as u64,
        boot: boot.max(0) as u64,
    });
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeStepsDenied(
    _env: JNIEnv,
    _activity: JObject,
) {
    counter().denied = true;
    super::repaint();
}
