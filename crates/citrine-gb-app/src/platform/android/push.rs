use crate::online::push::StatusLine;
use crate::platform::push::Opened;
use jni::JNIEnv;
use jni::objects::{JObject, JString};
use jni::sys::jboolean;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static AVAILABLE: AtomicBool = AtomicBool::new(false);
static ALLOWED: AtomicBool = AtomicBool::new(false);
static TOKEN: Mutex<Option<String>> = Mutex::new(None);
static OPENED: Mutex<Option<Opened>> = Mutex::new(None);
static STATUS: Mutex<Option<StatusLine>> = Mutex::new(None);

pub fn available() -> bool {
    AVAILABLE.load(Ordering::Relaxed)
}

pub fn allowed() -> bool {
    available() && ALLOWED.load(Ordering::Relaxed)
}

pub fn ask() {
    super::java(|env, activity| {
        env.call_method(activity, "askNotifications", "()V", &[])
            .map(drop)
    });
}

pub fn open_settings() {
    super::java(|env, activity| {
        env.call_method(activity, "notificationSettings", "()V", &[])
            .map(drop)
    });
}

pub fn with_token<R>(read: impl FnOnce(Option<&str>) -> R) -> R {
    match TOKEN.lock() {
        Ok(token) => read(token.as_deref()),
        Err(_) => read(None),
    }
}

pub fn take_opened() -> Option<Opened> {
    OPENED.lock().ok()?.take()
}

pub fn keep_status_line(line: Option<StatusLine>) {
    let Ok(mut kept) = STATUS.lock() else {
        return;
    };
    let same = match (&*kept, &line) {
        (Some(a), Some(b)) => a.same(b),
        (None, None) => true,
        _ => false,
    };
    if !same {
        *kept = line;
    }
}

pub fn went_hidden() {
    if let Ok(kept) = STATUS.lock()
        && let Some(line) = kept.as_ref()
    {
        line.away();
    }
}

fn string(env: &mut JNIEnv, text: &JString) -> String {
    env.get_string(text).map(String::from).unwrap_or_default()
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePush(
    _env: JNIEnv,
    _activity: JObject,
    available: jboolean,
    allowed: jboolean,
) {
    AVAILABLE.store(available != 0, Ordering::Relaxed);
    ALLOWED.store(allowed != 0, Ordering::Relaxed);
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePushToken(
    mut env: JNIEnv,
    _activity: JObject,
    token: JString,
) {
    let token = string(&mut env, &token);
    if let Ok(mut kept) = TOKEN.lock() {
        *kept = Some(token).filter(|t| !t.is_empty());
    }
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeOpen(
    mut env: JNIEnv,
    _activity: JObject,
    kind: JString,
    from: JString,
    session: JString,
) {
    let kind = string(&mut env, &kind);
    let from = string(&mut env, &from);
    let session = string(&mut env, &session);
    if let Some(opened) = Opened::parse(&kind, &from, &session)
        && let Ok(mut kept) = OPENED.lock()
    {
        *kept = Some(opened);
    }
    super::repaint();
}
