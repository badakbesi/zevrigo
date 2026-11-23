//! FTR-04 lanjutan (DEC-43, TSK-M74, TSK-M75): penemuan rekan lewat protokol `addr` dan buku alamat, dari ujung ke ujung.
//!
//! Lima node (MOD-11, `MemoryStore`) dengan layanan P2P sungguhan di 127.0.0.1, seperti `ftr04_multi_node`. Hanya seed S
//! yang dikenal: node A sampai D masing-masing hanya punya alamat S (`--connect`). Setiap node mengumumkan alamat dengarnya
//! sendiri (`--p2p-publik`), dan alamat lokal diterima (seperti regtest di daemon). Selesai bila: A sampai D saling
//! tersambung langsung (setiap pasangan tercatat teruji di buku alamat salah satunya), blok tetap tersiar ke semua node
//! sesudah S mati, dan node yang dibuka ulang tanpa `--connect` tersambung lagi dari `peers.json` lalu mengejar.

#![forbid(unsafe_code)]

#[cfg(test)]
mod penemuan {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use tokio::runtime::Runtime;
    use tokio::sync::mpsc;
    use tokio::task::JoinHandle;
    use zevrigo_consensus::{
        BlockError, ChainParams, REGTEST_GENESIS_TIMESTAMP, check_block_context_free,
    };
    use zevrigo_crypto::SecretKey;
    use zevrigo_mempool::DEFAULT_MAX_BYTES;
    use zevrigo_network::{AddressBook, Keypair, Multiaddr, Network, NetworkConfig, PeerId};
    use zevrigo_node::{Node, P2pService, Relay, local_status};
    use zevrigo_primitives::{Block, Decode, Encode, Header, OutputKind};
    use zevrigo_rpc::{GetBlockTemplateParams, Hex32, KindNumber, RpcBackend, SubmitStatus};
    use zevrigo_storage::MemoryStore;

    /// Jam semua node: 1_000 jadwal blok sesudah genesis (target tetap `pow_limit`).
    const NOW: u64 = REGTEST_GENESIS_TIMESTAMP + 120 * 1_000;
    /// Batas waktu menunggu.
    const WAIT: Duration = Duration::from_secs(60);

    type TestNode = Arc<Node<MemoryStore>>;

    /// Sesi P2P satu node.
    struct Session {
        task: JoinHandle<()>,
        relay: Relay<MemoryStore>,
        peer: PeerId,
        address: Multiaddr,
    }

    fn key(n: u8) -> [u8; 32] {
        let mut bytes = [0_u8; 32];
        if let Some(last) = bytes.last_mut() {
            *last = n;
        }
        *SecretKey::from_bytes(&bytes)
            .unwrap()
            .public_key()
            .as_bytes()
    }

    fn open_node() -> TestNode {
        let node = Node::open(
            MemoryStore::default(),
            ChainParams::regtest().unwrap(),
            DEFAULT_MAX_BYTES,
            Box::new(|| NOW),
        )
        .unwrap();
        Arc::new(node)
    }

    /// Port TCP bebas di 127.0.0.1 (dipilih sistem operasi, lalu dilepas).
    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    /// Menjalankan sesi P2P: mendengar di port bebas 127.0.0.1, mengumumkan alamat itu, mendial `connect`, buku alamat di
    /// `peers`. Selang dipercepat untuk uji.
    fn start(rt: &Runtime, node: &TestNode, connect: &[Multiaddr], peers: &Path) -> Session {
        let listen: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", free_port())
            .parse()
            .unwrap();
        let mut config = NetworkConfig::new(node.params().network_id());
        config.listen = vec![listen.clone()];
        config.public = vec![listen.clone()];
        config.connect = connect.to_vec();
        config.redial_interval = Duration::from_millis(300);
        config.addr_interval = Duration::from_millis(500);
        config.allow_local = true;
        config.peers_file = Some(peers.to_path_buf());
        let status = local_status(node.as_ref()).unwrap();
        let net = rt
            .block_on(async { Network::new(Keypair::generate_ed25519(), config, status).unwrap() });
        let peer = net.local_peer_id();
        let address = format!("{listen}/p2p/{peer}").parse::<Multiaddr>().unwrap();
        let (sender, receiver) = mpsc::unbounded_channel();
        let relay = Relay::new(Arc::clone(node), sender);
        let task = rt.spawn(P2pService::new(Arc::clone(node), net).run(receiver));
        Session {
            task,
            relay,
            peer,
            address,
        }
    }

    /// Menghentikan sesi P2P (buku alamat tersimpan saat layanan berhenti).
    fn stop(rt: &Runtime, session: Session) {
        session.task.abort();
        assert!(
            rt.block_on(session.task)
                .is_err_and(|error| error.is_cancelled())
        );
    }

    fn solve(block: &Block) -> Block {
        let params = ChainParams::regtest().unwrap();
        let h = block.header();
        (0_u64..)
            .map(|nonce| {
                let header = Header::new(
                    h.height(),
                    h.prev_block_id(),
                    h.merkle_root(),
                    h.timestamp(),
                    h.target(),
                    nonce,
                );
                Block::new(header, block.txs().to_vec()).unwrap()
            })
            .find(|candidate| {
                !matches!(
                    check_block_context_free(candidate, &params),
                    Err(BlockError::HighHash)
                )
            })
            .unwrap()
    }

    fn mine(session: &Session, count: u64) -> Hex32 {
        let payout = GetBlockTemplateParams {
            payout_kind: KindNumber::new(OutputKind::SingleKey),
            payout_payload: Hex32::new(key(2)),
        };
        let mut last = None;
        for _ in 0..count {
            let template = session.relay.block_template(&payout).unwrap();
            let block = solve(&Block::from_bytes(template.hex.as_slice()).unwrap());
            let result = session.relay.submit_block(&block.to_bytes()).unwrap();
            assert_eq!(result.status, SubmitStatus::Accepted, "{result:?}");
            last = result.block_id;
        }
        last.unwrap()
    }

    fn tip(node: &TestNode) -> (u64, Hex32) {
        let info = node.chain_info().unwrap();
        (info.height, info.best_block_id)
    }

    fn wait_tip(what: &str, nodes: &[&TestNode], height: u64, id: Hex32) {
        let deadline = Instant::now() + WAIT;
        while !nodes.iter().all(|node| tip(node) == (height, id)) {
            let tips: Vec<(u64, Hex32)> = nodes.iter().copied().map(tip).collect();
            assert!(
                Instant::now() < deadline,
                "{what}: ujung {tips:?}, seharusnya {height} {id:?}"
            );
            sleep(Duration::from_millis(50));
        }
    }

    /// Menunggu sampai setiap pasangan sesi tersambung langsung: `PeerId` salah satunya tercatat teruji (pernah berhasil
    /// didial) di `peers.json` yang lain.
    fn wait_direct(sessions: &[(&Session, PathBuf)]) {
        let deadline = Instant::now() + WAIT;
        loop {
            let tried: Vec<_> = sessions
                .iter()
                .map(|(_, path)| AddressBook::load(path, true).tried_peers())
                .collect();
            let mut missing = Vec::new();
            for (i, (a, _)) in sessions.iter().enumerate() {
                for (j, (b, _)) in sessions.iter().enumerate().skip(i + 1) {
                    let direct = tried.get(i).is_some_and(|set| set.contains(&b.peer))
                        || tried.get(j).is_some_and(|set| set.contains(&a.peer));
                    if !direct {
                        missing.push((i, j));
                    }
                }
            }
            if missing.is_empty() {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "pasangan belum tersambung langsung: {missing:?}"
            );
            sleep(Duration::from_millis(200));
        }
    }

    #[test]
    fn peers_find_each_other_and_survive_seed_loss() {
        let rt = Runtime::new().unwrap();
        let dir = std::env::temp_dir().join(format!("zvrg-ftr04-penemuan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let book = |name: &str| dir.join(format!("{name}-peers.json"));
        let [s, a, b, c, d] = [(); 5].map(|()| open_node());

        // 1. S mendengar; A sampai D hanya mengenal S. Blok pertama dari A sampai ke semua node lewat S.
        let ss = start(&rt, &s, &[], &book("s"));
        let seed = [ss.address.clone()];
        let sa = start(&rt, &a, &seed, &book("a"));
        let sb = start(&rt, &b, &seed, &book("b"));
        let sc = start(&rt, &c, &seed, &book("c"));
        let sd = start(&rt, &d, &seed, &book("d"));
        let tip_1 = mine(&sa, 1);
        wait_tip("blok 1 lewat S", &[&s, &a, &b, &c, &d], 1, tip_1);

        // 2. Pertukaran alamat: setiap pasangan A sampai D tersambung langsung, tanpa lewat S.
        wait_direct(&[
            (&sa, book("a")),
            (&sb, book("b")),
            (&sc, book("c")),
            (&sd, book("d")),
        ]);

        // 3. S mati: blok dari A tetap sampai ke B, C, dan D.
        stop(&rt, ss);
        let tip_2 = mine(&sa, 1);
        wait_tip("blok 2 tanpa S", &[&a, &b, &c, &d], 2, tip_2);

        // 4. D berhenti selama A menambang 2 blok, lalu dibuka ulang tanpa --connect: tersambung lagi dari buku alamat
        //    (alamat A, B, atau C di peers.json) dan mengejar.
        stop(&rt, sd);
        let tip_4 = mine(&sa, 2);
        wait_tip("blok 3 dan 4 tanpa D", &[&a, &b, &c], 4, tip_4);
        assert_eq!(tip(&d), (2, tip_2));
        let known = AddressBook::load(&book("d"), true).peers();
        assert!(
            [sa.peer, sb.peer, sc.peer]
                .iter()
                .all(|peer| known.contains(peer)),
            "{known:?}"
        );
        let sd = start(&rt, &d, &[], &book("d"));
        wait_tip("D tersambung dari buku alamat", &[&d], 4, tip_4);

        for session in [sa, sb, sc, sd] {
            stop(&rt, session);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
