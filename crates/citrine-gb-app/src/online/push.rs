use super::Online;
use super::transport::SideLine;
use citrine_gb_net::{ClientMsg, PUSH_PROTOCOL, PushKinds};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Default)]
pub struct PushState {
    token: Option<String>,
    kinds: PushKinds,
    sent: Option<(Option<String>, PushKinds)>,
    told_away: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct StatusLine {
    line: SideLine,
    told_away: Arc<AtomicBool>,
}

impl StatusLine {
    pub fn away(&self) {
        if let Ok(bytes) = (ClientMsg::SetStatus { away: true }).encode() {
            (self.line)(bytes);
            self.told_away.store(true, Ordering::Relaxed);
        }
    }

    pub fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.line, &other.line)
    }
}

impl Online {
    pub fn set_push(&mut self, token: Option<&str>, kinds: PushKinds) {
        if self.push.token.as_deref() != token {
            self.push.token = token.map(str::to_owned);
        }
        self.push.kinds = kinds;
    }

    pub fn status_line(&self) -> Option<StatusLine> {
        if !self.is_connected() {
            return None;
        }
        Some(StatusLine {
            line: self.conn.as_ref()?.side_line()?,
            told_away: self.push.told_away.clone(),
        })
    }

    pub(super) fn took_away(&mut self) -> bool {
        self.push.told_away.swap(false, Ordering::Relaxed)
    }

    pub(super) fn push_reset(&mut self) {
        self.push.sent = None;
        self.push.told_away.store(false, Ordering::Relaxed);
    }

    pub(super) fn share_push(&mut self) {
        if !self.is_connected()
            || !self
                .info
                .as_ref()
                .is_some_and(|info| info.protocol >= PUSH_PROTOCOL)
        {
            return;
        }
        let PushState {
            token, kinds, sent, ..
        } = &self.push;
        let unchanged = match sent {
            Some((sent_token, sent_kinds)) => sent_token == token && sent_kinds == kinds,
            None => token.is_none(),
        };
        if unchanged {
            return;
        }
        let (token, kinds) = (token.clone(), *kinds);
        self.push.sent = Some((token.clone(), kinds));
        self.send(ClientMsg::SetPush { token, kinds });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::tests::{sent, signed_in};
    use citrine_gb_net::RelayId;

    fn pushes(msgs: Vec<ClientMsg>) -> Vec<ClientMsg> {
        msgs.into_iter()
            .filter(|m| matches!(m, ClientMsg::SetPush { .. }))
            .collect()
    }

    #[test]
    fn the_token_goes_up_once_and_again_when_it_changes() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        online.update(now);
        assert!(pushes(sent(&wire)).is_empty(), "no token, nothing to send");
        online.set_push(Some("first"), PushKinds::ALL);
        online.update(now);
        online.update(now);
        assert_eq!(
            pushes(sent(&wire)),
            vec![ClientMsg::SetPush {
                token: Some("first".into()),
                kinds: PushKinds::ALL,
            }]
        );
        let chats = PushKinds {
            chats: true,
            ..PushKinds::default()
        };
        online.set_push(Some("second"), chats);
        online.update(now);
        assert_eq!(
            pushes(sent(&wire)),
            vec![ClientMsg::SetPush {
                token: Some("second".into()),
                kinds: chats,
            }]
        );
        online.set_push(None, chats);
        online.update(now);
        assert_eq!(
            pushes(sent(&wire)),
            vec![ClientMsg::SetPush {
                token: None,
                kinds: chats,
            }]
        );
    }

    #[test]
    fn an_older_server_never_hears_of_push() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        if let Some(info) = online.info.as_mut() {
            info.protocol = PUSH_PROTOCOL - 1;
        }
        online.set_push(Some("token"), PushKinds::ALL);
        online.update(now);
        assert!(pushes(sent(&wire)).is_empty());
    }

    #[test]
    fn a_new_connection_sends_the_token_again() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        online.set_push(Some("token"), PushKinds::ALL);
        online.update(now);
        assert_eq!(pushes(sent(&wire)).len(), 1);
        online.push_reset();
        online.update(now);
        assert_eq!(pushes(sent(&wire)).len(), 1);
    }

    #[test]
    fn going_to_the_background_says_away_at_once_and_back_says_here() {
        let (mut online, wire, now) = signed_in(&RelayId::random());
        let (side, heard) = std::sync::mpsc::channel();
        wire.borrow_mut().side = Some(side);
        online.update(now);
        sent(&wire);
        let line = online.status_line().expect("a connected line");
        line.away();
        assert_eq!(
            heard.try_iter().collect::<Vec<_>>(),
            vec![ClientMsg::SetStatus { away: true }]
        );
        online.set_activity(true, None);
        online.update(now);
        assert!(
            !sent(&wire).contains(&ClientMsg::SetStatus { away: true }),
            "the server already heard it"
        );
        online.set_activity(false, None);
        online.update(now);
        assert!(sent(&wire).contains(&ClientMsg::SetStatus { away: false }));
    }

    #[test]
    fn no_line_while_signed_out() {
        let (mut online, _wire, _now) = signed_in(&RelayId::random());
        online.leave();
        assert!(online.status_line().is_none());
    }
}
