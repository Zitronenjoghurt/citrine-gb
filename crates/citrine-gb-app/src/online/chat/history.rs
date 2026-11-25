use super::{Change, ChatEntry, Conversation, Delivery};
use citrine_gb_net::{Body, MessageId, OnlineId, pack, unpack};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

const MAX_HISTORY_BYTES: usize = 256 << 20;
const MARK: &[u8] = b"citrine chats 4\n";
const THIRD_MARK: &[u8] = b"citrine chats 3\n";
const SECOND_MARK: &[u8] = b"citrine chats 2\n";

pub type History = BTreeMap<OnlineId, Conversation>;

#[derive(Deserialize)]
struct FirstEntry {
    id: u64,
    mine: bool,
    at: u64,
    body: Body,
    delivery: Delivery,
}

#[derive(Deserialize)]
struct FirstConversation {
    entries: Vec<FirstEntry>,
    unread: usize,
}

#[derive(Deserialize)]
struct ThirdEntry {
    id: u64,
    mine: bool,
    at: u64,
    body: Body,
    delivery: Delivery,
    change: Change,
}

impl From<ThirdEntry> for ChatEntry {
    fn from(old: ThirdEntry) -> Self {
        Self {
            id: old.id,
            mine: old.mine,
            at: old.at,
            body: old.body,
            delivery: old.delivery,
            change: old.change,
            reply: None,
        }
    }
}

#[derive(Deserialize)]
struct SecondConversation {
    entries: Vec<ThirdEntry>,
    unread: usize,
}

#[derive(Deserialize)]
struct ThirdConversation {
    entries: Vec<ThirdEntry>,
    unread: usize,
    relayed: BTreeMap<u64, MessageId>,
}

impl From<ThirdConversation> for Conversation {
    fn from(old: ThirdConversation) -> Self {
        Self {
            entries: old.entries.into_iter().map(ChatEntry::from).collect(),
            unread: old.unread,
            relayed: old.relayed,
            typing_text: String::new(),
        }
    }
}

impl From<SecondConversation> for Conversation {
    fn from(old: SecondConversation) -> Self {
        Self {
            entries: old.entries.into_iter().map(ChatEntry::from).collect(),
            unread: old.unread,
            relayed: BTreeMap::new(),
            typing_text: String::new(),
        }
    }
}

impl From<FirstConversation> for Conversation {
    fn from(old: FirstConversation) -> Self {
        Self {
            entries: old
                .entries
                .into_iter()
                .map(|e| ChatEntry {
                    id: e.id,
                    mine: e.mine,
                    at: e.at,
                    body: e.body,
                    delivery: e.delivery,
                    change: Change::Kept,
                    reply: None,
                })
                .collect(),
            unread: old.unread,
            relayed: BTreeMap::new(),
            typing_text: String::new(),
        }
    }
}

pub(super) fn history_name(owner: &str) -> String {
    let safe: String = owner
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("{safe}.bin")
}

pub(super) fn is_history_name(name: &str) -> bool {
    name.strip_suffix(".bin").is_some_and(|stem| {
        !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

pub fn decode_history(bytes: &[u8]) -> Option<History> {
    let plain = unpack(bytes, MAX_HISTORY_BYTES).ok()?;
    if let Some(current) = plain.strip_prefix(MARK) {
        return postcard::from_bytes(current).ok();
    }
    if let Some(third) = plain.strip_prefix(THIRD_MARK) {
        let third: BTreeMap<OnlineId, ThirdConversation> = postcard::from_bytes(third).ok()?;
        return Some(third.into_iter().map(|(id, c)| (id, c.into())).collect());
    }
    if let Some(second) = plain.strip_prefix(SECOND_MARK) {
        let second: BTreeMap<OnlineId, SecondConversation> = postcard::from_bytes(second).ok()?;
        return Some(second.into_iter().map(|(id, c)| (id, c.into())).collect());
    }
    let first: BTreeMap<OnlineId, FirstConversation> = postcard::from_bytes(&plain).ok()?;
    Some(first.into_iter().map(|(id, c)| (id, c.into())).collect())
}

pub(super) fn decode_kept(plain: &[u8]) -> Option<ChatEntry> {
    postcard::from_bytes::<ChatEntry>(plain).ok().or_else(|| {
        postcard::from_bytes::<ThirdEntry>(plain)
            .ok()
            .map(Into::into)
    })
}

pub fn encode_history(history: &History) -> Option<Vec<u8>> {
    let mut plain = MARK.to_vec();
    plain.extend(postcard::to_allocvec(history).ok()?);
    Some(pack(&plain, true))
}

fn entry_bytes(entry: &ChatEntry) -> usize {
    let body = match &entry.body {
        Body::Text(text) | Body::Palette(text) | Body::TypingText(text) => text.len(),
        Body::Share(share) => {
            share.preview.len()
                + share.game.len()
                + share.rom_sha256.as_ref().map_or(0, String::len)
        }
        Body::Receipt(receipt) => (receipt.delivered.len() + receipt.read.len()) * 8,
        Body::Edit(edit) => edit.text.len(),
        Body::Delete(_) => 0,
    };
    64 + body + entry.reply.as_ref().map_or(0, |quote| quote.line.len())
}

pub fn trim_history(history: &mut History, budget: usize) -> usize {
    let mut sizes: BTreeMap<OnlineId, usize> = history
        .iter()
        .map(|(friend, chat)| (*friend, chat.entries.iter().map(entry_bytes).sum()))
        .collect();
    let mut total: usize = sizes.values().sum();
    let mut dropped = 0;
    while total > budget {
        let Some((&friend, &size)) = sizes.iter().max_by_key(|(_, size)| **size) else {
            break;
        };
        let next = sizes
            .iter()
            .filter(|(other, _)| **other != friend)
            .map(|(_, size)| *size)
            .max()
            .unwrap_or(0);
        let Some(chat) = history.get_mut(&friend) else {
            break;
        };
        let mut freed = 0;
        let mut cut = 0;
        for entry in &chat.entries {
            if total - freed <= budget || (cut > 0 && size - freed < next) {
                break;
            }
            freed += entry_bytes(entry);
            cut += 1;
        }
        if cut == 0 {
            break;
        }
        chat.entries.drain(..cut);
        chat.unread = chat.unread.min(chat.entries.len());
        dropped += cut;
        total -= freed;
        sizes.insert(friend, size - freed);
    }
    dropped
}

pub fn merge_history(into: &mut History, from: &History) -> usize {
    let mut added = 0;
    for (friend, theirs) in from {
        let chat = into.entry(*friend).or_default();
        let mut known: HashSet<u64> = chat.entries.iter().map(|e| e.id).collect();
        for entry in &theirs.entries {
            if known.insert(entry.id) {
                chat.entries.push(entry.clone());
                added += 1;
            }
        }
        for (id, relay) in &theirs.relayed {
            chat.relayed.entry(*id).or_insert(*relay);
        }
        if chat.typing_text.is_empty() {
            chat.typing_text.clone_from(&theirs.typing_text);
        }
        chat.entries.sort_by_key(|e| e.at);
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct FirstEntryOut {
        id: u64,
        mine: bool,
        at: u64,
        body: Body,
        delivery: Delivery,
    }

    #[derive(Serialize)]
    struct FirstConversationOut {
        entries: Vec<FirstEntryOut>,
        unread: usize,
    }

    #[derive(Clone, Serialize)]
    struct ThirdEntryOut {
        id: u64,
        mine: bool,
        at: u64,
        body: Body,
        delivery: Delivery,
        change: Change,
    }

    #[derive(Serialize)]
    struct SecondConversationOut {
        entries: Vec<ThirdEntryOut>,
        unread: usize,
    }

    #[derive(Serialize)]
    struct ThirdConversationOut {
        entries: Vec<ThirdEntryOut>,
        unread: usize,
        relayed: BTreeMap<u64, MessageId>,
    }

    fn third_entry(entry: &ChatEntry) -> ThirdEntryOut {
        ThirdEntryOut {
            id: entry.id,
            mine: entry.mine,
            at: entry.at,
            body: entry.body.clone(),
            delivery: entry.delivery,
            change: entry.change,
        }
    }

    #[test]
    fn a_history_from_before_relay_ids_still_loads() {
        let friend = OnlineId::new(7).unwrap();
        let entry = ChatEntry {
            id: 3,
            mine: true,
            at: 100,
            body: Body::Text("hi".into()),
            delivery: Delivery::Read(120),
            change: Change::Edited { told: true },
            reply: None,
        };
        let old = BTreeMap::from([(
            friend,
            SecondConversationOut {
                entries: vec![third_entry(&entry)],
                unread: 1,
            },
        )]);
        let mut plain = SECOND_MARK.to_vec();
        plain.extend(postcard::to_allocvec(&old).unwrap());
        let history = decode_history(&pack(&plain, true)).unwrap();
        let chat = &history[&friend];
        assert_eq!(chat.entries, [entry]);
        assert_eq!(chat.unread, 1);
        assert!(chat.relayed.is_empty());
    }

    #[test]
    fn a_long_history_drops_the_oldest_messages_of_the_biggest_chat() {
        let (ana, ben) = (OnlineId::new(7).unwrap(), OnlineId::new(8).unwrap());
        let entry = |id, len| ChatEntry {
            id,
            mine: false,
            at: id,
            body: Body::Text("x".repeat(len)),
            delivery: Delivery::Received,
            change: Change::Kept,
            reply: None,
        };
        let chat = |entries: Vec<ChatEntry>| Conversation {
            unread: entries.len(),
            entries,
            relayed: BTreeMap::new(),
            typing_text: String::new(),
        };
        let mut history = History::from([
            (ana, chat((1..=4).map(|id| entry(id, 936)).collect())),
            (ben, chat(vec![entry(9, 36)])),
        ]);
        assert_eq!(trim_history(&mut history, 2200), 2);
        let ids: Vec<u64> = history[&ana].entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, [3, 4]);
        assert_eq!(history[&ana].unread, 2);
        assert_eq!(history[&ben].entries.len(), 1);
        assert_eq!(trim_history(&mut history, 2200), 0);
    }

    #[test]
    fn a_merge_keeps_every_message_and_its_relay_id() {
        let friend = OnlineId::new(7).unwrap();
        let entry = |id| ChatEntry {
            id,
            mine: true,
            at: id,
            body: Body::Text("x".into()),
            delivery: Delivery::Sent,
            change: Change::Kept,
            reply: None,
        };
        let total = 5000;
        let mut into = History::from([(
            friend,
            Conversation {
                entries: (1..=10).map(entry).collect(),
                unread: 0,
                relayed: BTreeMap::new(),
                typing_text: String::new(),
            },
        )]);
        let from = History::from([(
            friend,
            Conversation {
                entries: (1..=total).map(entry).collect(),
                unread: 0,
                relayed: (1..=total).map(|id| (id, id + 1000)).collect(),
                typing_text: String::new(),
            },
        )]);
        assert_eq!(merge_history(&mut into, &from), total as usize - 10);
        let chat = &into[&friend];
        assert_eq!(chat.entries.len(), total as usize);
        assert_eq!(chat.relayed.len(), total as usize);
        assert_eq!(chat.relayed.get(&1), Some(&1001));
    }

    #[test]
    fn a_history_from_before_edits_still_loads() {
        let friend = OnlineId::new(7).unwrap();
        let old = BTreeMap::from([(
            friend,
            FirstConversationOut {
                entries: vec![FirstEntryOut {
                    id: 3,
                    mine: true,
                    at: 100,
                    body: Body::Text("hi".into()),
                    delivery: Delivery::Sent,
                }],
                unread: 2,
            },
        )]);
        let bytes = pack(&postcard::to_allocvec(&old).unwrap(), true);
        let history = decode_history(&bytes).unwrap();
        let chat = &history[&friend];
        assert_eq!(chat.unread, 2);
        assert_eq!(chat.entries[0].body, Body::Text("hi".into()));
        assert_eq!(chat.entries[0].change, Change::Kept);
    }

    #[test]
    fn edits_and_deletes_survive_a_save() {
        let friend = OnlineId::new(7).unwrap();
        let entry = |id, change| ChatEntry {
            id,
            mine: false,
            at: id,
            body: Body::Text("x".into()),
            delivery: Delivery::Seen,
            change,
            reply: None,
        };
        let history = History::from([(
            friend,
            Conversation {
                entries: vec![
                    entry(1, Change::Edited { told: true }),
                    entry(2, Change::Deleted { told: false }),
                ],
                unread: 0,
                relayed: BTreeMap::from([(1, 40)]),
                typing_text: "is meowing".into(),
            },
        )]);
        let back = decode_history(&encode_history(&history).unwrap()).unwrap();
        assert_eq!(back, history);
    }

    #[test]
    fn a_history_from_before_replies_still_loads() {
        let friend = OnlineId::new(7).unwrap();
        let entry = ChatEntry {
            id: 3,
            mine: false,
            at: 100,
            body: Body::Text("hi".into()),
            delivery: Delivery::Seen,
            change: Change::Kept,
            reply: None,
        };
        let old = BTreeMap::from([(
            friend,
            ThirdConversationOut {
                entries: vec![third_entry(&entry)],
                unread: 1,
                relayed: BTreeMap::from([(3, 9)]),
            },
        )]);
        let mut plain = THIRD_MARK.to_vec();
        plain.extend(postcard::to_allocvec(&old).unwrap());
        let history = decode_history(&pack(&plain, true)).unwrap();
        let chat = &history[&friend];
        assert_eq!(chat.entries, [entry]);
        assert_eq!(chat.relayed.get(&3), Some(&9));
        assert_eq!(chat.typing_text, "");
    }

    #[test]
    fn a_reply_survives_a_save_and_kept_copies_of_both_ages_load() {
        let friend = OnlineId::new(7).unwrap();
        let reply = ChatEntry {
            id: 4,
            mine: true,
            at: 101,
            body: Body::Text("yes!".into()),
            delivery: Delivery::Sent,
            change: Change::Kept,
            reply: Some(citrine_gb_net::Quote {
                id: 3,
                by_sender: false,
                line: "did you win?".into(),
            }),
        };
        let history = History::from([(
            friend,
            Conversation {
                entries: vec![reply.clone()],
                ..Conversation::default()
            },
        )]);
        let saved = decode_history(&encode_history(&history).unwrap()).unwrap();
        assert_eq!(saved[&friend].entries, std::slice::from_ref(&reply));
        let kept = postcard::to_allocvec(&reply).unwrap();
        assert_eq!(decode_kept(&kept), Some(reply.clone()));
        let old = postcard::to_allocvec(&third_entry(&reply)).unwrap();
        assert_eq!(
            decode_kept(&old),
            Some(ChatEntry {
                reply: None,
                ..reply.clone()
            })
        );
        let shipped: ThirdEntryIn = postcard::from_bytes(&kept).unwrap();
        assert_eq!(shipped.body, Body::Text("yes!".into()));
    }

    #[derive(Deserialize)]
    struct ThirdEntryIn {
        #[allow(dead_code)]
        id: u64,
        #[allow(dead_code)]
        mine: bool,
        #[allow(dead_code)]
        at: u64,
        body: Body,
        #[allow(dead_code)]
        delivery: Delivery,
        #[allow(dead_code)]
        change: Change,
    }
}
