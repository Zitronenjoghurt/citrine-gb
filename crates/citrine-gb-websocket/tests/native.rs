#![cfg(not(target_arch = "wasm32"))]
#![expect(
    clippy::disallowed_methods,
    reason = "each test runs a small WebSocket server on its own thread"
)]

use citrine_gb_websocket::{Event, Message, connect};
use std::net::TcpListener;
use std::sync::mpsc::{Receiver, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(5);

fn server(
    handle: impl FnOnce(tungstenite::WebSocket<std::net::TcpStream>) + Send + 'static,
) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        tcp.set_nodelay(true).unwrap();
        handle(tungstenite::accept(tcp).unwrap());
    });
    (url, thread)
}

fn echo(mut ws: tungstenite::WebSocket<std::net::TcpStream>) {
    loop {
        match ws.read() {
            Ok(msg @ (tungstenite::Message::Binary(_) | tungstenite::Message::Text(_))) => {
                ws.send(msg).unwrap();
            }
            Ok(_) => {}
            Err(_) => return,
        }
    }
}

fn open(url: &str) -> (citrine_gb_websocket::Sender, Receiver<(Instant, Event)>) {
    let (tx, rx) = channel();
    let sender = connect(url, 1 << 20, move |event| {
        let _ = tx.send((Instant::now(), event));
    })
    .unwrap();
    (sender, rx)
}

fn next(rx: &Receiver<(Instant, Event)>) -> Event {
    rx.recv_timeout(WAIT).unwrap().1
}

#[test]
fn messages_come_back_after_opened() {
    let (url, _server) = server(echo);
    let (mut sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    sender.send(Message::Binary(vec![1, 2, 3]));
    sender.send(Message::Text("lemon".into()));
    assert_eq!(next(&rx), Event::Message(Message::Binary(vec![1, 2, 3])));
    assert_eq!(next(&rx), Event::Message(Message::Text("lemon".into())));
}

#[test]
fn messages_sent_before_the_open_go_out_in_order() {
    let (url, _server) = server(echo);
    let (mut sender, rx) = open(&url);
    for i in 0..5u8 {
        sender.send(Message::Binary(vec![i]));
    }
    assert_eq!(next(&rx), Event::Opened);
    for i in 0..5u8 {
        assert_eq!(next(&rx), Event::Message(Message::Binary(vec![i])));
    }
}

#[test]
fn a_send_goes_out_without_waiting_for_a_read() {
    let (url, _server) = server(echo);
    let (mut sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    let mut trips = Vec::new();
    for i in 0..30u8 {
        std::thread::sleep(Duration::from_millis(7));
        let sent = Instant::now();
        sender.send(Message::Binary(vec![i]));
        let (at, event) = rx.recv_timeout(WAIT).unwrap();
        assert_eq!(event, Event::Message(Message::Binary(vec![i])));
        trips.push(at - sent);
    }
    trips.sort();
    assert!(trips[15] < Duration::from_millis(2), "{trips:?}");
}

#[test]
fn dropping_the_sender_closes_the_socket() {
    let (closed_tx, closed_rx) = channel();
    let (url, _server) = server(move |mut ws| {
        loop {
            match ws.read() {
                Ok(tungstenite::Message::Close(_)) => {
                    let _ = closed_tx.send(true);
                    return;
                }
                Ok(_) => {}
                Err(_) => {
                    let _ = closed_tx.send(false);
                    return;
                }
            }
        }
    });
    let (sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    drop(sender);
    assert!(closed_rx.recv_timeout(WAIT).unwrap());
    assert!(rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn messages_sent_right_before_the_drop_still_go_out() {
    let (heard_tx, heard_rx) = channel();
    let (url, _server) = server(move |mut ws| {
        let mut heard = Vec::new();
        loop {
            match ws.read() {
                Ok(tungstenite::Message::Text(text)) => heard.push(text.to_string()),
                Ok(tungstenite::Message::Close(_)) | Err(_) => {
                    let _ = heard_tx.send(heard);
                    return;
                }
                Ok(_) => {}
            }
        }
    });
    let (mut sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    sender.send(Message::Binary(vec![7; 512 * 1024]));
    sender.send(Message::Text("bye".into()));
    drop(sender);
    assert_eq!(
        heard_rx.recv_timeout(WAIT).unwrap(),
        vec!["bye".to_string()]
    );
}

#[test]
fn sends_and_the_drop_return_while_the_server_reads_nothing() {
    let (done_tx, done_rx) = channel::<()>();
    let (url, _server) = server(move |ws| {
        let _ = done_rx.recv_timeout(WAIT * 4);
        drop(ws);
    });
    let (mut sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    let mut worst = Duration::ZERO;
    for _ in 0..64 {
        let started = Instant::now();
        sender.send(Message::Binary(vec![7; 512 * 1024]));
        worst = worst.max(started.elapsed());
    }
    let started = Instant::now();
    drop(sender);
    let dropping = started.elapsed();
    let _ = done_tx.send(());
    assert!(worst < Duration::from_millis(50), "a send took {worst:?}");
    assert!(
        dropping < Duration::from_millis(50),
        "the drop took {dropping:?}"
    );
}

#[test]
fn a_server_close_reports_closed() {
    let (url, _server) = server(|mut ws| {
        ws.close(None).unwrap();
        while ws.read().is_ok() {}
    });
    let (_sender, rx) = open(&url);
    assert_eq!(next(&rx), Event::Opened);
    assert_eq!(next(&rx), Event::Closed);
}

#[test]
fn a_refused_connect_reports_failed() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/", listener.local_addr().unwrap());
    drop(listener);
    let (_sender, rx) = open(&url);
    assert!(matches!(next(&rx), Event::Failed(_)));
}

#[test]
fn tls_has_a_crypto_provider_without_other_crates() {
    let _ = rustls::ClientConfig::builder();
}
