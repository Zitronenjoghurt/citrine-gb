use super::{SIGN_IN_TIMEOUT, info};
use crate::App;
use axum::extract::ws::{Message, WebSocket};
use citrine_gb_net::{
    Channel, ClientMsg, ErrorCode, PROTOCOL_VERSION, ServerMsg, accept, is_secure_start,
};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};

pub(super) async fn handshake(
    app: &App,
    sink: &mut SplitSink<WebSocket, Message>,
    stream: &mut SplitStream<WebSocket>,
) -> Option<Channel> {
    let first = loop {
        match tokio::time::timeout(SIGN_IN_TIMEOUT, stream.next()).await {
            Ok(Some(Ok(Message::Binary(bytes)))) => break bytes,
            Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => continue,
            _ => return None,
        }
    };
    app.stats.received(first.len());
    if is_secure_start(&first) {
        let (reply, channel) = accept(&app.relay_secret, &first).ok()?;
        app.stats.sent(reply.len());
        sink.send(Message::Binary(reply.into())).await.ok()?;
        return Some(channel);
    }
    if let Ok(ClientMsg::Hello { .. }) = ClientMsg::decode(&first) {
        let too_old = ServerMsg::Error(ErrorCode::Protocol {
            server: PROTOCOL_VERSION,
        });
        for msg in [ServerMsg::Info(info(app)), too_old] {
            if let Ok(bytes) = msg.encode() {
                let _ = sink.send(Message::Binary(bytes.into())).await;
            }
        }
    }
    let _ = sink.send(Message::Close(None)).await;
    None
}
