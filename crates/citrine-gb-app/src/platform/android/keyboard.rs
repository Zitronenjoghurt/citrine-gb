use crate::platform::ime::{self, ImeText};
use jni::JNIEnv;
use jni::objects::{JObject, JString, JValue};
use jni::sys::jint;
use std::sync::{Mutex, MutexGuard};

struct Typing {
    last: ImeText,
    wanted: bool,
    field: Option<egui::Id>,
    visible: bool,
    dismissed: bool,
    events: Vec<egui::Event>,
}

static TYPING: Mutex<Typing> = Mutex::new(Typing {
    last: ImeText::empty(),
    wanted: false,
    field: None,
    visible: false,
    dismissed: false,
    events: Vec::new(),
});

fn typing() -> MutexGuard<'static, Typing> {
    TYPING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn keyboard(show: bool) {
    super::java(|env, activity| {
        let seed = env.new_string(ImeText::seed_text())?;
        env.call_method(
            activity,
            "keyboard",
            "(ZLjava/lang/String;)V",
            &[JValue::Bool(u8::from(show)), (&seed).into()],
        )
        .map(drop)
    });
}

fn reseed() {
    super::java(|env, activity| {
        let seed = env.new_string(ImeText::seed_text())?;
        env.call_method(
            activity,
            "setText",
            "(Ljava/lang/String;)V",
            &[(&seed).into()],
        )
        .map(drop)
    });
}

pub fn follow(ctx: &egui::Context) {
    let wants = ctx.output(|o| o.ime.is_some());
    let field = ctx.memory(|m| m.focused());
    let mut typing = typing();
    if std::mem::take(&mut typing.dismissed) && wants {
        typing.wanted = false;
        typing.field = None;
        drop(typing);
        ctx.memory_mut(|m| m.stop_text_input());
        return;
    }
    let show = if wants && (!typing.wanted || typing.field != field) {
        typing.last = ImeText::seed();
        Some(true)
    } else if !wants && typing.wanted {
        Some(false)
    } else {
        None
    };
    typing.wanted = wants;
    typing.field = field.filter(|_| wants);
    drop(typing);
    if let Some(show) = show {
        keyboard(show);
    }
}

pub fn shown(visible: bool) {
    let mut typing = typing();
    if typing.visible && !visible && typing.wanted {
        typing.dismissed = true;
    }
    typing.visible = visible;
}

pub fn input(raw_input: &mut egui::RawInput) {
    raw_input.events.append(&mut typing().events);
}

pub fn paste(text: String) {
    typing().events.push(egui::Event::Paste(text));
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeText(
    mut env: JNIEnv,
    _activity: JObject,
    text: JString,
    cursor: jint,
) {
    let Ok(text) = env.get_string(&text).map(String::from) else {
        return;
    };
    let new = ImeText::from_utf16(&text, cursor);
    let mut typing = typing();
    if !typing.wanted {
        typing.last = new;
        return;
    }
    let edits = ime::edits(&typing.last, &new);
    typing.events.extend(edits);
    let empty = new.is_empty();
    typing.last = if empty { ImeText::seed() } else { new };
    drop(typing);
    if empty {
        reseed();
    }
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativePaste(
    mut env: JNIEnv,
    _activity: JObject,
    text: JString,
) {
    if let Ok(text) = env.get_string(&text) {
        paste(String::from(text));
    }
}
