use std::sync::mpsc::{Receiver, Sender, channel};

pub type Answer = Result<Vec<u8>, String>;

pub trait Net {
    fn get(&mut self, url: String) -> Receiver<Answer>;
}

pub struct Offline;

impl Net for Offline {
    fn get(&mut self, _url: String) -> Receiver<Answer> {
        channel().1
    }
}

#[cfg(test)]
pub fn platform(_spawner: &citrine_gb_threads::Spawner) -> Box<dyn Net> {
    Box::new(Offline)
}

#[cfg(not(test))]
pub fn platform(spawner: &citrine_gb_threads::Spawner) -> Box<dyn Net> {
    Box::new(Platform {
        spawner: spawner.clone(),
    })
}

#[cfg(not(test))]
struct Platform {
    spawner: citrine_gb_threads::Spawner,
}

#[cfg(not(test))]
impl Net for Platform {
    fn get(&mut self, url: String) -> Receiver<Answer> {
        let (answer, reply) = channel();
        start(&self.spawner, url, answer);
        reply
    }
}

#[cfg(all(not(test), not(target_arch = "wasm32")))]
fn start(spawner: &citrine_gb_threads::Spawner, url: String, answer: Sender<Answer>) {
    const MOST: u64 = 16 << 20;
    spawner.run("cover download", move || {
        let result = ureq::get(&url)
            .header("User-Agent", concat!("Citrine/", env!("CARGO_PKG_VERSION")))
            .call()
            .and_then(|mut response| response.body_mut().with_config().limit(MOST).read_to_vec())
            .map_err(|err| err.to_string());
        let _ = answer.send(result);
    });
}

#[cfg(all(not(test), target_arch = "wasm32"))]
fn start(_spawner: &citrine_gb_threads::Spawner, url: String, answer: Sender<Answer>) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    async fn read(url: &str) -> Result<Vec<u8>, wasm_bindgen::JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let response: web_sys::Response = JsFuture::from(window.fetch_with_str(url))
            .await?
            .dyn_into()?;
        if !response.ok() {
            return Err(wasm_bindgen::JsValue::from(response.status()));
        }
        let body = JsFuture::from(response.array_buffer()?).await?;
        Ok(js_sys::Uint8Array::new(&body).to_vec())
    }

    citrine_gb_threads::spawn_local(async move {
        let result = read(&url).await.map_err(|err| format!("{err:?}"));
        let _ = answer.send(result);
    });
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    type Asked = Vec<(String, Sender<Answer>)>;

    #[derive(Clone, Default)]
    pub(crate) struct Fake {
        asked: Rc<RefCell<Asked>>,
    }

    impl Net for Fake {
        fn get(&mut self, url: String) -> Receiver<Answer> {
            let (answer, reply) = channel();
            self.asked.borrow_mut().push((url, answer));
            reply
        }
    }

    impl Fake {
        pub(crate) fn urls(&self) -> Vec<String> {
            self.asked
                .borrow()
                .iter()
                .map(|(url, _)| url.clone())
                .collect()
        }

        pub(crate) fn fail(&self, err: &str) {
            for (_, answer) in std::mem::take(&mut *self.asked.borrow_mut()) {
                let _ = answer.send(Err(err.to_owned()));
            }
        }

        pub(crate) fn answer(&self, files: &BTreeMap<String, Vec<u8>>) -> usize {
            let asked = std::mem::take(&mut *self.asked.borrow_mut());
            let count = asked.len();
            for (url, answer) in asked {
                let result = files
                    .get(&url)
                    .cloned()
                    .ok_or_else(|| "http status: 404".to_owned());
                let _ = answer.send(result);
            }
            count
        }
    }
}
