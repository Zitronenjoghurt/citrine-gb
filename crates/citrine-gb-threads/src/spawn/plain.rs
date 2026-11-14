use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen(inline_js = r#"
export function isolated() {
  return globalThis.crossOriginIsolated === true;
}
"#)]
extern "C" {
    fn isolated() -> bool;
}

pub(crate) struct Handle;

impl Handle {
    pub(crate) fn is_finished(&self) -> bool {
        true
    }

    pub(crate) fn join(self) {}
}

pub(crate) fn thread(
    _name: String,
    _body: impl FnOnce() + Send + 'static,
    _failed: impl FnOnce(String) + 'static,
) -> Handle {
    unreachable!("a build without atomics runs every worker inline")
}

pub(crate) fn supported() -> bool {
    false
}

pub(crate) fn cores() -> usize {
    1
}

pub(crate) fn missing() -> Option<crate::Unthreaded> {
    if isolated() {
        Some(crate::Unthreaded::NoThreadBuild)
    } else {
        Some(crate::Unthreaded::NotIsolated)
    }
}
