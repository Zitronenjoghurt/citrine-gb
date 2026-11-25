use crate::{CONNECT_WAIT, Event, Note, Stun};
use citrine_gb_threads::Spawner;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcDataChannelType, RtcIceCandidateInit, RtcIceConnectionState,
    RtcIceServer, RtcPeerConnection, RtcPeerConnectionIceEvent, RtcSdpType,
    RtcSessionDescriptionInit,
};

const LABEL: &str = "citrine";

type Queue = Rc<RefCell<Vec<Event>>>;
type Hooks = Rc<RefCell<Vec<Closure<dyn FnMut(JsValue)>>>>;

struct Peer {
    pc: RtcPeerConnection,
    channel: Rc<RefCell<Option<RtcDataChannel>>>,
    remote_set: Rc<Cell<bool>>,
    waiting: Rc<RefCell<Vec<String>>>,
    up: Rc<Cell<bool>>,
    opened: Rc<Cell<bool>>,
    made: f64,
    hooks: Hooks,
}

impl Drop for Peer {
    fn drop(&mut self) {
        self.pc.set_onicecandidate(None);
        self.pc.set_oniceconnectionstatechange(None);
        self.pc.set_ondatachannel(None);
        if let Some(channel) = self.channel.borrow().as_ref() {
            channel.set_onopen(None);
            channel.set_onmessage(None);
            channel.set_onclose(None);
        }
        self.pc.close();
    }
}

#[derive(Default)]
pub struct Direct {
    stun: Option<Stun>,
    peers: HashMap<u64, Peer>,
    queue: Queue,
}

impl Direct {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn give_spawner(&mut self, _spawner: impl FnOnce() -> Spawner) {}

    pub fn set_stun(&mut self, stun: Option<Stun>) {
        self.stun = stun;
    }

    pub fn offer(&mut self, peer: u64) {
        self.drop_peer(peer);
        let Some(made) = self.peer(peer) else {
            return;
        };
        let init = RtcDataChannelInit::new();
        init.set_ordered(false);
        init.set_max_retransmits(0);
        let channel = made
            .pc
            .create_data_channel_with_data_channel_dict(LABEL, &init);
        let hooks = hook_channel(peer, &channel, (&made.up, &made.opened), &self.queue);
        made.hooks.borrow_mut().extend(hooks);
        *made.channel.borrow_mut() = Some(channel);
        let (pc, queue) = (made.pc.clone(), self.queue.clone());
        citrine_gb_threads::spawn_local(async move {
            let Ok(offer) = JsFuture::from(pc.create_offer()).await else {
                return;
            };
            let Some(sdp) = sdp_of(&offer) else {
                return;
            };
            let local = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
            local.set_sdp(&sdp);
            if JsFuture::from(pc.set_local_description(&local))
                .await
                .is_ok()
            {
                queue.borrow_mut().push(Event::Note(peer, Note::Offer(sdp)));
            }
        });
        self.peers.insert(peer, made);
    }

    pub fn hear(&mut self, peer: u64, note: Note) {
        match note {
            Note::Offer(sdp) => self.answer(peer, sdp),
            Note::Answer(sdp) => {
                let Some(made) = self.peers.get(&peer) else {
                    return;
                };
                let remote = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                remote.set_sdp(&sdp);
                let (pc, remote_set, waiting) = (
                    made.pc.clone(),
                    made.remote_set.clone(),
                    made.waiting.clone(),
                );
                citrine_gb_threads::spawn_local(async move {
                    if JsFuture::from(pc.set_remote_description(&remote))
                        .await
                        .is_ok()
                    {
                        remote_set.set(true);
                        add_candidates(&pc, waiting.take());
                    }
                });
            }
            Note::Candidate(line) => {
                let Some(made) = self.peers.get(&peer) else {
                    return;
                };
                if made.remote_set.get() {
                    add_candidates(&made.pc, vec![line]);
                } else {
                    made.waiting.borrow_mut().push(line);
                }
            }
        }
    }

    pub fn send(&mut self, peer: u64, frame: Vec<u8>) {
        let Some(made) = self.peers.get(&peer) else {
            return;
        };
        if let Some(channel) = made.channel.borrow().as_ref()
            && channel.ready_state() == RtcDataChannelState::Open
        {
            let _ = channel.send_with_u8_array(&frame);
        }
    }

    pub fn close(&mut self, peer: u64) {
        self.peers.remove(&peer);
    }

    pub fn cut(&self, _cut: bool) {}

    pub fn deafen(&self, _deaf: bool) {}

    pub fn poll(&mut self) -> Vec<Event> {
        let now = js_sys::Date::now();
        let late: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, made)| {
                !made.opened.get() && now - made.made >= CONNECT_WAIT.as_millis() as f64
            })
            .map(|(id, _)| *id)
            .collect();
        for id in late {
            self.peers.remove(&id);
            self.queue.borrow_mut().push(Event::Down(id));
        }
        self.queue.take()
    }

    fn answer(&mut self, peer: u64, sdp: String) {
        self.drop_peer(peer);
        let Some(made) = self.peer(peer) else {
            return;
        };
        let (channel, up, opened, queue, hooks) = (
            made.channel.clone(),
            made.up.clone(),
            made.opened.clone(),
            self.queue.clone(),
            Rc::downgrade(&made.hooks),
        );
        let on_channel = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
            let Ok(event) = event.dyn_into::<RtcDataChannelEvent>() else {
                return;
            };
            let Some(hooks) = hooks.upgrade() else {
                return;
            };
            let incoming = event.channel();
            let made = hook_channel(peer, &incoming, (&up, &opened), &queue);
            hooks.borrow_mut().extend(made);
            *channel.borrow_mut() = Some(incoming);
        });
        made.pc
            .set_ondatachannel(Some(on_channel.as_ref().unchecked_ref()));
        made.hooks.borrow_mut().push(on_channel);
        let remote = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        remote.set_sdp(&sdp);
        let (pc, queue, remote_set, waiting) = (
            made.pc.clone(),
            self.queue.clone(),
            made.remote_set.clone(),
            made.waiting.clone(),
        );
        citrine_gb_threads::spawn_local(async move {
            if JsFuture::from(pc.set_remote_description(&remote))
                .await
                .is_err()
            {
                return;
            }
            remote_set.set(true);
            add_candidates(&pc, waiting.take());
            let Ok(answer) = JsFuture::from(pc.create_answer()).await else {
                return;
            };
            let Some(sdp) = sdp_of(&answer) else {
                return;
            };
            let local = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
            local.set_sdp(&sdp);
            if JsFuture::from(pc.set_local_description(&local))
                .await
                .is_ok()
            {
                queue
                    .borrow_mut()
                    .push(Event::Note(peer, Note::Answer(sdp)));
            }
        });
        self.peers.insert(peer, made);
    }

    fn peer(&self, peer: u64) -> Option<Peer> {
        let config = RtcConfiguration::new();
        if let Some(stun) = &self.stun {
            let server = RtcIceServer::new();
            let host = match stun.host.contains(':') && !stun.host.starts_with('[') {
                true => format!("[{}]", stun.host),
                false => stun.host.clone(),
            };
            server.set_urls(&JsValue::from_str(&format!("stun:{host}:{}", stun.port)));
            config.set_ice_servers(&js_sys::Array::of1(&server));
        }
        let pc = RtcPeerConnection::new_with_configuration(&config).ok()?;
        let queue = self.queue.clone();
        let on_candidate = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
            let Ok(event) = event.dyn_into::<RtcPeerConnectionIceEvent>() else {
                return;
            };
            if let Some(candidate) = event.candidate()
                && !candidate.candidate().is_empty()
            {
                queue
                    .borrow_mut()
                    .push(Event::Note(peer, Note::Candidate(candidate.candidate())));
            }
        });
        pc.set_onicecandidate(Some(on_candidate.as_ref().unchecked_ref()));
        let up = Rc::new(Cell::new(false));
        let (state_pc, state_up, queue) = (pc.clone(), up.clone(), self.queue.clone());
        let on_state = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
            if matches!(
                state_pc.ice_connection_state(),
                RtcIceConnectionState::Failed
                    | RtcIceConnectionState::Disconnected
                    | RtcIceConnectionState::Closed
            ) && state_up.replace(false)
            {
                queue.borrow_mut().push(Event::Down(peer));
            }
        });
        pc.set_oniceconnectionstatechange(Some(on_state.as_ref().unchecked_ref()));
        Some(Peer {
            pc,
            channel: Rc::new(RefCell::new(None)),
            remote_set: Rc::new(Cell::new(false)),
            waiting: Rc::new(RefCell::new(Vec::new())),
            up,
            opened: Rc::new(Cell::new(false)),
            made: js_sys::Date::now(),
            hooks: Rc::new(RefCell::new(vec![on_candidate, on_state])),
        })
    }

    fn drop_peer(&mut self, peer: u64) {
        if let Some(made) = self.peers.remove(&peer)
            && made.up.get()
        {
            self.queue.borrow_mut().push(Event::Down(peer));
        }
    }
}

fn hook_channel(
    peer: u64,
    channel: &RtcDataChannel,
    (up, opened): (&Rc<Cell<bool>>, &Rc<Cell<bool>>),
    queue: &Queue,
) -> Vec<Closure<dyn FnMut(JsValue)>> {
    channel.set_binary_type(RtcDataChannelType::Arraybuffer);
    let (open_up, open_queue, opened) = (up.clone(), queue.clone(), opened.clone());
    let on_open = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
        opened.set(true);
        if !open_up.replace(true) {
            open_queue.borrow_mut().push(Event::Up(peer));
        }
    });
    let message_queue = queue.clone();
    let on_message = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let Ok(event) = event.dyn_into::<MessageEvent>() else {
            return;
        };
        let Ok(buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() else {
            return;
        };
        let frame = js_sys::Uint8Array::new(&buffer).to_vec();
        message_queue.borrow_mut().push(Event::Frame(peer, frame));
    });
    let (close_up, close_queue) = (up.clone(), queue.clone());
    let on_close = Closure::<dyn FnMut(JsValue)>::new(move |_: JsValue| {
        if close_up.replace(false) {
            close_queue.borrow_mut().push(Event::Down(peer));
        }
    });
    channel.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    channel.set_onclose(Some(on_close.as_ref().unchecked_ref()));
    vec![on_open, on_message, on_close]
}

fn add_candidates(pc: &RtcPeerConnection, lines: Vec<String>) {
    for line in lines {
        let init = RtcIceCandidateInit::new(&line);
        init.set_sdp_m_line_index(Some(0));
        let promise = pc.add_ice_candidate_with_opt_rtc_ice_candidate_init(Some(&init));
        citrine_gb_threads::spawn_local(async move {
            let _ = JsFuture::from(promise).await;
        });
    }
}

fn sdp_of(description: &JsValue) -> Option<String> {
    js_sys::Reflect::get(description, &JsValue::from_str("sdp"))
        .ok()?
        .as_string()
}
