use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};

use super::{
    AddressBook, MAX_ENTRIES, MAX_FAILURES, MAX_PER_GROUP_PER_SOURCE, MAX_PER_SOURCE, PEERS_FILE,
    group_of, is_local,
};

fn peer() -> PeerId {
    Keypair::generate_ed25519().public().to_peer_id()
}

fn addr(text: &str, peer: PeerId) -> Multiaddr {
    text.parse::<Multiaddr>().unwrap().with(Protocol::P2p(peer))
}

const SEED: &str =
    "/dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws";

#[test]
fn groups_addresses_by_network() {
    let p = peer();
    assert_eq!(
        group_of(&addr("/ip4/203.0.113.7/tcp/28800", p)),
        "ip4:203.0"
    );
    assert_eq!(
        group_of(&addr("/ip6/2001:db8:1:2::5/tcp/28800", p)),
        "ip6:2001:db8"
    );
    assert_eq!(
        group_of(&addr(
            "/dns4/Demo-Seed-P2P.Zevrigo.Online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws",
            p
        )),
        "dns:zevrigo.online"
    );
    assert_eq!(
        group_of(&addr("/ip4/127.0.0.1/tcp/1", p)),
        "lokal:/ip4/127.0.0.1/tcp/1"
    );
    for local in [
        "10.1.2.3",
        "172.16.0.1",
        "192.168.0.1",
        "127.0.0.1",
        "169.254.1.1",
        "0.0.0.0",
        "::1",
        "::",
        "fd00::1",
        "fe80::1",
    ] {
        assert!(is_local(local.parse().unwrap()), "{local}");
    }
    for public in ["203.0.113.7", "8.8.8.8", "2001:db8::1"] {
        assert!(!is_local(public.parse().unwrap()), "{public}");
    }
}

#[test]
fn accepts_only_dialable_addresses_with_peer() {
    let p = peer();
    let public = AddressBook::new(false);
    let local = AddressBook::new(true);
    assert_eq!(
        public.acceptable(&addr("/ip4/203.0.113.7/tcp/28800", p)),
        Some(p)
    );
    assert_eq!(public.acceptable(&addr(SEED, p)), Some(p));
    assert_eq!(
        public.acceptable(&"/ip4/203.0.113.7/tcp/28800".parse().unwrap()),
        None
    );
    assert_eq!(public.acceptable(&addr("/ip4/203.0.113.7/udp/1", p)), None);
    let loopback = addr("/ip4/127.0.0.1/tcp/28800", p);
    assert_eq!(public.acceptable(&loopback), None);
    assert_eq!(
        public.acceptable(&addr("/ip4/192.168.1.5/tcp/28800", p)),
        None
    );
    assert_eq!(local.acceptable(&loopback), Some(p));
}

#[test]
fn one_source_cannot_fill_the_book() {
    let mut book = AddressBook::new(false);
    let p = peer();
    let source = "ip4:198.51";
    // Sembilan alamat di satu /16 dari satu sumber: hanya delapan yang masuk.
    let same_group = (0..9)
        .filter(|i| book.add_heard(&addr(&format!("/ip4/203.0.{i}.1/tcp/1"), p), source))
        .count();
    assert_eq!(same_group, MAX_PER_GROUP_PER_SOURCE);
    // Sumber yang sama di kelompok lain: paling banyak 64 alamat seluruhnya (10.1.0.1 privat, ditolak).
    let other_groups = (1..=100)
        .filter(|i| book.add_heard(&addr(&format!("/ip4/{i}.1.0.1/tcp/1"), p), source))
        .count();
    assert_eq!(other_groups, MAX_PER_SOURCE - MAX_PER_GROUP_PER_SOURCE);
    assert_eq!(book.len(), MAX_PER_SOURCE);
    // Sumber lain masih bisa menambah; alamat yang sudah ada tidak masuk dua kali.
    assert!(book.add_heard(&addr("/ip4/203.0.200.1/tcp/1", p), "ip4:192.0"));
    assert!(!book.add_heard(&addr("/ip4/203.0.0.1/tcp/1", p), "ip4:192.0"));
    assert_eq!(book.len(), MAX_PER_SOURCE + 1);
}

#[test]
fn full_book_evicts_untried_address() {
    let mut book = AddressBook::new(false);
    let p = peer();
    for i in 0..MAX_ENTRIES {
        let text = format!("/ip4/{}.{}.0.1/tcp/1", 20 + i / 200, i % 200);
        assert!(
            book.add_heard(&addr(&text, p), &format!("sumber-{i}")),
            "{text}"
        );
    }
    assert_eq!(book.len(), MAX_ENTRIES);
    let tried = addr("/ip4/20.0.0.1/tcp/1", p);
    assert!(book.mark_success(&tried));
    assert!(book.add_heard(&addr("/ip4/203.0.113.9/tcp/2", p), "sumber-baru"));
    assert_eq!(book.len(), MAX_ENTRIES);
    assert!(book.is_tried(&tried));
}

#[test]
fn failures_remove_address_and_success_resets() {
    let mut book = AddressBook::new(false);
    let a = addr("/ip4/203.0.113.7/tcp/28800", peer());
    assert!(book.add_heard(&a, "ip4:198.51"));
    assert!(!book.is_tried(&a));
    for _ in 1..MAX_FAILURES {
        assert!(book.mark_failure(&a));
    }
    assert!(book.mark_success(&a));
    assert!(book.is_tried(&a));
    for _ in 1..MAX_FAILURES {
        book.mark_failure(&a);
    }
    assert!(book.contains(&a));
    assert!(book.mark_failure(&a));
    assert!(!book.contains(&a));
    assert!(!book.mark_failure(&a));
    // Alamat yang belum ada (seed, --connect) langsung masuk tabel teruji saat berhasil didial.
    let seed = addr(SEED, peer());
    assert!(book.mark_success(&seed));
    assert!(book.is_tried(&seed));
    assert!(!book.mark_success(&seed));
    assert_eq!(book.tried_len(), 1);
    book.remove_peer(&crate::service::peer_of(&seed).unwrap());
    assert!(book.is_empty());
}

#[test]
fn pick_skips_busy_peers_groups_and_recent_attempts() {
    let mut book = AddressBook::new(false);
    let (p1, p2) = (peer(), peer());
    let a = addr("/ip4/203.0.113.7/tcp/1", p1);
    let b = addr("/ip4/198.51.100.7/tcp/1", p2);
    assert!(book.add_heard(&a, "x"));
    assert!(book.add_heard(&b, "x"));
    let now = Instant::now();
    let retry = Duration::from_secs(60);
    let no_peers: BTreeSet<PeerId> = BTreeSet::new();
    let no_groups: BTreeSet<String> = BTreeSet::new();
    let seen: BTreeSet<String> = (0..16)
        .filter_map(|random| book.pick(now, retry, &no_peers, &no_groups, random))
        .map(|address| address.to_string())
        .collect();
    assert_eq!(seen.len(), 2);
    let busy_peer = BTreeSet::from([p1]);
    let busy_group = BTreeSet::from(["ip4:198.51".to_owned()]);
    for random in 0..8 {
        assert_eq!(
            book.pick(now, retry, &busy_peer, &no_groups, random),
            Some(b.clone())
        );
        assert_eq!(
            book.pick(now, retry, &no_peers, &busy_group, random),
            Some(a.clone())
        );
    }
    book.mark_attempt(&a, now);
    for random in 0..8 {
        assert_eq!(
            book.pick(now, retry, &no_peers, &no_groups, random),
            Some(b.clone())
        );
    }
    assert_eq!(book.pick(now, retry, &no_peers, &busy_group, 0), None);
    assert_eq!(
        book.pick(now + retry, retry, &no_peers, &busy_group, 0),
        Some(a)
    );
    // Tabel teruji dipilih bila bit terendah 0.
    assert!(book.mark_success(&b));
    assert_eq!(
        book.pick(now + retry, retry, &no_peers, &no_groups, 0),
        Some(b)
    );
}

#[test]
fn shares_only_tried_and_round_trips_json() {
    let mut book = AddressBook::new(true);
    let (p1, p2) = (peer(), peer());
    let heard = addr("/ip4/127.0.0.1/tcp/1", p1);
    let tried = addr("/ip4/127.0.0.1/tcp/2", p2);
    assert!(book.add_heard(&heard, "x"));
    assert!(book.mark_success(&tried));
    assert!(book.mark_failure(&heard));
    assert_eq!(book.shareable(250, 7), vec![tried.clone()]);
    assert!(book.shareable(0, 7).is_empty());
    assert!(AddressBook::new(true).shareable(250, 7).is_empty());

    let text = book.to_json();
    assert!(text.contains("\"versi\": 1"), "{text}");
    let loaded = AddressBook::from_json(&text, true).unwrap();
    assert_eq!(loaded.peers(), BTreeSet::from([p1, p2]));
    assert_eq!(loaded.tried_peers(), BTreeSet::from([p2]));
    assert!(loaded.is_tried(&tried));
    assert!(AddressBook::from_json(&text, false).unwrap().is_empty());
    assert!(AddressBook::from_json("{\"versi\": 2, \"alamat\": []}", true).is_none());
    assert!(AddressBook::from_json("bukan json", true).is_none());

    let dir = std::env::temp_dir().join(format!("zvrg-addrbook-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(PEERS_FILE);
    book.save(&path).unwrap();
    assert_eq!(
        AddressBook::load(&path, true).peers(),
        BTreeSet::from([p1, p2])
    );
    std::fs::write(&path, "rusak").unwrap();
    assert!(AddressBook::load(&path, true).is_empty());
    assert!(AddressBook::load(&dir.join("tidak-ada.json"), true).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
