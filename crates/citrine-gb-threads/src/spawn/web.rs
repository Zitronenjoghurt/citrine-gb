use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen::prelude::*;

type Task = Box<dyn FnOnce() + Send>;

#[wasm_bindgen(inline_js = r#"
const source = `
self.onmessage = async (event) => {
  const [glue, module, memory, task] = event.data;
  globalThis.citrineGlue = glue;
  try {
    const bindings = await import(glue);
    const wasm = await bindings.default({ module_or_path: module, memory });
    bindings.citrine_thread_run(task);
    wasm.__wbindgen_thread_destroy();
  } catch (err) {
    self.postMessage(String((err && err.stack) || err));
  }
  self.close();
};
`;

let url;

export function glue() {
  return typeof globalThis.citrineGlue === "string" ? globalThis.citrineGlue : undefined;
}

export function isolated() {
  return globalThis.crossOriginIsolated === true;
}

export function cores() {
  return globalThis.navigator?.hardwareConcurrency ?? 1;
}

export function start(name, module, memory, task, failed) {
  url ??= URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
  const worker = new Worker(url, { type: "module", name });
  let told = false;
  const fail = (text) => {
    if (!told) {
      told = true;
      failed(text);
    }
  };
  worker.onmessage = (event) => fail(String(event.data));
  worker.onerror = (event) => fail(event.message || "the worker did not start");
  worker.postMessage([glue(), module, memory, task]);
}
"#)]
extern "C" {
    fn glue() -> Option<String>;
    fn isolated() -> bool;
    #[wasm_bindgen(js_name = cores)]
    fn hardware_cores() -> f64;
    fn start(name: &str, module: JsValue, memory: JsValue, task: usize, failed: JsValue);
}

#[wasm_bindgen]
pub fn citrine_thread_run(task: usize) {
    let task = unsafe { Box::from_raw(task as *mut Task) };
    task();
}

pub(crate) struct Handle(Arc<AtomicBool>);

impl Handle {
    pub(crate) fn is_finished(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    pub(crate) fn join(self) {}
}

pub(crate) fn thread(
    name: String,
    body: impl FnOnce() + Send + 'static,
    failed: impl FnOnce(String) + 'static,
) -> Handle {
    let finished = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&finished);
    let task: Box<Task> = Box::new(Box::new(move || {
        body();
        done.store(true, Ordering::Release);
    }));
    let lost = Arc::clone(&finished);
    let failed = Closure::once_into_js(move |text: String| {
        lost.store(true, Ordering::Release);
        failed(text);
    });
    start(
        &name,
        wasm_bindgen::module(),
        wasm_bindgen::memory(),
        Box::into_raw(task) as usize,
        failed,
    );
    Handle(finished)
}

pub(crate) fn supported() -> bool {
    isolated() && glue().is_some()
}

pub(crate) fn cores() -> usize {
    (hardware_cores() as usize).max(1)
}

pub(crate) fn missing() -> Option<crate::Unthreaded> {
    if !isolated() {
        Some(crate::Unthreaded::NotIsolated)
    } else if glue().is_none() {
        Some(crate::Unthreaded::NoThreadBuild)
    } else {
        None
    }
}
