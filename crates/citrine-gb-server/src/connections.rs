use crate::config::Limits;
use crate::limit::Bucket;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const PRUNE_AT: usize = 4096;

pub fn address_key(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
        },
        v4 => v4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Full,
    Address,
    Blocked,
}

struct Address {
    open: usize,
    unsigned: usize,
    connects: Bucket,
}

#[derive(Default)]
struct State {
    open: usize,
    addresses: HashMap<IpAddr, Address>,
    blocked: HashMap<IpAddr, Instant>,
}

pub struct Connections {
    max: usize,
    per_address: usize,
    unsigned_per_address: usize,
    connects_per_minute: f64,
    state: Mutex<State>,
}

pub struct Slot {
    connections: Arc<Connections>,
    key: IpAddr,
    signed_in: bool,
}

impl Connections {
    pub fn new(limits: &Limits) -> Self {
        Self {
            max: limits.max_connections,
            per_address: limits.ip_connections,
            unsigned_per_address: limits.ip_unsigned,
            connects_per_minute: limits.ip_connects_per_minute as f64,
            state: Mutex::default(),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn admit(self: &Arc<Self>, ip: IpAddr) -> Result<Slot, Refusal> {
        let key = address_key(ip);
        let mut state = self.state();
        let now = Instant::now();
        state.blocked.retain(|_, until| *until > now);
        if state.blocked.contains_key(&key) {
            return Err(Refusal::Blocked);
        }
        if state.open >= self.max {
            return Err(Refusal::Full);
        }
        if state.addresses.len() >= PRUNE_AT {
            state
                .addresses
                .retain(|_, a| a.open > 0 || !a.connects.full());
        }
        let per_minute = self.connects_per_minute;
        let address = state.addresses.entry(key).or_insert_with(|| Address {
            open: 0,
            unsigned: 0,
            connects: Bucket::new(per_minute / 60.0, per_minute),
        });
        if address.open >= self.per_address
            || address.unsigned >= self.unsigned_per_address
            || !address.connects.take(1.0)
        {
            return Err(Refusal::Address);
        }
        address.open += 1;
        address.unsigned += 1;
        state.open += 1;
        Ok(Slot {
            connections: self.clone(),
            key,
            signed_in: false,
        })
    }
}

impl Connections {
    pub fn block(&self, ip: IpAddr, how_long: Duration) -> IpAddr {
        let key = address_key(ip);
        self.state().blocked.insert(key, Instant::now() + how_long);
        key
    }

    pub fn unblock(&self, ip: IpAddr) -> bool {
        self.state().blocked.remove(&address_key(ip)).is_some()
    }

    pub fn blocked(&self) -> Vec<(IpAddr, Duration)> {
        let now = Instant::now();
        let mut state = self.state();
        state.blocked.retain(|_, until| *until > now);
        let mut list: Vec<(IpAddr, Duration)> = state
            .blocked
            .iter()
            .map(|(ip, until)| (*ip, until.saturating_duration_since(now)))
            .collect();
        list.sort_by_key(|(ip, left)| (std::cmp::Reverse(*left), *ip));
        list
    }
}

pub fn address_text(key: IpAddr) -> String {
    match key {
        IpAddr::V6(_) => format!("{key}/64"),
        IpAddr::V4(_) => key.to_string(),
    }
}

impl Slot {
    pub fn signed_in(&mut self) {
        if self.signed_in {
            return;
        }
        self.signed_in = true;
        if let Some(address) = self.connections.state().addresses.get_mut(&self.key) {
            address.unsigned = address.unsigned.saturating_sub(1);
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut state = self.connections.state();
        state.open = state.open.saturating_sub(1);
        if let Some(address) = state.addresses.get_mut(&self.key) {
            address.open = address.open.saturating_sub(1);
            if !self.signed_in {
                address.unsigned = address.unsigned.saturating_sub(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connections(pairs: &[(&str, &str)]) -> Arc<Connections> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let limits = Limits::from_vars(|name| map.get(name).cloned()).unwrap();
        Arc::new(Connections::new(&limits))
    }

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([10, 0, 0, last])
    }

    #[test]
    fn an_address_gets_a_few_unsigned_and_more_signed_connections() {
        let c = connections(&[]);
        let mut slots: Vec<Slot> = (0..3).map(|_| c.admit(ip(1)).unwrap()).collect();
        assert_eq!(c.admit(ip(1)).err(), Some(Refusal::Address));
        assert!(c.admit(ip(2)).is_ok());
        for slot in &mut slots {
            slot.signed_in();
        }
        for _ in 0..3 {
            let mut slot = c.admit(ip(1)).unwrap();
            slot.signed_in();
            slots.push(slot);
        }
        slots.extend((0..3).map(|_| c.admit(ip(1)).unwrap()));
        assert_eq!(slots.len(), 9);
        assert_eq!(c.admit(ip(1)).err(), Some(Refusal::Address));
        slots[8].signed_in();
        let mut tenth = c.admit(ip(1)).unwrap();
        tenth.signed_in();
        slots.push(tenth);
        assert_eq!(c.admit(ip(1)).err(), Some(Refusal::Address));
        slots.pop();
        assert!(c.admit(ip(1)).is_ok());
    }

    #[test]
    fn the_whole_relay_has_a_cap() {
        let c = connections(&[("CITRINE_MAX_CONNECTIONS", "2")]);
        let _a = c.admit(ip(1)).unwrap();
        let b = c.admit(ip(2)).unwrap();
        assert_eq!(c.admit(ip(3)).err(), Some(Refusal::Full));
        drop(b);
        let _c = c.admit(ip(3)).unwrap();
        assert_eq!(c.admit(ip(4)).err(), Some(Refusal::Full));
    }

    #[test]
    fn new_connections_per_minute_are_capped() {
        let c = connections(&[("CITRINE_IP_CONNECTS_PER_MINUTE", "3")]);
        for _ in 0..3 {
            drop(c.admit(ip(1)).unwrap());
        }
        assert_eq!(c.admit(ip(1)).err(), Some(Refusal::Address));
        assert!(c.admit(ip(2)).is_ok());
    }

    #[test]
    fn a_blocked_address_is_refused_until_unblocked() {
        let c = connections(&[]);
        let v6: IpAddr = "2001:db8:1:2::5".parse().unwrap();
        assert_eq!(c.block(v6, Duration::from_secs(60)), address_key(v6));
        c.block(ip(1), Duration::from_secs(60));
        assert_eq!(c.admit(ip(1)).err(), Some(Refusal::Blocked));
        let neighbour: IpAddr = "2001:db8:1:2::9".parse().unwrap();
        assert_eq!(c.admit(neighbour).err(), Some(Refusal::Blocked));
        assert!(c.admit(ip(2)).is_ok());
        assert_eq!(c.blocked().len(), 2);
        assert!(c.unblock(ip(1)));
        assert!(c.admit(ip(1)).is_ok());
        c.block(ip(3), Duration::ZERO);
        assert!(c.admit(ip(3)).is_ok());
        assert_eq!(address_text(address_key(v6)), "2001:db8:1:2::/64");
    }

    #[test]
    fn ipv6_counts_by_its_64_prefix() {
        let a: IpAddr = "2001:db8:1:2:aaaa::1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:bbbb::2".parse().unwrap();
        let other: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(address_key(a), address_key(b));
        assert_ne!(address_key(a), address_key(other));
        let mapped: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
        assert_eq!(address_key(mapped), ip(1));
    }
}
