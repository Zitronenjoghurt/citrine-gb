use crate::{Event, Message};
use std::ops::ControlFlow;

pub struct Sender(ewebsock::WsSender);

impl Sender {
    pub fn send(&mut self, message: Message) {
        self.0.send(match message {
            Message::Binary(bytes) => ewebsock::WsMessage::Binary(bytes),
            Message::Text(text) => ewebsock::WsMessage::Text(text),
        });
    }
}

pub fn connect(
    url: &str,
    max_frame_bytes: usize,
    on_event: impl Fn(Event) + Send + Sync + 'static,
) -> Result<Sender, String> {
    let options = ewebsock::Options {
        max_incoming_frame_size: max_frame_bytes,
        ..Default::default()
    };
    let on_event = Box::new(move |event| {
        let event = match event {
            ewebsock::WsEvent::Opened => Event::Opened,
            ewebsock::WsEvent::Message(ewebsock::WsMessage::Binary(bytes)) => {
                Event::Message(Message::Binary(bytes))
            }
            ewebsock::WsEvent::Message(ewebsock::WsMessage::Text(text)) => {
                Event::Message(Message::Text(text))
            }
            ewebsock::WsEvent::Message(_) => return ControlFlow::Continue(()),
            ewebsock::WsEvent::Error(err) => Event::Failed(err),
            ewebsock::WsEvent::Closed => Event::Closed,
        };
        on_event(event);
        ControlFlow::Continue(())
    });
    ewebsock::ws_connect(url.to_owned(), options, on_event).map(Sender)
}
