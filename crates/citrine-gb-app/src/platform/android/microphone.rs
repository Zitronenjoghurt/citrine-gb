use jni::JNIEnv;
use jni::objects::JObject;
use jni::sys::jboolean;
use std::sync::atomic::{AtomicU8, Ordering};

const WAITING: u8 = 0;
const GRANTED: u8 = 1;
const DENIED: u8 = 2;

static ANSWER: AtomicU8 = AtomicU8::new(WAITING);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Waiting,
    Granted,
    Denied,
}

pub fn ask() {
    ANSWER.store(WAITING, Ordering::Relaxed);
    super::java(|env, activity| {
        env.call_method(activity, "askMicrophone", "()V", &[])
            .map(drop)
    });
}

pub fn permission() -> Permission {
    match ANSWER.load(Ordering::Relaxed) {
        GRANTED => Permission::Granted,
        DENIED => Permission::Denied,
        _ => Permission::Waiting,
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeMicrophone(
    _env: JNIEnv,
    _activity: JObject,
    granted: jboolean,
) {
    let answer = if granted != 0 { GRANTED } else { DENIED };
    ANSWER.store(answer, Ordering::Relaxed);
    super::repaint();
}
