use std::path::PathBuf;
use std::time::Duration;

use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use zevrigo_primitives::{Block, BlockHeight, Hash32};

use super::{NetEvent, Network, NetworkConfig, Offense, ip_of, peer_of, strip_p2p};
use crate::addrbook::AddressBook;
use crate::message::{Ack, Blocks, GetBlocks, GetHeaders, Headers, Status};
use crate::test_support::{id, sample_block, sample_tx};
use crate::transport::is_websocket;

const NET_A: Hash32 = id(0xa1);
const NET_B: Hash32 = id(0xb2);

fn status(network_id: Hash32, height: u64) -> Status {
    Status::new(network_id, BlockHeight::new(height), id(5), [0; 32])
}

fn node(network_id: Hash32, connect: Vec<Multiaddr>, height: u64) -> Network {
    let mut config = NetworkConfig::new(network_id);
    config.listen = vec!["/ip4/127.0.0.1/tcp/0".parse().unwrap()];
    config.connect = connect;
    config.redial_interval = Duration::from_millis(200);
    Network::new(
        Keypair::generate_ed25519(),
        config,
        status(network_id, height),
    )
    .unwrap()
}

/// Menunggu event pertama yang cocok (paling lama 20 detik); event lain dibuang.
async fn next_matching<F>(net: &mut Network, mut wanted: F) -> NetEvent
where
    F: FnMut(&NetEvent) -> bool,
{
    let wait = async {
        loop {
            let event = net.next_event().await;
            if wanted(&event) {
                return event;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(20), wait)
        .await
        .unwrap()
}

/// Alamat dengar lengkap dengan `/p2p/<PeerId>`.
async fn dial_address(net: &mut Network) -> Multiaddr {
    let event = next_matching(net, |event| matches!(event, NetEvent::Listening { .. })).await;
    let NetEvent::Listening { address } = event else {
        unreachable!()
    };
    address.with(Protocol::P2p(net.local_peer_id()))
}

/// Rekan uji: menjawab semua permintaan. Jika `short_blocks`, jawaban `Blocks` kurang satu entri.
async fn serve(mut net: Network, short_blocks: bool) {
    loop {
        match net.next_event().await {
            NetEvent::GetHeaders { reply, .. } => {
                let headers = Headers {
                    headers: vec![*sample_block(1).header()],
                };
                net.reply_headers(reply, headers);
            }
            NetEvent::GetBlocks { request, reply, .. } => {
                let mut entries: Vec<Option<Block>> = request.ids.iter().map(|_| None).collect();
                if let Some(first) = entries.first_mut() {
                    *first = Some(sample_block(1));
                }
                if short_blocks {
                    entries.pop();
                }
                net.reply_blocks(reply, Blocks { entries });
            }
            NetEvent::Block { reply, .. } => {
                net.reply_ack(reply, Ack::Accepted);
            }
            NetEvent::Tx { reply, .. } => {
                net.reply_ack(reply, Ack::Rejected);
            }
            _ => {}
        }
    }
}

async fn wait_status(net: &mut Network, from: PeerId) -> Status {
    let event = next_matching(
        net,
        |event| matches!(event, NetEvent::PeerStatus { peer, .. } if *peer == from),
    )
    .await;
    let NetEvent::PeerStatus { status, .. } = event else {
        unreachable!()
    };
    status
}

#[test]
fn address_helpers_find_ip_and_peer() {
    let peer = Keypair::generate_ed25519().public().to_peer_id();
    let plain: Multiaddr = "/ip4/10.1.2.3/tcp/28800".parse().unwrap();
    let full = plain.clone().with(Protocol::P2p(peer));
    assert_eq!(ip_of(&full), Some("10.1.2.3".parse().unwrap()));
    assert_eq!(peer_of(&full), Some(peer));
    assert_eq!(peer_of(&plain), None);
    assert_eq!(strip_p2p(&full), plain);
    let v6: Multiaddr = "/ip6/::1/tcp/1".parse().unwrap();
    assert_eq!(ip_of(&v6), Some("::1".parse().unwrap()));
}

#[tokio::test]
async fn peers_exchange_status_headers_blocks_and_acks() {
    let mut a = node(NET_A, Vec::new(), 7);
    let a_id = a.local_peer_id();
    let a_addr = dial_address(&mut a).await;
    tokio::spawn(serve(a, false));

    let mut b = node(NET_A, vec![a_addr], 0);
    assert_eq!(wait_status(&mut b, a_id).await, status(NET_A, 7));
    assert_eq!(b.connected_peers(), vec![a_id]);

    let sent = b.request_headers(
        &a_id,
        GetHeaders {
            locator: vec![id(1)],
            stop: Hash32::default(),
        },
    );
    let event = next_matching(&mut b, |event| matches!(event, NetEvent::Headers { .. })).await;
    let NetEvent::Headers {
        peer,
        request,
        headers,
    } = event
    else {
        unreachable!()
    };
    assert_eq!((peer, request), (a_id, sent));
    assert_eq!(headers.headers, vec![*sample_block(1).header()]);

    let sent = b.request_blocks(
        &a_id,
        GetBlocks {
            ids: vec![id(1), id(2)],
        },
    );
    let event = next_matching(&mut b, |event| matches!(event, NetEvent::Blocks { .. })).await;
    let NetEvent::Blocks {
        request, blocks, ..
    } = event
    else {
        unreachable!()
    };
    assert_eq!(request, sent);
    assert_eq!(blocks.entries, vec![Some(sample_block(1)), None]);

    b.send_block(&a_id, sample_block(2));
    let event = next_matching(&mut b, |event| matches!(event, NetEvent::BlockAck { .. })).await;
    assert!(matches!(
        event,
        NetEvent::BlockAck {
            ack: Ack::Accepted,
            ..
        }
    ));

    b.send_tx(&a_id, sample_tx());
    let event = next_matching(&mut b, |event| matches!(event, NetEvent::TxAck { .. })).await;
    assert!(matches!(
        event,
        NetEvent::TxAck {
            ack: Ack::Rejected,
            ..
        }
    ));
    assert!(!b.is_banned(&a_id));
}

#[tokio::test]
async fn other_network_is_disconnected_without_ban() {
    let mut a = node(NET_A, Vec::new(), 1);
    let a_id = a.local_peer_id();
    let a_addr = dial_address(&mut a).await;
    tokio::spawn(serve(a, false));

    let mut b = node(NET_B, vec![a_addr], 0);
    let event = next_matching(&mut b, |event| {
        matches!(
            event,
            NetEvent::PeerDisconnected { .. }
                | NetEvent::PeerStatus { .. }
                | NetEvent::PeerPunished { .. }
        )
    })
    .await;
    assert!(matches!(event, NetEvent::PeerDisconnected { peer } if peer == a_id));
    assert!(!b.is_banned(&a_id));
}

#[tokio::test]
async fn wrong_blocks_entry_count_from_inbound_peer_is_punished() {
    // `a` menyambung ke `b`, jadi bagi `b` rekan `a` bukan rekan operator: blokir 24 jam berlaku.
    let mut b = node(NET_A, Vec::new(), 0);
    let b_addr = dial_address(&mut b).await;
    let a = node(NET_A, vec![b_addr], 3);
    let a_id = a.local_peer_id();
    tokio::spawn(serve(a, true));

    wait_status(&mut b, a_id).await;
    b.request_blocks(
        &a_id,
        GetBlocks {
            ids: vec![id(1), id(2)],
        },
    );
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerPunished { .. })
    })
    .await;
    let NetEvent::PeerPunished {
        peer,
        offense,
        banned,
    } = event
    else {
        unreachable!()
    };
    assert_eq!(peer, a_id);
    assert_eq!(
        offense,
        Offense::WrongEntryCount {
            expected: 2,
            actual: 1
        }
    );
    assert!(banned);
    assert!(b.is_banned(&a_id));
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerDisconnected { .. })
    })
    .await;
    assert!(matches!(event, NetEvent::PeerDisconnected { peer } if peer == a_id));
    assert!(b.connected_peers().is_empty());
}

#[tokio::test]
async fn operator_peer_is_disconnected_without_ban_and_redialed() {
    // DEC-41: `a` ada di `--connect` milik `b` (seperti seed resmi), jadi pelanggaran hanya memutus koneksi.
    let mut a = node(NET_A, Vec::new(), 3);
    let a_id = a.local_peer_id();
    let a_addr = dial_address(&mut a).await;
    tokio::spawn(serve(a, true));

    let mut b = node(NET_A, vec![a_addr], 0);
    wait_status(&mut b, a_id).await;
    b.request_blocks(
        &a_id,
        GetBlocks {
            ids: vec![id(1), id(2)],
        },
    );
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerPunished { .. })
    })
    .await;
    let NetEvent::PeerPunished {
        peer,
        offense,
        banned,
    } = event
    else {
        unreachable!()
    };
    assert_eq!(peer, a_id);
    assert_eq!(
        offense,
        Offense::WrongEntryCount {
            expected: 2,
            actual: 1
        }
    );
    assert!(!banned);
    assert!(!b.is_banned(&a_id));
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerDisconnected { .. })
    })
    .await;
    assert!(matches!(event, NetEvent::PeerDisconnected { peer } if peer == a_id));
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerConnected { .. })
    })
    .await;
    assert!(matches!(event, NetEvent::PeerConnected { peer, .. } if peer == a_id));
    assert_eq!(wait_status(&mut b, a_id).await, status(NET_A, 3));
    assert!(!b.is_banned(&a_id));
}

#[tokio::test]
async fn dialer_reconnects_after_disconnect() {
    let mut a = node(NET_A, Vec::new(), 1);
    let a_id = a.local_peer_id();
    let a_addr = dial_address(&mut a).await;
    tokio::spawn(serve(a, false));

    let mut b = node(NET_A, vec![a_addr], 0);
    wait_status(&mut b, a_id).await;
    b.disconnect(&a_id);
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerDisconnected { .. })
    })
    .await;
    assert!(matches!(event, NetEvent::PeerDisconnected { peer } if peer == a_id));
    let event = next_matching(&mut b, |event| {
        matches!(event, NetEvent::PeerConnected { .. })
    })
    .await;
    assert!(matches!(event, NetEvent::PeerConnected { peer, .. } if peer == a_id));
    assert_eq!(wait_status(&mut b, a_id).await, status(NET_A, 1));
}

/// Port TCP di `address`.
fn tcp_port(address: &Multiaddr) -> u16 {
    address
        .iter()
        .find_map(|part| match part {
            Protocol::Tcp(port) => Some(port),
            _ => None,
        })
        .unwrap()
}

#[tokio::test]
async fn websocket_and_dns_addresses_connect() {
    let mut config = NetworkConfig::new(NET_A);
    config.listen = vec![
        "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        "/ip4/127.0.0.1/tcp/0/ws".parse().unwrap(),
    ];
    let mut a = Network::new(Keypair::generate_ed25519(), config, status(NET_A, 4)).unwrap();
    let a_id = a.local_peer_id();
    let mut tcp = None;
    let mut ws = None;
    while tcp.is_none() || ws.is_none() {
        let event =
            next_matching(&mut a, |event| matches!(event, NetEvent::Listening { .. })).await;
        let NetEvent::Listening { address } = event else {
            unreachable!()
        };
        if is_websocket(&address) {
            ws = Some(tcp_port(&address));
        } else {
            tcp = Some(tcp_port(&address));
        }
    }
    tokio::spawn(serve(a, false));

    // Nama `localhost` di-resolve di bawah WebSocket dan TCP (seperti alamat seed `/dns4/<nama>/tcp/443/tls/sni/...`).
    let ws_address: Multiaddr = format!("/dns4/localhost/tcp/{}/ws/p2p/{a_id}", ws.unwrap())
        .parse()
        .unwrap();
    let tcp_address: Multiaddr = format!("/dns4/localhost/tcp/{}/p2p/{a_id}", tcp.unwrap())
        .parse()
        .unwrap();
    for (address, websocket) in [(ws_address, true), (tcp_address, false)] {
        let mut b = node(NET_A, vec![address], 0);
        let event = next_matching(&mut b, |event| {
            matches!(event, NetEvent::PeerConnected { .. })
        })
        .await;
        let NetEvent::PeerConnected { peer, address } = event else {
            unreachable!()
        };
        assert_eq!(peer, a_id);
        assert_eq!(is_websocket(&address), websocket, "{address}");
        assert_eq!(wait_status(&mut b, a_id).await, status(NET_A, 4));
        assert_eq!(b.connected_peers(), vec![a_id]);
    }
}

#[tokio::test]
async fn keepalive_sends_status_periodically() {
    let mut a = node(NET_A, Vec::new(), 2);
    let a_addr = dial_address(&mut a).await;
    let mut config = NetworkConfig::new(NET_A);
    config.connect = vec![a_addr];
    config.keepalive_interval = Duration::from_millis(100);
    let b = Network::new(Keypair::generate_ed25519(), config, status(NET_A, 0)).unwrap();
    let b_id = b.local_peer_id();
    tokio::spawn(serve(b, false));

    // Saat terhubung `a` menerima dua `status` dari `b` (permintaan `b` dan jawaban atas permintaan `a`). Tiga
    // berikutnya hanya datang dari keepalive `b`; keepalive `a` sendiri baru berdetak sesudah 60 detik.
    for _ in 0..5 {
        assert_eq!(wait_status(&mut a, b_id).await, status(NET_A, 0));
    }
    assert_eq!(a.connected_peers(), vec![b_id]);
}

/// Port TCP bebas di 127.0.0.1 (dipilih sistem operasi, lalu dilepas).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// Node dengan penemuan rekan (TSK-M74): alamat lokal diterima, selang pendek, mendengar di port tetap dan
/// mengumumkannya sebagai alamat publik (TSK-M75). Mengembalikan alamat lengkapnya dengan `/p2p`.
fn discovering(connect: Vec<Multiaddr>, peers_file: Option<PathBuf>) -> (Network, Multiaddr) {
    let listen: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", free_port())
        .parse()
        .unwrap();
    let mut config = NetworkConfig::new(NET_A);
    config.listen = vec![listen.clone()];
    config.public = vec![listen.clone()];
    config.connect = connect;
    config.redial_interval = Duration::from_millis(200);
    config.addr_interval = Duration::from_millis(300);
    config.allow_local = true;
    config.peers_file = peers_file;
    let net = Network::new(Keypair::generate_ed25519(), config, status(NET_A, 0)).unwrap();
    let full = listen.with(Protocol::P2p(net.local_peer_id()));
    (net, full)
}

#[tokio::test]
async fn peers_learn_each_other_through_hub_and_keep_address_book() {
    let (hub, hub_addr) = discovering(Vec::new(), None);
    let hub_id = hub.local_peer_id();
    tokio::spawn(serve(hub, false));
    let (a, _) = discovering(vec![hub_addr.clone()], None);
    let a_id = a.local_peer_id();
    tokio::spawn(serve(a, false));

    // B hanya mengenal hub, mendapat alamat A dari hub (alamat diri A), lalu tersambung langsung dengan A.
    let file = std::env::temp_dir().join(format!("zvrg-mod06-peers-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let (mut b, _) = discovering(vec![hub_addr], Some(file.clone()));
    next_matching(
        &mut b,
        |event| matches!(event, NetEvent::PeerConnected { peer, .. } if *peer == a_id),
    )
    .await;
    let peers = b.connected_peers();
    assert!(
        peers.contains(&hub_id) && peers.contains(&a_id),
        "{peers:?}"
    );
    // Beri waktu jawaban `Addrs` dari A sampai, lalu hentikan B: buku alamatnya tersimpan di `peers.json`.
    let drive = async {
        loop {
            if matches!(b.next_event().await, NetEvent::Listening { .. }) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), drive).await;
    assert!(b.address_book().peers().contains(&a_id));
    drop(b);
    let book = AddressBook::load(&file, true);
    let known = book.peers();
    assert!(
        known.contains(&hub_id) && known.contains(&a_id),
        "{known:?}"
    );
    assert!(book.tried_peers().contains(&hub_id));
    let _ = std::fs::remove_file(&file);
}

#[tokio::test]
async fn local_addresses_are_ignored_by_default() {
    // Tanpa `allow_local` (testnet, mainnet) alamat 127.0.0.1 tidak masuk buku alamat dan tidak diteruskan.
    let mut a = node(NET_A, Vec::new(), 1);
    let a_id = a.local_peer_id();
    let a_addr = dial_address(&mut a).await;
    tokio::spawn(serve(a, false));
    let mut b = node(NET_A, vec![a_addr], 0);
    wait_status(&mut b, a_id).await;
    let drive = async {
        loop {
            if matches!(b.next_event().await, NetEvent::Listening { .. }) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), drive).await;
    assert!(b.address_book().is_empty());
    assert_eq!(b.connected_peers(), vec![a_id]);
}
