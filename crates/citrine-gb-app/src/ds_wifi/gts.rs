use crate::online::Online;
use citrine_gb_nds_net::services::GtsLink;
use citrine_gb_net::{ClientMsg, ErrorCode, GtsReply, ServerMsg};
use std::sync::mpsc::{Receiver, Sender, channel};

type Answer = (u64, Option<Vec<u8>>);

pub struct GtsPort {
    calls: Receiver<(u64, String)>,
    answers: Sender<Answer>,
    waiting: Vec<u64>,
}

pub struct RelayLink {
    calls: Sender<(u64, String)>,
    answers: Receiver<Answer>,
}

pub fn pair() -> (GtsPort, RelayLink) {
    let (call_tx, call_rx) = channel();
    let (answer_tx, answer_rx) = channel();
    (
        GtsPort {
            calls: call_rx,
            answers: answer_tx,
            waiting: Vec::new(),
        },
        RelayLink {
            calls: call_tx,
            answers: answer_rx,
        },
    )
}

impl GtsLink for RelayLink {
    fn call(&mut self, id: u64, target: &str) {
        self.calls.send((id, target.to_owned())).ok();
    }

    fn answer(&mut self) -> Option<Answer> {
        self.answers.try_recv().ok()
    }
}

impl GtsPort {
    pub fn pump(&mut self, online: &mut Online) {
        let mut refused = false;
        for msg in online.inbox() {
            match msg {
                ServerMsg::Gts { call, reply } => {
                    if let Some(at) = self.waiting.iter().position(|id| id == call) {
                        self.waiting.swap_remove(at);
                        let body = match reply {
                            GtsReply::Body(body) => Some(body.clone()),
                            GtsReply::Status(_) => None,
                        };
                        self.answers.send((*call, body)).ok();
                    }
                }
                ServerMsg::Error(ErrorCode::GtsOff) => refused = true,
                _ => {}
            }
        }
        if refused || !online.is_connected() {
            for id in self.waiting.drain(..) {
                self.answers.send((id, None)).ok();
            }
        }
        while let Ok((call, target)) = self.calls.try_recv() {
            if online.is_connected()
                && online.account().is_some()
                && online.send(ClientMsg::Gts { call, target })
            {
                self.waiting.push(call);
            } else {
                self.answers.send((call, None)).ok();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_without_a_relay_is_answered_at_once() {
        let (mut port, mut link) = pair();
        link.call(7, "/pokemondpds/worldexchange/info.asp?pid=1");
        port.pump(&mut Online::offline());
        assert_eq!(link.answer(), Some((7, None)));
        assert_eq!(link.answer(), None);
    }
}
