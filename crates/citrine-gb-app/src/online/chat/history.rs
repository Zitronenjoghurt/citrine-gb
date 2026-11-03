use super::{Change, ChatEntry, Conversation, Delivery};
use citrine_gb_net::{Body, OnlineId, pack, unpack};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};

const MAX_HISTORY_BYTES: usize = 256 << 20;
const MARK: &[u8] = b"citrine chats 3\n";
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
struct SecondConversation {
    entries: Vec<ChatEntry>,
    unread: usize,
}

impl From<SecondConversation> for Conversation {
    fn from(old: SecondConversation) -> Self {
        Self {
            entries: old.entries,
            unread: old.unread,
            relayed: BTreeMap::new(),
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
                })
                .collect(),
            unread: old.unread,
            relayed: BTreeMap::new(),
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
    if let Some(second) = plain.strip_prefix(SECOND_MARK) {
        let second: BTreeMap<OnlineId, SecondConversation> = postcard::from_bytes(second).ok()?;
        return Some(second.into_iter().map(|(id, c)| (id, c.into())).collect());
    }
    let first: BTreeMap<OnlineId, FirstConversation> = postcard::from_bytes(&plain).ok()?;
    Some(first.into_iter().map(|(id, c)| (id, c.into())).collect())
}

pub fn encode_history(history: &History) -> Option<Vec<u8>> {
    let mut plain = MARK.to_vec();
    plain.extend(postcard::to_allocvec(history).ok()?);
    Some(pack(&plain, true))
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

    #[derive(Serialize)]
    struct SecondConversationOut {
        entries: Vec<ChatEntry>,
        unread: usize,
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
        };
        let old = BTreeMap::from([(
            friend,
            SecondConversationOut {
                entries: vec![entry.clone()],
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
    fn a_merge_keeps_every_message_and_its_relay_id() {
        let friend = OnlineId::new(7).unwrap();
        let entry = |id| ChatEntry {
            id,
            mine: true,
            at: id,
            body: Body::Text("x".into()),
            delivery: Delivery::Sent,
            change: Change::Kept,
        };
        let total = 5000;
        let mut into = History::from([(
            friend,
            Conversation {
                entries: (1..=10).map(entry).collect(),
                unread: 0,
                relayed: BTreeMap::new(),
            },
        )]);
        let from = History::from([(
            friend,
            Conversation {
                entries: (1..=total).map(entry).collect(),
                unread: 0,
                relayed: (1..=total).map(|id| (id, id + 1000)).collect(),
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
            },
        )]);
        let back = decode_history(&encode_history(&history).unwrap()).unwrap();
        assert_eq!(back, history);
    }
}
