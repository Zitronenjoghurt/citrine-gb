mod common;

use std::net::Ipv4Addr;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, Instant as Clock};

use citrine_gb_nds_net::gateway::{Config, Gateway, Resolver};
use citrine_gb_threads::Threads;
use common::Client;
use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::wire::{DnsQueryType, IpAddress};

const PCN: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 7);
const OTHER: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 9);
const UPSTREAM: Ipv4Addr = Ipv4Addr::new(192, 0, 2, 53);

fn fake_resolver(asked: Sender<String>) -> Resolver {
    Arc::new(move |name: &str| {
        asked.send(name.to_string()).ok();
        match name {
            "pkmnclassic.net" => vec![PCN],
            _ => vec![OTHER],
        }
    })
}

fn ask(server: Ipv4Addr, name: &str, wait: Duration) -> (Option<Vec<IpAddress>>, Vec<String>) {
    let (asks, asked) = mpsc::channel();
    let threads = Threads::default();
    let config = Config::default();
    let mut gateway = Gateway::new(config, threads.spawner()).with_resolver(fake_resolver(asks));
    let mut client = Client::new(config.address);
    let mut resolver = dns::Socket::new(&[IpAddress::Ipv4(server)], vec![]);
    let query = resolver
        .start_query(client.iface.context(), name, DnsQueryType::A)
        .expect("the query starts");
    let handle = client.sockets.add(resolver);
    let started = Clock::now();
    let found = loop {
        client.step(&mut gateway);
        match client
            .sockets
            .get_mut::<dns::Socket>(handle)
            .get_query_result(query)
        {
            Ok(addresses) => break Some(addresses.to_vec()),
            Err(GetQueryResultError::Pending) if started.elapsed() < wait => {
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(_) => break None,
        }
    };
    (found, asked.try_iter().collect())
}

#[test]
fn a_forwarded_lookup_of_an_aliased_name_answers_with_the_alias_address() {
    let (found, asked) = ask(
        UPSTREAM,
        "gamestats2.gs.nintendowifi.net",
        Duration::from_secs(5),
    );
    assert_eq!(found, Some(vec![IpAddress::Ipv4(PCN)]));
    assert_eq!(asked, vec!["pkmnclassic.net".to_string()]);
}

#[test]
fn the_gateway_resolver_follows_the_alias_and_keeps_other_names() {
    let gateway = Config::default().address;
    let (found, asked) = ask(
        gateway,
        "GameStats2.gs.nintendowifi.net",
        Duration::from_secs(5),
    );
    assert_eq!(found, Some(vec![IpAddress::Ipv4(PCN)]));
    assert_eq!(asked, vec!["pkmnclassic.net".to_string()]);
    let (found, asked) = ask(gateway, "nas.nintendowifi.net", Duration::from_secs(5));
    assert_eq!(found, Some(vec![IpAddress::Ipv4(OTHER)]));
    assert_eq!(asked, vec!["nas.nintendowifi.net".to_string()]);
}

#[test]
fn a_forwarded_lookup_of_another_name_goes_upstream() {
    let (found, asked) = ask(UPSTREAM, "nas.nintendowifi.net", Duration::from_millis(300));
    assert!(found.is_none_or(|addresses| !addresses.contains(&IpAddress::Ipv4(OTHER))));
    assert!(asked.is_empty());
}
