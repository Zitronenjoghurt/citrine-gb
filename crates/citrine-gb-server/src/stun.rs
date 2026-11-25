use crate::App;
use citrine_gb_net::stun_answer;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

pub(crate) async fn bind(ip: IpAddr, port: u16) -> Option<UdpSocket> {
    if port == 0 {
        return None;
    }
    match UdpSocket::bind((ip, port)).await {
        Ok(socket) => Some(socket),
        Err(e) => {
            log::warn!(
                "STUN can't listen on UDP port {port}: {e}. Players can't connect straight to each other and stay on the relay. Free the port or set CITRINE_STUN_PORT"
            );
            None
        }
    }
}

pub(crate) fn serve(socket: UdpSocket, app: Arc<App>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut buf = [0; 576];
        loop {
            let Ok((n, from)) = socket.recv_from(&mut buf).await else {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            };
            if !app.settings.get().relay {
                continue;
            }
            if let Some(answer) = stun_answer(&buf[..n], from) {
                let _ = socket.send_to(&answer, from).await;
            }
        }
    })
}
