use citrine_gb_server::{Config, Running, start};
use citrine_gb_surface::pairing::Code;
use citrine_gb_surface::relay::{
    ACCOUNT_DOORS_PER_ADDRESS, Control, DOORS_PER_ADDRESS, DoorKey, PATH, WRONG_CODES_PER_MINUTE,
};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

const PASSWORD: &str = "lemon tree";

async fn server(dir: &Path) -> Running {
    let map: HashMap<String, String> = HashMap::from([
        ("CITRINE_BIND".into(), "127.0.0.1".into()),
        ("CITRINE_PORT".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_MB".into(), "0".into()),
        ("CITRINE_DISK_FLOOR_PERCENT".into(), "0".into()),
        ("CITRINE_IP_CONNECTIONS".into(), "100000".into()),
        ("CITRINE_IP_UNSIGNED".into(), "100000".into()),
        ("CITRINE_IP_CONNECTS_PER_MINUTE".into(), "100000".into()),
        (
            "CITRINE_DATA_DIR".into(),
            dir.join("data").display().to_string(),
        ),
        (
            "CITRINE_STATIC_DIR".into(),
            dir.join("static").display().to_string(),
        ),
        ("CITRINE_RELAY".into(), "on".into()),
        ("CITRINE_RELAY_PASSWORD".into(), PASSWORD.into()),
    ]);
    start(Config::from_vars(|k| map.get(k).cloned()).unwrap())
        .await
        .unwrap()
}

async fn dial(addr: SocketAddr) -> Ws {
    tokio_tungstenite::connect_async(format!("ws://{addr}{PATH}"))
        .await
        .unwrap()
        .0
}

async fn say(ws: &mut Ws, control: Control) {
    ws.send(Message::Text(control.text().into())).await.unwrap();
}

async fn next(ws: &mut Ws) -> Option<Message> {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .ok()??
            .ok()?;
        match message {
            Message::Ping(_) | Message::Pong(_) => {}
            other => return Some(other),
        }
    }
}

async fn heard(ws: &mut Ws) -> Option<Control> {
    match next(ws).await? {
        Message::Text(text) => Control::parse(&text),
        _ => None,
    }
}

async fn open(addr: SocketAddr, code: Code) -> (Ws, Option<Control>) {
    let mut host = dial(addr).await;
    say(
        &mut host,
        Control::Open {
            code,
            password: Some(PASSWORD.into()),
        },
    )
    .await;
    let reply = heard(&mut host).await;
    (host, reply)
}

fn code(text: &str) -> Code {
    Code::parse(text).unwrap()
}

#[tokio::test]
async fn a_code_bridges_a_host_and_a_surface() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let (mut host, reply) = open(running.addr, code("482913")).await;
    assert_eq!(reply, Some(Control::Opened));
    let mut phone = dial(running.addr).await;
    say(&mut phone, Control::Join(code("482913"))).await;
    assert_eq!(heard(&mut phone).await, Some(Control::Joined));
    assert_eq!(heard(&mut host).await, Some(Control::Joined));
    phone
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut host).await,
        Some(Message::Binary(vec![1, 2, 3].into()))
    );
    host.send(Message::Binary(vec![9; 70_000].into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut phone).await,
        Some(Message::Binary(vec![9; 70_000].into()))
    );
    let mut second = dial(running.addr).await;
    say(&mut second, Control::Join(code("482913"))).await;
    assert_eq!(heard(&mut second).await, Some(Control::Busy));
    drop(phone);
    assert_eq!(heard(&mut host).await, Some(Control::Gone));
    let mut again = dial(running.addr).await;
    say(&mut again, Control::Join(code("482913"))).await;
    assert_eq!(heard(&mut again).await, Some(Control::Joined));
    assert_eq!(heard(&mut host).await, Some(Control::Joined));
    drop(host);
    assert_eq!(heard(&mut again).await, Some(Control::Gone));
}

#[tokio::test]
async fn a_taken_code_and_a_wrong_password_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let (_host, reply) = open(running.addr, code("111111")).await;
    assert_eq!(reply, Some(Control::Opened));
    let (_other, reply) = open(running.addr, code("111111")).await;
    assert_eq!(reply, Some(Control::Taken));
    let mut stranger = dial(running.addr).await;
    say(
        &mut stranger,
        Control::Open {
            code: code("222222"),
            password: Some("wrong".into()),
        },
    )
    .await;
    assert_eq!(heard(&mut stranger).await, Some(Control::Limit));
}

#[tokio::test]
async fn wrong_codes_run_out() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let (_host, _) = open(running.addr, code("333333")).await;
    for _ in 0..WRONG_CODES_PER_MINUTE {
        let mut guess = dial(running.addr).await;
        say(&mut guess, Control::Join(code("444444"))).await;
        assert_eq!(heard(&mut guess).await, Some(Control::NoCode));
    }
    let mut guess = dial(running.addr).await;
    say(&mut guess, Control::Join(code("333333"))).await;
    assert_eq!(heard(&mut guess).await, Some(Control::Limit));
}

async fn open_account(addr: SocketAddr, key: DoorKey) -> (Ws, Option<Control>) {
    let mut host = dial(addr).await;
    say(
        &mut host,
        Control::Account {
            key,
            password: Some(PASSWORD.into()),
        },
    )
    .await;
    let reply = heard(&mut host).await;
    (host, reply)
}

async fn look(addr: SocketAddr, keys: Vec<DoorKey>) -> Option<Control> {
    let mut phone = dial(addr).await;
    say(&mut phone, Control::Look(keys)).await;
    heard(&mut phone).await
}

#[tokio::test]
async fn an_account_door_bridges_a_device_of_the_same_account() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let mac = DoorKey([1; 16]);
    let (mut host, reply) = open_account(running.addr, mac).await;
    assert_eq!(reply, Some(Control::Opened));
    let mut phone = dial(running.addr).await;
    say(&mut phone, Control::Dial(mac)).await;
    assert_eq!(heard(&mut phone).await, Some(Control::Joined));
    assert_eq!(heard(&mut host).await, Some(Control::Joined));
    phone
        .send(Message::Binary(vec![4, 5].into()))
        .await
        .unwrap();
    assert_eq!(
        next(&mut host).await,
        Some(Message::Binary(vec![4, 5].into()))
    );
    host.send(Message::Binary(vec![6].into())).await.unwrap();
    assert_eq!(
        next(&mut phone).await,
        Some(Message::Binary(vec![6].into()))
    );
    drop(phone);
    assert_eq!(heard(&mut host).await, Some(Control::Gone));
}

#[tokio::test]
async fn a_look_names_only_open_free_account_doors() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let (mac, pc, tablet) = (DoorKey([1; 16]), DoorKey([2; 16]), DoorKey([3; 16]));
    let (_mac_host, _) = open_account(running.addr, mac).await;
    let (mut pc_host, _) = open_account(running.addr, pc).await;
    assert_eq!(
        look(running.addr, vec![mac, pc, tablet]).await,
        Some(Control::Here(vec![mac, pc]))
    );
    let mut phone = dial(running.addr).await;
    say(&mut phone, Control::Dial(pc)).await;
    assert_eq!(heard(&mut phone).await, Some(Control::Joined));
    assert_eq!(heard(&mut pc_host).await, Some(Control::Joined));
    assert_eq!(
        look(running.addr, vec![mac, pc, tablet]).await,
        Some(Control::Here(vec![mac]))
    );
    let (_code_host, reply) = open(running.addr, code("555555")).await;
    assert_eq!(reply, Some(Control::Opened));
    assert_eq!(
        look(running.addr, Vec::new()).await,
        Some(Control::Here(Vec::new()))
    );
}

#[tokio::test]
async fn account_doors_have_their_own_limit_and_need_the_password() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let mut codes = Vec::new();
    for at in 0..DOORS_PER_ADDRESS {
        let (host, reply) = open(running.addr, code(&format!("70000{at}"))).await;
        assert_eq!(reply, Some(Control::Opened));
        codes.push(host);
    }
    let mut accounts = Vec::new();
    for at in 0..ACCOUNT_DOORS_PER_ADDRESS {
        let (host, reply) = open_account(running.addr, DoorKey([at as u8; 16])).await;
        assert_eq!(reply, Some(Control::Opened), "door {at}");
        accounts.push(host);
    }
    let (_, reply) = open_account(running.addr, DoorKey([0xee; 16])).await;
    assert_eq!(reply, Some(Control::Limit));
    let (_, reply) = open_account(running.addr, DoorKey([0; 16])).await;
    assert_eq!(reply, Some(Control::Limit));
    let mut stranger = dial(running.addr).await;
    say(
        &mut stranger,
        Control::Account {
            key: DoorKey([0xdd; 16]),
            password: Some("wrong".into()),
        },
    )
    .await;
    assert_eq!(heard(&mut stranger).await, Some(Control::Limit));
}

#[tokio::test]
async fn a_dial_to_no_door_counts_as_a_wrong_code() {
    let dir = tempfile::tempdir().unwrap();
    let running = server(dir.path()).await;
    let (_host, _) = open_account(running.addr, DoorKey([1; 16])).await;
    for _ in 0..WRONG_CODES_PER_MINUTE {
        let mut guess = dial(running.addr).await;
        say(&mut guess, Control::Dial(DoorKey([2; 16]))).await;
        assert_eq!(heard(&mut guess).await, Some(Control::NoCode));
    }
    let mut guess = dial(running.addr).await;
    say(&mut guess, Control::Dial(DoorKey([1; 16]))).await;
    assert_eq!(heard(&mut guess).await, Some(Control::Limit));
}
