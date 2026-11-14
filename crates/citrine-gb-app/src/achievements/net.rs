use citrine_gb_achievements::{Request, Response};
use std::sync::mpsc::{Receiver, channel};

pub trait Net {
    fn send(&mut self, request: Request, user_agent: &str) -> Receiver<Response>;
}

pub struct Offline;

impl Net for Offline {
    fn send(&mut self, _request: Request, _user_agent: &str) -> Receiver<Response> {
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
    fn send(&mut self, request: Request, user_agent: &str) -> Receiver<Response> {
        let (answer, reply) = channel();
        start(&self.spawner, request, user_agent.to_owned(), answer);
        reply
    }
}

#[cfg(not(test))]
fn start(
    spawner: &citrine_gb_threads::Spawner,
    request: Request,
    user_agent: String,
    answer: std::sync::mpsc::Sender<Response>,
) {
    const MOST: u64 = 8 << 20;
    spawner.run("achievements request", move || {
        let read = |mut response: ureq::http::Response<ureq::Body>| {
            let status = i32::from(response.status().as_u16());
            let body = response
                .body_mut()
                .with_config()
                .limit(MOST)
                .read_to_vec()
                .unwrap_or_default();
            Response { status, body }
        };
        let result = match request.post_data {
            Some(post) => ureq::post(&request.url)
                .header("User-Agent", &user_agent)
                .header(
                    "Content-Type",
                    request
                        .content_type
                        .as_deref()
                        .unwrap_or("application/x-www-form-urlencoded"),
                )
                .config()
                .http_status_as_error(false)
                .build()
                .send(post.as_bytes()),
            None => ureq::get(&request.url)
                .header("User-Agent", &user_agent)
                .config()
                .http_status_as_error(false)
                .build()
                .call(),
        };
        let response = match result {
            Ok(response) => read(response),
            Err(err) => {
                log::warn!("achievements request failed: {err}");
                Response::network_error()
            }
        };
        let _ = answer.send(response);
    });
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "the fake server and the test that reads what it was sent share one list"
)]
pub(crate) mod fake {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    pub(crate) struct Fake {
        answers: Arc<Mutex<Vec<(String, Response)>>>,
        pub(crate) sent: Arc<Mutex<Vec<(Request, String)>>>,
    }

    impl Fake {
        pub(crate) fn answer(&self, post_starts: &str, status: i32, body: &str) {
            self.answers.lock().unwrap().push((
                post_starts.to_owned(),
                Response {
                    status,
                    body: body.as_bytes().to_vec(),
                },
            ));
        }
    }

    impl Net for Fake {
        fn send(&mut self, request: Request, user_agent: &str) -> Receiver<Response> {
            let (answer, reply) = channel();
            let post = request.post_data.clone().unwrap_or_default();
            let found = self
                .answers
                .lock()
                .unwrap()
                .iter()
                .find(|(starts, _)| post.starts_with(starts.as_str()))
                .map(|(_, response)| response.clone());
            self.sent
                .lock()
                .unwrap()
                .push((request, user_agent.to_owned()));
            if let Some(response) = found {
                let _ = answer.send(response);
            }
            reply
        }
    }
}
