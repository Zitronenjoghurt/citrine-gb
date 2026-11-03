use crate::app::ui_state::rom_folder::Listing;
use crate::utils::file_loader::PickedFile;
use crate::utils::file_saver::SaveOutcome;
use jni::JNIEnv;
use jni::objects::{JByteArray, JObject, JObjectArray, JString, JValue};
use jni::sys::jint;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, MutexGuard};

enum Waiting {
    One(Sender<PickedFile>),
    Many(Sender<Vec<PickedFile>>),
    Save(Sender<SaveOutcome>),
    Folder(Sender<PathBuf>),
    RomFolder(Sender<String>),
    Roms(String, Sender<Listing>),
}

pub fn pick_rom_folder(tx: Sender<String>) {
    let id = wait(Waiting::RomFolder(tx));
    let called = super::java(|env, activity| {
        env.call_method(activity, "openTree", "(I)V", &[JValue::Int(id)])
            .map(drop)
    });
    if called.is_none() {
        take(id);
    }
}

pub fn list_roms(tree: &str, tx: Sender<Listing>) {
    let id = wait(Waiting::Roms(tree.to_owned(), tx));
    let called = super::java(|env, activity| {
        let tree = env.new_string(tree)?;
        env.call_method(
            activity,
            "listRoms",
            "(ILjava/lang/String;)V",
            &[JValue::Int(id), JValue::Object(&tree)],
        )
        .map(drop)
    });
    if called.is_none() {
        take(id);
    }
}

pub fn read_rom(uri: &str, tx: Sender<PickedFile>) {
    let id = wait(Waiting::One(tx));
    let called = super::java(|env, activity| {
        let uri = env.new_string(uri)?;
        env.call_method(
            activity,
            "readRom",
            "(ILjava/lang/String;)V",
            &[JValue::Int(id), JValue::Object(&uri)],
        )
        .map(drop)
    });
    if called.is_none() {
        take(id);
    }
}

struct Staged {
    dir: PathBuf,
    tree: String,
    ready: bool,
}

static NEXT: AtomicI32 = AtomicI32::new(1);
static WAITING: Mutex<BTreeMap<i32, Waiting>> = Mutex::new(BTreeMap::new());
static STAGED: Mutex<Vec<Staged>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &'static Mutex<T>) -> MutexGuard<'static, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn wait(waiting: Waiting) -> i32 {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&WAITING).insert(id, waiting);
    id
}

fn take(id: i32) -> Option<Waiting> {
    lock(&WAITING).remove(&id)
}

fn open(id: i32, multiple: bool) {
    let called = super::java(|env, activity| {
        env.call_method(
            activity,
            "openDocuments",
            "(IZ)V",
            &[JValue::Int(id), JValue::Bool(u8::from(multiple))],
        )
        .map(drop)
    });
    if called.is_none() {
        take(id);
    }
}

#[derive(Default)]
pub struct FileLoader;

impl FileLoader {
    pub fn new() -> Self {
        Self
    }

    pub fn title(self, _title: &str) -> Self {
        self
    }

    pub fn add_filter(self, _name: &str, _extensions: &[&str]) -> Self {
        self
    }

    pub fn dispatch(self, tx: Sender<PickedFile>) {
        open(wait(Waiting::One(tx)), false);
    }

    pub fn dispatch_many(self, tx: Sender<Vec<PickedFile>>) {
        open(wait(Waiting::Many(tx)), true);
    }
}

#[derive(Default)]
pub struct FolderPicker;

impl FolderPicker {
    pub fn new() -> Self {
        Self
    }

    pub fn title(self, _title: &str) -> Self {
        self
    }

    pub fn dispatch(self, tx: Sender<PathBuf>) {
        let id = wait(Waiting::Folder(tx));
        let called = super::java(|env, activity| {
            env.call_method(activity, "openTree", "(I)V", &[JValue::Int(id)])
                .map(drop)
        });
        if called.is_none() {
            take(id);
        }
    }
}

pub struct FileSaver {
    name: String,
}

impl FileSaver {
    pub fn new(file_name: &str) -> Self {
        Self {
            name: file_name.to_string(),
        }
    }

    pub fn add_filter(self, _name: &str, _extensions: &[&str]) -> Self {
        self
    }

    pub fn dispatch(self, data: Vec<u8>, tx: Sender<SaveOutcome>) {
        save(vec![(self.name, data)], tx);
    }

    pub fn dispatch_copy(self, source: PathBuf, tx: Sender<SaveOutcome>) {
        match std::fs::read(&source) {
            Ok(data) => save(vec![(self.name, data)], tx),
            Err(err) => {
                let _ = tx.send(SaveOutcome::Failed(err.to_string()));
            }
        }
    }

    pub fn dispatch_pair(
        self,
        data: Vec<u8>,
        companion_extension: &'static str,
        companion: Vec<u8>,
        tx: Sender<SaveOutcome>,
    ) {
        let companion_name = std::path::Path::new(&self.name)
            .with_extension(companion_extension)
            .to_string_lossy()
            .into_owned();
        save(vec![(self.name, data), (companion_name, companion)], tx);
    }
}

fn save(files: Vec<(String, Vec<u8>)>, tx: Sender<SaveOutcome>) {
    let id = wait(Waiting::Save(tx.clone()));
    let called = super::java(|env, activity| {
        let names =
            env.new_object_array(files.len() as i32, "java/lang/String", JObject::null())?;
        let data = env.new_object_array(files.len() as i32, "[B", JObject::null())?;
        for (index, (name, bytes)) in files.iter().enumerate() {
            let name = env.new_string(name)?;
            env.set_object_array_element(&names, index as i32, name)?;
            let bytes = env.byte_array_from_slice(bytes)?;
            env.set_object_array_element(&data, index as i32, bytes)?;
        }
        env.call_method(
            activity,
            "saveDocuments",
            "(I[Ljava/lang/String;[[B)V",
            &[
                JValue::Int(id),
                JValue::Object(&names),
                JValue::Object(&data),
            ],
        )
        .map(drop)
    });
    if called.is_none() {
        take(id);
        let _ = tx.send(SaveOutcome::Failed("could not open the save dialog".into()));
    }
}

fn strings(env: &mut JNIEnv, array: &JObjectArray) -> jni::errors::Result<Vec<String>> {
    let count = env.get_array_length(array)?;
    let mut out = Vec::new();
    for index in 0..count {
        let item = JString::from(env.get_object_array_element(array, index)?);
        out.push(env.get_string(&item)?.into());
    }
    Ok(out)
}

fn picked(env: &mut JNIEnv, names: &JObjectArray, data: &JObjectArray) -> Vec<PickedFile> {
    let read = |env: &mut JNIEnv| -> jni::errors::Result<Vec<PickedFile>> {
        let names = strings(env, names)?;
        let mut files = Vec::new();
        for (index, name) in names.into_iter().enumerate() {
            let bytes = JByteArray::from(env.get_object_array_element(data, index as i32)?);
            files.push(PickedFile {
                name,
                data: env.convert_byte_array(&bytes)?,
                path: None,
            });
        }
        Ok(files)
    };
    read(env)
        .map_err(|err| log::error!("could not read the picked files: {err}"))
        .unwrap_or_default()
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeOpened(
    mut env: JNIEnv,
    _activity: JObject,
    id: jint,
    names: JObjectArray,
    data: JObjectArray,
) {
    let files = picked(&mut env, &names, &data);
    match take(id) {
        Some(Waiting::One(tx)) => {
            if let Some(file) = files.into_iter().next() {
                let _ = tx.send(file);
            }
        }
        Some(Waiting::Many(tx)) if !files.is_empty() => {
            let _ = tx.send(files);
        }
        _ => {}
    }
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeSaved(
    mut env: JNIEnv,
    _activity: JObject,
    id: jint,
    names: JObjectArray,
    error: JString,
) {
    let names = strings(&mut env, &names).unwrap_or_default();
    let error = if error.is_null() {
        None
    } else {
        env.get_string(&error).ok().map(String::from)
    };
    let outcome = match (error, names.as_slice()) {
        (Some(error), _) => SaveOutcome::Failed(error),
        (None, [first, second, ..]) => SaveOutcome::SavedPair(first.clone(), second.clone()),
        (None, [first]) => SaveOutcome::Saved(first.clone()),
        (None, []) => SaveOutcome::Cancelled,
    };
    if let Some(Waiting::Save(tx)) = take(id) {
        let _ = tx.send(outcome);
    }
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeTree(
    mut env: JNIEnv,
    _activity: JObject,
    id: jint,
    tree: JString,
) {
    let waiting = take(id);
    if tree.is_null() {
        return;
    }
    let Ok(tree) = env.get_string(&tree).map(String::from) else {
        return;
    };
    let tx = match waiting {
        Some(Waiting::Folder(tx)) => tx,
        Some(Waiting::RomFolder(tx)) => {
            let _ = tx.send(tree);
            super::repaint();
            return;
        }
        _ => return,
    };
    let dir = crate::storage::data_dir()
        .join("export")
        .join(id.to_string());
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(err) = std::fs::create_dir_all(&dir) {
        log::error!("could not make the export folder: {err}");
        return;
    }
    lock(&STAGED).push(Staged {
        dir: dir.clone(),
        tree,
        ready: false,
    });
    let _ = tx.send(dir);
    super::repaint();
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_zitronenjoghurt_citrine_MainActivity_nativeRoms(
    mut env: JNIEnv,
    _activity: JObject,
    id: jint,
    names: JObjectArray,
    uris: JObjectArray,
    readable: jni::sys::jboolean,
) {
    let Some(Waiting::Roms(tree, tx)) = take(id) else {
        return;
    };
    let names = strings(&mut env, &names).unwrap_or_default();
    let uris = strings(&mut env, &uris).unwrap_or_default();
    let _ = tx.send(Listing {
        tree,
        games: names.into_iter().zip(uris).collect(),
        readable: readable != 0,
    });
    super::repaint();
}

pub fn flush_folders() {
    let ready: Vec<Staged> = {
        let mut staged = lock(&STAGED);
        let (ready, waiting) = std::mem::take(&mut *staged)
            .into_iter()
            .partition(|folder| folder.ready);
        *staged = waiting;
        staged.iter_mut().for_each(|folder| folder.ready = true);
        ready
    };
    for folder in ready {
        let dir = folder.dir.to_string_lossy().into_owned();
        super::java(|env, activity| {
            let tree = env.new_string(&folder.tree)?;
            let dir = env.new_string(&dir)?;
            env.call_method(
                activity,
                "copyIntoTree",
                "(Ljava/lang/String;Ljava/lang/String;)V",
                &[JValue::Object(&tree), JValue::Object(&dir)],
            )
            .map(drop)
        });
        let _ = std::fs::remove_dir_all(&folder.dir);
    }
}
