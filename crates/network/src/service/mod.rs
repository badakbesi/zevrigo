//! Layanan jaringan MOD-06 (Spesifikasi P2P Bagian 1 sampai 4, 6, dan 7).
//!
//! [`Network`] memegang swarm libp2p: TCP atau WebSocket di atas TCP (`/ws`, TSK-M45), lalu Noise XX dan Yamux; lima
//! protokol request-response; paling banyak 8 koneksi keluar, 32 masuk, dan satu per `PeerId`; koneksi menganggur
//! ditutup setelah 300 detik; permintaan gagal setelah 30 detik. Alamat `/dns4`, `/dns6`, dan `/dns` (juga di bawah
//! `/tls/sni/<nama>/ws`) di-resolve resolver sistem setiap kali didial. Layanan ini sendiri menangani:
//! - sambung ulang ke alamat `--connect` setiap 10 detik jika putus;
//! - pertukaran `status` saat terhubung (versi atau jaringan lain: koneksi ditutup tanpa sanksi);
//! - `status` ke setiap rekan setiap 60 detik (keepalive), karena proxy seperti Cloudflare menutup WebSocket yang diam
//!   sekitar 100 detik;
//! - sanksi untuk pesan rusak dan jawaban `Blocks` yang jumlah entrinya salah (putus dan blokir 24 jam), kecuali rekan
//!   operator dari `--connect`, yang hanya diputus lalu disambung ulang (DEC-41);
//! - putus tanpa blokir setelah 3 permintaan berturut-turut tidak dijawab dalam 30 detik;
//! - penemuan rekan (DEC-43, TSK-M74): `GetAddr` ke setiap rekan saat terhubung dan setiap 10 menit, jawaban `Addrs` berisi
//!   alamat publik node ini (`--p2p-publik`, TSK-M75), alamat yang diumumkan rekan lain yang sedang terhubung untuk dirinya
//!   sendiri, dan alamat teruji buku alamat ([`AddressBook`]); koneksi keluar tambahan dari buku alamat ke kelompok jaringan
//!   yang berbeda; buku alamat disimpan di `peers.json`. Rekan versi lama yang tidak mengenal protokol `addr` tetap
//!   tersambung.
//!
//! Aliran yang berakhir sebelum pesan lengkap (koneksi putus di tengah pesan, atau rekan menutup aliran tanpa menjawab)
//! dilaporkan sebagai permintaan yang gagal, bukan pesan rusak (DEC-41).
//!
//! Permintaan `headers`, `blocks`, `block`, dan `tx` dari rekan diteruskan sebagai [`NetEvent`] beserta [`Reply`]
//! yang wajib dijawab pemanggil (MOD-11). Validasi blok, transaksi, dan header, serta sanksi untuk isi yang tidak sah
//! ([`Network::punish`]), ada di pemanggil.

use std::collections::hash_map::RandomState;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use libp2p::connection_limits::{self, ConnectionLimits};
use libp2p::core::ConnectedPoint;
use libp2p::core::upgrade;
use libp2p::futures::StreamExt;
use libp2p::identity::Keypair;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::{
    self, OutboundFailure, OutboundRequestId, ProtocolSupport, ResponseChannel,
};
use libp2p::swarm::{ConnectionId, NetworkBehaviour, SwarmEvent};
use libp2p::{
    Multiaddr, PeerId, Swarm, Transport as _, allow_block_list, noise, swarm, websocket, yamux,
};
use thiserror::Error;
use tokio::time::{Interval, MissedTickBehavior};
use zevrigo_primitives::{Block, Hash32, Transaction};

use crate::addrbook::{AddressBook, group_of};
use crate::ban::BanList;
use crate::codec::{Received, WireCodec};
use crate::message::{
    Ack, Addrs, Blocks, GetAddr, GetBlocks, GetHeaders, Headers, MAX_ADDRS, MessageError, Status,
    Wire,
};
use crate::protocol::{ProtocolKind, stream_protocol};
use crate::transport::DnsTcp;

/// Koneksi keluar paling banyak.
pub const MAX_OUTBOUND: u32 = 8;

/// Koneksi masuk paling banyak.
pub const MAX_INBOUND: u32 = 32;

/// Koneksi tanpa aliran terbuka ditutup setelah 300 detik.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Permintaan yang tidak dijawab dalam 30 detik dianggap gagal.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Batas waktu membuka koneksi (TCP, Noise, Yamux), sama dengan bawaan `SwarmBuilder` libp2p (10 detik).
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(10);

/// Selang sambung ulang ke alamat `--connect` (10 detik).
pub const REDIAL_INTERVAL: Duration = Duration::from_secs(10);

/// Selang pengiriman `status` ke setiap rekan yang terhubung (60 detik), di bawah batas diam WebSocket Cloudflare
/// (sekitar 100 detik) dan batas koneksi menganggur (300 detik).
pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(60);

/// Jumlah permintaan berturut-turut yang tidak dijawab sebelum koneksi diputus (tanpa blokir).
pub const MAX_TIMEOUTS: u8 = 3;

/// Selang `GetAddr` ke setiap rekan (10 menit, TSK-M74). Rekan juga ditanya sekali saat terhubung.
pub const ADDR_INTERVAL: Duration = Duration::from_secs(600);

/// Dial baru dari buku alamat paling banyak per selang sambung ulang.
pub const BOOK_DIALS_PER_TICK: usize = 2;

/// Alamat diri yang diumumkan satu rekan paling banyak (diteruskan ke rekan lain selama rekan itu terhubung).
pub const MAX_ADVERTISED: usize = 4;

/// Alamat buku yang sudah dicoba didial menunggu `RETRY_FACTOR` kali selang sambung ulang (60 detik) sebelum dicoba lagi.
const RETRY_FACTOR: u32 = 6;

/// Dial dari buku alamat yang tidak berkabar selama ini dilupakan (dial gagal atau berhasil selalu berkabar lebih cepat).
const DIAL_STALE: Duration = Duration::from_secs(60);

/// ID permintaan keluar. Unik per protokol, jadi selalu dibaca bersama jenis event atau [`ProtocolKind`].
pub type RequestId = OutboundRequestId;

type Rr<Req, Resp> = request_response::Behaviour<WireCodec<Req, Resp>>;

/// Gabungan behaviour libp2p. Urutan field = urutan pemeriksaan koneksi baru.
#[derive(NetworkBehaviour)]
struct Behaviour {
    limits: connection_limits::Behaviour,
    banned: allow_block_list::Behaviour<allow_block_list::BlockedPeers>,
    status: Rr<Status, Status>,
    headers: Rr<GetHeaders, Headers>,
    bodies: Rr<GetBlocks, Blocks>,
    block: Rr<Block, Ack>,
    tx: Rr<Transaction, Ack>,
    addr: Rr<GetAddr, Addrs>,
}

fn rr<Req, Resp>(network_id: &Hash32, kind: ProtocolKind) -> Result<Rr<Req, Resp>, NetworkError>
where
    Req: Wire + Send + 'static,
    Resp: Wire + Send + 'static,
{
    let protocol = stream_protocol(network_id, kind)
        .map_err(|error| NetworkError::Protocol(error.to_string()))?;
    Ok(request_response::Behaviour::new(
        [(protocol, ProtocolSupport::Full)],
        request_response::Config::default().with_request_timeout(REQUEST_TIMEOUT),
    ))
}

impl Behaviour {
    fn new(network_id: &Hash32) -> Result<Self, NetworkError> {
        let limits = ConnectionLimits::default()
            .with_max_established_incoming(Some(MAX_INBOUND))
            .with_max_established_outgoing(Some(MAX_OUTBOUND))
            .with_max_established_per_peer(Some(1));
        Ok(Self {
            limits: connection_limits::Behaviour::new(limits),
            banned: allow_block_list::Behaviour::default(),
            status: rr(network_id, ProtocolKind::Status)?,
            headers: rr(network_id, ProtocolKind::Headers)?,
            bodies: rr(network_id, ProtocolKind::Blocks)?,
            block: rr(network_id, ProtocolKind::Block)?,
            tx: rr(network_id, ProtocolKind::Tx)?,
            addr: rr(network_id, ProtocolKind::Addr)?,
        })
    }
}

/// Galat saat membangun layanan jaringan.
#[derive(Debug, Error)]
pub enum NetworkError {
    /// Nama protokol tidak diterima libp2p.
    #[error("nama protokol tidak sah: {0}")]
    Protocol(String),
    /// Transport Noise gagal dibangun.
    #[error("transport gagal dibangun: {0}")]
    Transport(String),
    /// Gagal mendengar di alamat.
    #[error("gagal mendengar di {address}: {reason}")]
    Listen {
        /// Alamat dengar.
        address: String,
        /// Pesan galat.
        reason: String,
    },
}

/// Konfigurasi layanan jaringan.
#[derive(Clone, Debug)]
pub struct NetworkConfig {
    /// `network_id` (block_id genesis) jaringan node.
    pub network_id: Hash32,
    /// Alamat dengar, misalnya `/ip4/127.0.0.1/tcp/28800` atau WebSocket `/ip4/127.0.0.1/tcp/28803/ws`.
    pub listen: Vec<Multiaddr>,
    /// Alamat rekan dari operator (`--connect`), boleh diakhiri `/p2p/<PeerId>`, misalnya
    /// `/dns4/<nama>/tcp/443/tls/sni/<nama>/ws/p2p/<PeerId>`.
    pub connect: Vec<Multiaddr>,
    /// Selang sambung ulang; bawaan [`REDIAL_INTERVAL`] (test memakai selang lebih pendek).
    pub redial_interval: Duration,
    /// Selang keepalive `status`; bawaan [`KEEPALIVE_INTERVAL`] (test memakai selang lebih pendek).
    pub keepalive_interval: Duration,
    /// Alamat publik node ini (tanpa `/p2p`; `PeerId` ditambahkan) yang diumumkan ke rekan lewat `Addrs` (TSK-M75,
    /// `--p2p-publik`). Kosong = node tidak mengumumkan dirinya.
    pub public: Vec<Multiaddr>,
    /// Path buku alamat (`<datadir>/<jaringan>/peers.json`, TSK-M74); `None` = hanya di memori.
    pub peers_file: Option<PathBuf>,
    /// Terima alamat loopback dan privat dari rekan (regtest, uji lokal). Bawaan `false`: alamat itu tidak masuk buku
    /// alamat dan tidak diteruskan.
    pub allow_local: bool,
    /// Selang `GetAddr` ke setiap rekan; bawaan [`ADDR_INTERVAL`] (test memakai selang lebih pendek).
    pub addr_interval: Duration,
}

impl NetworkConfig {
    /// Konfigurasi tanpa alamat dengar dan tanpa rekan, selang sambung ulang 10 detik, keepalive 60 detik, tanpa alamat
    /// publik, buku alamat hanya di memori tanpa alamat lokal, `GetAddr` setiap 10 menit.
    #[must_use]
    pub const fn new(network_id: Hash32) -> Self {
        Self {
            network_id,
            listen: Vec::new(),
            connect: Vec::new(),
            redial_interval: REDIAL_INTERVAL,
            keepalive_interval: KEEPALIVE_INTERVAL,
            public: Vec::new(),
            peers_file: None,
            allow_local: false,
            addr_interval: ADDR_INTERVAL,
        }
    }
}

/// Alasan sanksi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offense {
    /// Pesan rusak (Spesifikasi P2P Bagian 3).
    BadMessage(MessageError),
    /// Jawaban `Blocks` tidak berisi tepat satu entri per ID yang diminta.
    WrongEntryCount {
        /// Jumlah ID yang diminta.
        expected: usize,
        /// Jumlah entri yang diterima.
        actual: usize,
    },
    /// Sanksi dari pemanggil lewat [`Network::punish`] (misalnya blok atau transaksi tidak sah).
    Reported,
}

/// Saluran jawaban untuk satu permintaan masuk. Jika dibuang tanpa dijawab, rekan menerima kegagalan.
#[derive(Debug)]
pub struct Reply<T>(ResponseChannel<Received<T>>);

/// Event dari layanan jaringan.
#[derive(Debug)]
pub enum NetEvent {
    /// Node mendengar di alamat ini.
    Listening {
        /// Alamat dengar (tanpa `/p2p`).
        address: Multiaddr,
    },
    /// Koneksi dengan rekan terbentuk (sesudah batas koneksi dan blokir diperiksa).
    PeerConnected {
        /// Rekan.
        peer: PeerId,
        /// Alamat rekan.
        address: Multiaddr,
    },
    /// Koneksi terakhir dengan rekan tertutup.
    PeerDisconnected {
        /// Rekan.
        peer: PeerId,
    },
    /// `Status` rekan diterima (versi dan jaringan cocok). Bisa muncul dua kali per koneksi: dari permintaan rekan
    /// dan dari jawaban atas permintaan node.
    PeerStatus {
        /// Rekan.
        peer: PeerId,
        /// Ujung chain rekan.
        status: Status,
    },
    /// Rekan meminta header.
    GetHeaders {
        /// Rekan.
        peer: PeerId,
        /// Permintaan.
        request: GetHeaders,
        /// Saluran jawaban.
        reply: Reply<Headers>,
    },
    /// Rekan meminta badan blok.
    GetBlocks {
        /// Rekan.
        peer: PeerId,
        /// Permintaan.
        request: GetBlocks,
        /// Saluran jawaban.
        reply: Reply<Blocks>,
    },
    /// Rekan menyiarkan blok.
    Block {
        /// Rekan.
        peer: PeerId,
        /// Blok (encoding kanonis sudah diperiksa, aturan konsensus belum).
        block: Box<Block>,
        /// Saluran jawaban `Ack`.
        reply: Reply<Ack>,
    },
    /// Rekan menyiarkan transaksi.
    Tx {
        /// Rekan.
        peer: PeerId,
        /// Transaksi (encoding kanonis sudah diperiksa, aturan belum).
        tx: Transaction,
        /// Saluran jawaban `Ack`.
        reply: Reply<Ack>,
    },
    /// Jawaban atas [`Network::request_headers`].
    Headers {
        /// Rekan.
        peer: PeerId,
        /// ID permintaan.
        request: RequestId,
        /// Header dari rekan (belum divalidasi).
        headers: Headers,
    },
    /// Jawaban atas [`Network::request_blocks`], dengan jumlah entri sama dengan jumlah ID yang diminta.
    Blocks {
        /// Rekan.
        peer: PeerId,
        /// ID permintaan.
        request: RequestId,
        /// Entri per ID.
        blocks: Blocks,
    },
    /// Jawaban atas [`Network::send_block`].
    BlockAck {
        /// Rekan.
        peer: PeerId,
        /// ID permintaan.
        request: RequestId,
        /// Jawaban.
        ack: Ack,
    },
    /// Jawaban atas [`Network::send_tx`].
    TxAck {
        /// Rekan.
        peer: PeerId,
        /// ID permintaan.
        request: RequestId,
        /// Jawaban.
        ack: Ack,
    },
    /// Permintaan keluar gagal (tidak terhubung, koneksi tertutup, waktu habis, atau protokol tidak didukung).
    RequestFailed {
        /// Rekan.
        peer: PeerId,
        /// Protokol permintaan.
        kind: ProtocolKind,
        /// ID permintaan.
        request: RequestId,
        /// `true` jika gagal karena 30 detik tanpa jawaban.
        timed_out: bool,
    },
    /// Rekan disanksi: koneksi diputus, dan rekan diblokir 24 jam kecuali rekan operator (DEC-41).
    PeerPunished {
        /// Rekan.
        peer: PeerId,
        /// Alasan.
        offense: Offense,
        /// `true` jika rekan diblokir 24 jam; `false` untuk rekan operator dari `--connect`, yang hanya diputus lalu
        /// disambung ulang pada selang sambung ulang berikutnya.
        banned: bool,
    },
}

/// Data rekan yang sedang terhubung.
#[derive(Debug)]
struct PeerInfo {
    /// Alamat yang didial node (tanpa `/p2p`), jika koneksi ini keluar.
    dialed: Option<Multiaddr>,
    /// Alamat IP rekan.
    ip: Option<IpAddr>,
    /// Permintaan berturut-turut yang tidak dijawab.
    timeouts: u8,
    /// Alamat rekan (tanpa `/p2p`) saat koneksi terbentuk; kelompok jaringannya menjadi sumber alamat dari rekan ini.
    remote: Multiaddr,
    /// Alamat publik yang diumumkan rekan untuk dirinya sendiri (paling banyak [`MAX_ADVERTISED`]).
    advertised: Vec<Multiaddr>,
    /// Waktu terakhir `GetAddr` rekan dijawab dengan alamat.
    addr_served: Option<Instant>,
}

/// Permintaan, jawaban, atau kegagalan dari satu behaviour request-response.
enum Rx<Req, Resp> {
    Request {
        peer: PeerId,
        request: Received<Req>,
        channel: ResponseChannel<Received<Resp>>,
    },
    Response {
        peer: PeerId,
        request: RequestId,
        response: Received<Resp>,
    },
    Failed {
        peer: PeerId,
        request: RequestId,
        error: OutboundFailure,
    },
}

fn split<Req, Resp>(
    event: request_response::Event<Received<Req>, Received<Resp>>,
) -> Option<Rx<Req, Resp>> {
    match event {
        request_response::Event::Message { peer, message, .. } => match message {
            request_response::Message::Request {
                request, channel, ..
            } => Some(Rx::Request {
                peer,
                request,
                channel,
            }),
            request_response::Message::Response {
                request_id,
                response,
            } => Some(Rx::Response {
                peer,
                request: request_id,
                response,
            }),
        },
        request_response::Event::OutboundFailure {
            peer,
            request_id,
            error,
            ..
        } => Some(Rx::Failed {
            peer,
            request: request_id,
            error,
        }),
        request_response::Event::InboundFailure { .. }
        | request_response::Event::ResponseSent { .. } => None,
    }
}

/// Alamat IP pertama di `address`.
pub(crate) fn ip_of(address: &Multiaddr) -> Option<IpAddr> {
    address.iter().find_map(|part| match part {
        Protocol::Ip4(ip) => Some(IpAddr::V4(ip)),
        Protocol::Ip6(ip) => Some(IpAddr::V6(ip)),
        _ => None,
    })
}

/// `PeerId` di bagian `/p2p/...` dari `address`.
pub(crate) fn peer_of(address: &Multiaddr) -> Option<PeerId> {
    address.iter().find_map(|part| match part {
        Protocol::P2p(peer) => Some(peer),
        _ => None,
    })
}

/// `address` tanpa bagian `/p2p/...`.
pub(crate) fn strip_p2p(address: &Multiaddr) -> Multiaddr {
    let mut out = Multiaddr::empty();
    for part in address.iter() {
        if !matches!(part, Protocol::P2p(_)) {
            out.push(part);
        }
    }
    out
}

/// Selang yang berdetak pertama kali sesudah `period` (bukan langsung), paling pendek 1 milidetik.
fn ticker(period: Duration) -> Interval {
    let period = period.max(Duration::from_millis(1));
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    interval
}

/// Layanan jaringan satu node.
pub struct Network {
    swarm: Swarm<Behaviour>,
    network_id: Hash32,
    local_status: Status,
    connect: Vec<Multiaddr>,
    peers: BTreeMap<PeerId, PeerInfo>,
    bans: BanList,
    pending_bodies: BTreeMap<RequestId, usize>,
    events: VecDeque<NetEvent>,
    redial: Interval,
    keepalive: Interval,
    book: AddressBook,
    public: Vec<Multiaddr>,
    peers_file: Option<PathBuf>,
    book_dirty: bool,
    /// Dial dari buku alamat yang sedang berjalan: alamat dan waktu mulai.
    dialing: BTreeMap<PeerId, (Multiaddr, Instant)>,
    retry_after: Duration,
    addr_interval: Duration,
    addr_tick: Interval,
    random: RandomState,
    random_counter: u64,
}

impl Network {
    /// Membangun swarm, memuat buku alamat dari `config.peers_file` (file yang tidak ada atau rusak = buku kosong), mulai
    /// mendengar di `config.listen`, dan mendial `config.connect`.
    ///
    /// Wajib dipanggil di dalam runtime tokio.
    ///
    /// # Errors
    ///
    /// [`NetworkError`] jika nama protokol, transport, atau alamat dengar gagal.
    pub fn new(
        keypair: Keypair,
        config: NetworkConfig,
        local_status: Status,
    ) -> Result<Self, NetworkError> {
        let behaviour = Behaviour::new(&config.network_id)?;
        let local = keypair.public().to_peer_id();
        let mut book = config.peers_file.as_deref().map_or_else(
            || AddressBook::new(config.allow_local),
            |path| AddressBook::load(path, config.allow_local),
        );
        book.remove_peer(&local);
        let public = config
            .public
            .iter()
            .map(|address| strip_p2p(address).with(Protocol::P2p(local)))
            .collect();
        let noise = noise::Config::new(&keypair)
            .map_err(|error| NetworkError::Transport(error.to_string()))?;
        // Seperti `SwarmBuilder::with_tcp` libp2p 0.57.1 (TCP, lalu Noise XX, lalu Yamux, batas waktu 10 detik),
        // dengan WebSocket sebagai jalur kedua: alamat `/ws` ditolak TCP lalu dicoba WebSocket. Keduanya di atas
        // `DnsTcp`, jadi nama `/dns*` di-resolve di bawah lapisan WebSocket (kepala `Host` dan SNI tetap berisi nama).
        let transport = DnsTcp::default()
            .or_transport(websocket::Config::new(DnsTcp::default()))
            .upgrade(upgrade::Version::V1Lazy)
            .authenticate(noise)
            .multiplex(yamux::Config::default())
            .timeout(CONNECTION_TIMEOUT)
            .boxed();
        let mut swarm = Swarm::new(
            transport,
            behaviour,
            local,
            swarm::Config::with_tokio_executor().with_idle_connection_timeout(IDLE_TIMEOUT),
        );
        for address in &config.listen {
            swarm
                .listen_on(address.clone())
                .map_err(|error| NetworkError::Listen {
                    address: address.to_string(),
                    reason: error.to_string(),
                })?;
        }
        for address in &config.connect {
            // Gagal dial (misalnya alamat tidak didukung) tidak fatal; dicoba lagi pada selang berikutnya.
            let _ = swarm.dial(address.clone());
        }
        let redial = ticker(config.redial_interval);
        let keepalive = ticker(config.keepalive_interval);
        let addr_tick = ticker(config.addr_interval);
        Ok(Self {
            swarm,
            network_id: config.network_id,
            local_status,
            connect: config.connect,
            peers: BTreeMap::new(),
            bans: BanList::new(),
            pending_bodies: BTreeMap::new(),
            events: VecDeque::new(),
            redial,
            keepalive,
            book,
            public,
            peers_file: config.peers_file,
            book_dirty: false,
            dialing: BTreeMap::new(),
            retry_after: config.redial_interval.saturating_mul(RETRY_FACTOR),
            addr_interval: config.addr_interval,
            addr_tick,
            random: RandomState::new(),
            random_counter: 0,
        })
    }

    /// Buku alamat node ini (TSK-M74).
    #[must_use]
    pub const fn address_book(&self) -> &AddressBook {
        &self.book
    }

    /// `PeerId` node ini.
    #[must_use]
    pub fn local_peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }

    /// Alamat dengar yang aktif.
    #[must_use]
    pub fn listeners(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    /// Rekan yang sedang terhubung, urut `PeerId`.
    #[must_use]
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers.keys().copied().collect()
    }

    /// Mengganti `Status` yang dikirim ke rekan (setiap kali ujung chain aktif berubah).
    pub fn set_local_status(&mut self, status: Status) {
        self.local_status = status;
    }

    /// `true` jika `peer` sedang diblokir.
    #[must_use]
    pub fn is_banned(&self, peer: &PeerId) -> bool {
        self.bans.is_peer_banned(peer, Instant::now())
    }

    /// Meminta header ke `peer`; jawabannya [`NetEvent::Headers`].
    pub fn request_headers(&mut self, peer: &PeerId, request: GetHeaders) -> RequestId {
        self.swarm
            .behaviour_mut()
            .headers
            .send_request(peer, Ok(request))
    }

    /// Meminta badan blok ke `peer`; jawabannya [`NetEvent::Blocks`].
    pub fn request_blocks(&mut self, peer: &PeerId, request: GetBlocks) -> RequestId {
        let expected = request.ids.len();
        let id = self
            .swarm
            .behaviour_mut()
            .bodies
            .send_request(peer, Ok(request));
        self.pending_bodies.insert(id, expected);
        id
    }

    /// Mengirim blok ke `peer`; jawabannya [`NetEvent::BlockAck`].
    pub fn send_block(&mut self, peer: &PeerId, block: Block) -> RequestId {
        self.swarm
            .behaviour_mut()
            .block
            .send_request(peer, Ok(block))
    }

    /// Mengirim transaksi ke `peer`; jawabannya [`NetEvent::TxAck`].
    pub fn send_tx(&mut self, peer: &PeerId, tx: Transaction) -> RequestId {
        self.swarm.behaviour_mut().tx.send_request(peer, Ok(tx))
    }

    /// Menjawab [`NetEvent::GetHeaders`]. `false` jika permintaan sudah tidak menunggu jawaban.
    pub fn reply_headers(&mut self, reply: Reply<Headers>, headers: Headers) -> bool {
        self.swarm
            .behaviour_mut()
            .headers
            .send_response(reply.0, Ok(headers))
            .is_ok()
    }

    /// Menjawab [`NetEvent::GetBlocks`] (tepat satu entri per ID, urutan sama).
    pub fn reply_blocks(&mut self, reply: Reply<Blocks>, blocks: Blocks) -> bool {
        self.swarm
            .behaviour_mut()
            .bodies
            .send_response(reply.0, Ok(blocks))
            .is_ok()
    }

    /// Menjawab [`NetEvent::Block`] atau [`NetEvent::Tx`].
    pub fn reply_ack(&mut self, reply: Reply<Ack>, ack: Ack) -> bool {
        // Saluran jawaban tidak terikat pada behaviour tertentu, jadi `block` juga dipakai untuk `tx`.
        self.swarm
            .behaviour_mut()
            .block
            .send_response(reply.0, Ok(ack))
            .is_ok()
    }

    /// Memutus koneksi dengan `peer` tanpa blokir.
    pub fn disconnect(&mut self, peer: &PeerId) {
        let _ = self.swarm.disconnect_peer_id(*peer);
    }

    /// Memberi sanksi kepada `peer`: putus dan blokir 24 jam (per `PeerId` dan per IP bukan loopback). Rekan operator
    /// dari `--connect` hanya diputus, tanpa blokir (DEC-41).
    pub fn punish(&mut self, peer: &PeerId) {
        self.apply_punishment(*peer, Offense::Reported);
    }

    /// Menunggu event berikutnya. Selama menunggu, layanan juga menjawab `status`, menyambung ulang, dan mencabut
    /// blokir yang sudah berakhir, serta mengirim keepalive `status`.
    pub async fn next_event(&mut self) -> NetEvent {
        loop {
            if let Some(event) = self.events.pop_front() {
                return event;
            }
            tokio::select! {
                event = self.swarm.select_next_some() => self.on_swarm_event(event),
                _ = self.redial.tick() => self.on_tick(),
                _ = self.keepalive.tick() => self.on_keepalive(),
                _ = self.addr_tick.tick() => self.on_addr_tick(),
            }
        }
    }

    fn on_swarm_event(&mut self, event: SwarmEvent<BehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                self.events.push_back(NetEvent::Listening { address });
            }
            SwarmEvent::ConnectionEstablished {
                peer_id,
                connection_id,
                endpoint,
                ..
            } => self.on_connected(peer_id, connection_id, &endpoint),
            SwarmEvent::ConnectionClosed {
                peer_id,
                num_established,
                ..
            } => {
                if num_established == 0 && self.peers.remove(&peer_id).is_some() {
                    self.events
                        .push_back(NetEvent::PeerDisconnected { peer: peer_id });
                }
            }
            SwarmEvent::OutgoingConnectionError {
                peer_id: Some(peer),
                ..
            } => self.on_dial_failed(peer),
            SwarmEvent::Behaviour(event) => self.on_behaviour(event),
            _ => {}
        }
    }

    fn on_connected(&mut self, peer: PeerId, connection: ConnectionId, endpoint: &ConnectedPoint) {
        let now = Instant::now();
        let address = endpoint.get_remote_address().clone();
        let ip = ip_of(&address);
        let ip_banned = ip.is_some_and(|ip| self.bans.is_ip_banned(&ip, now));
        if ip_banned || self.bans.is_peer_banned(&peer, now) {
            self.dialing.remove(&peer);
            self.swarm.close_connection(connection);
            return;
        }
        let remote = strip_p2p(&address);
        let dialed = endpoint.is_dialer().then(|| remote.clone());
        if dialed.is_some() {
            // Alamat yang berhasil didial (termasuk seed dan `--connect`) masuk tabel teruji buku alamat.
            self.dialing.remove(&peer);
            if self
                .book
                .mark_success(&remote.clone().with(Protocol::P2p(peer)))
            {
                self.book_dirty = true;
            }
        }
        self.peers.insert(
            peer,
            PeerInfo {
                dialed,
                ip,
                timeouts: 0,
                remote,
                advertised: Vec::new(),
                addr_served: None,
            },
        );
        self.events
            .push_back(NetEvent::PeerConnected { peer, address });
        let status = self.local_status.clone();
        self.swarm
            .behaviour_mut()
            .status
            .send_request(&peer, Ok(status));
        self.swarm
            .behaviour_mut()
            .addr
            .send_request(&peer, Ok(GetAddr));
    }

    fn on_behaviour(&mut self, event: BehaviourEvent) {
        match event {
            BehaviourEvent::Limits(never) | BehaviourEvent::Banned(never) => match never {},
            BehaviourEvent::Status(event) => self.on_status(event),
            BehaviourEvent::Headers(event) => self.on_headers(event),
            BehaviourEvent::Bodies(event) => self.on_bodies(event),
            BehaviourEvent::Block(event) => self.on_block(event),
            BehaviourEvent::Tx(event) => self.on_tx(event),
            BehaviourEvent::Addr(event) => self.on_addr(event),
        }
    }

    fn on_status(&mut self, event: request_response::Event<Received<Status>, Received<Status>>) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(status) if status.is_compatible(&self.network_id) => {
                    let local = self.local_status.clone();
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .status
                        .send_response(channel, Ok(local));
                    self.events.push_back(NetEvent::PeerStatus { peer, status });
                }
                Ok(_) => self.disconnect(&peer),
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response { peer, response, .. }) => {
                self.reset_timeouts(&peer);
                match response {
                    Ok(status) if status.is_compatible(&self.network_id) => {
                        self.events.push_back(NetEvent::PeerStatus { peer, status });
                    }
                    Ok(_) => self.disconnect(&peer),
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            Some(Rx::Failed {
                peer,
                request,
                error,
            }) => self.on_failure(peer, ProtocolKind::Status, request, &error),
            None => {}
        }
    }

    fn on_headers(
        &mut self,
        event: request_response::Event<Received<GetHeaders>, Received<Headers>>,
    ) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(request) => self.events.push_back(NetEvent::GetHeaders {
                    peer,
                    request,
                    reply: Reply(channel),
                }),
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response {
                peer,
                request,
                response,
            }) => {
                self.reset_timeouts(&peer);
                match response {
                    Ok(headers) => self.events.push_back(NetEvent::Headers {
                        peer,
                        request,
                        headers,
                    }),
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            Some(Rx::Failed {
                peer,
                request,
                error,
            }) => self.on_failure(peer, ProtocolKind::Headers, request, &error),
            None => {}
        }
    }

    fn on_bodies(&mut self, event: request_response::Event<Received<GetBlocks>, Received<Blocks>>) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(request) => self.events.push_back(NetEvent::GetBlocks {
                    peer,
                    request,
                    reply: Reply(channel),
                }),
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response {
                peer,
                request,
                response,
            }) => {
                self.reset_timeouts(&peer);
                let expected = self.pending_bodies.remove(&request);
                match response {
                    Ok(blocks) => {
                        let actual = blocks.entries.len();
                        match expected {
                            Some(expected) if expected != actual => self.apply_punishment(
                                peer,
                                Offense::WrongEntryCount { expected, actual },
                            ),
                            _ => self.events.push_back(NetEvent::Blocks {
                                peer,
                                request,
                                blocks,
                            }),
                        }
                    }
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            Some(Rx::Failed {
                peer,
                request,
                error,
            }) => {
                self.pending_bodies.remove(&request);
                self.on_failure(peer, ProtocolKind::Blocks, request, &error);
            }
            None => {}
        }
    }

    fn on_block(&mut self, event: request_response::Event<Received<Block>, Received<Ack>>) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(block) => self.events.push_back(NetEvent::Block {
                    peer,
                    block: Box::new(block),
                    reply: Reply(channel),
                }),
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response {
                peer,
                request,
                response,
            }) => {
                self.reset_timeouts(&peer);
                match response {
                    Ok(ack) => self
                        .events
                        .push_back(NetEvent::BlockAck { peer, request, ack }),
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            Some(Rx::Failed {
                peer,
                request,
                error,
            }) => self.on_failure(peer, ProtocolKind::Block, request, &error),
            None => {}
        }
    }

    fn on_tx(&mut self, event: request_response::Event<Received<Transaction>, Received<Ack>>) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(tx) => self.events.push_back(NetEvent::Tx {
                    peer,
                    tx,
                    reply: Reply(channel),
                }),
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response {
                peer,
                request,
                response,
            }) => {
                self.reset_timeouts(&peer);
                match response {
                    Ok(ack) => self
                        .events
                        .push_back(NetEvent::TxAck { peer, request, ack }),
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            Some(Rx::Failed {
                peer,
                request,
                error,
            }) => self.on_failure(peer, ProtocolKind::Tx, request, &error),
            None => {}
        }
    }

    fn on_addr(&mut self, event: request_response::Event<Received<GetAddr>, Received<Addrs>>) {
        match split(event) {
            Some(Rx::Request {
                peer,
                request,
                channel,
            }) => match request {
                Ok(GetAddr) => {
                    let addresses = self.addresses_for(peer);
                    let _ = self
                        .swarm
                        .behaviour_mut()
                        .addr
                        .send_response(channel, Ok(Addrs { addresses }));
                }
                Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
            },
            Some(Rx::Response { peer, response, .. }) => {
                self.reset_timeouts(&peer);
                match response {
                    Ok(addrs) => self.on_addrs(peer, &addrs),
                    Err(error) => self.apply_punishment(peer, Offense::BadMessage(error)),
                }
            }
            // Rekan versi lama tidak mengenal protokol `addr` (tetap tersambung); kegagalan lain tidak memengaruhi
            // sinkronisasi dan tidak dihitung.
            Some(Rx::Failed { .. }) | None => {}
        }
    }

    /// Alamat untuk jawaban `Addrs` ke `requester`: alamat publik node ini, alamat diri rekan lain yang sedang terhubung,
    /// lalu alamat teruji buku alamat; tanpa alamat milik `requester`, tanpa duplikat, paling banyak [`MAX_ADDRS`].
    /// Rekan yang bertanya lagi sebelum separuh selang `GetAddr` menerima jawaban kosong.
    fn addresses_for(&mut self, requester: PeerId) -> Vec<Multiaddr> {
        let now = Instant::now();
        let half = self.addr_interval / 2;
        let Some(info) = self.peers.get_mut(&requester) else {
            return Vec::new();
        };
        if info
            .addr_served
            .is_some_and(|served| now.saturating_duration_since(served) < half)
        {
            return Vec::new();
        }
        info.addr_served = Some(now);
        let mut out: Vec<Multiaddr> = self.public.clone();
        for (peer, info) in &self.peers {
            if *peer != requester {
                out.extend(info.advertised.iter().cloned());
            }
        }
        let random = self.next_random();
        out.extend(self.book.shareable(MAX_ADDRS, random));
        let mut seen = BTreeSet::new();
        out.retain(|address| {
            peer_of(address) != Some(requester) && seen.insert(address.to_string())
        });
        out.truncate(MAX_ADDRS);
        out
    }

    /// Memproses `Addrs` dari `peer`: alamat yang sah masuk tabel baru dengan sumber kelompok jaringan `peer`; alamat milik
    /// `peer` sendiri juga dicatat sebagai alamat dirinya, untuk diteruskan selama `peer` terhubung.
    fn on_addrs(&mut self, peer: PeerId, addrs: &Addrs) {
        let local = self.local_peer_id();
        let Some(source) = self.peers.get(&peer).map(|info| group_of(&info.remote)) else {
            return;
        };
        let mut own: Vec<Multiaddr> = Vec::new();
        for address in &addrs.addresses {
            let Some(owner) = self.book.acceptable(address) else {
                continue;
            };
            if owner == local {
                continue;
            }
            if owner == peer && own.len() < MAX_ADVERTISED && !own.contains(address) {
                own.push(address.clone());
            }
            if self.book.add_heard(address, &source) {
                self.book_dirty = true;
            }
        }
        if !own.is_empty()
            && let Some(info) = self.peers.get_mut(&peer)
        {
            info.advertised = own;
        }
    }

    /// `GetAddr` ke setiap rekan yang terhubung.
    fn on_addr_tick(&mut self) {
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for peer in peers {
            self.swarm
                .behaviour_mut()
                .addr
                .send_request(&peer, Ok(GetAddr));
        }
    }

    /// Dial dari buku alamat ke `peer` gagal: hitungan gagal alamat itu bertambah, kecuali rekan sudah tersambung lewat
    /// koneksi lain (misalnya rekan itu lebih dulu menyambung ke node ini).
    fn on_dial_failed(&mut self, peer: PeerId) {
        if let Some((address, _)) = self.dialing.remove(&peer)
            && !self.peers.contains_key(&peer)
        {
            self.book.mark_failure(&address);
            self.book_dirty = true;
        }
    }

    /// Menambah koneksi keluar dari buku alamat sampai [`MAX_OUTBOUND`] dikurangi jumlah alamat `--connect` (jangkar yang
    /// selalu dipertahankan), paling banyak [`BOOK_DIALS_PER_TICK`] per selang, ke rekan dan kelompok jaringan yang belum
    /// dipakai koneksi keluar (perlindungan eclipse).
    fn dial_from_book(&mut self, now: Instant) {
        let target = usize::try_from(MAX_OUTBOUND)
            .unwrap_or(usize::MAX)
            .saturating_sub(self.connect.len());
        let mut busy_peers: BTreeSet<PeerId> = self.peers.keys().copied().collect();
        busy_peers.extend(self.dialing.keys().copied());
        let mut busy_groups: BTreeSet<String> = self
            .dialing
            .values()
            .map(|(address, _)| group_of(address))
            .collect();
        let mut outbound = self.dialing.len();
        for (peer, info) in &self.peers {
            if let Some(dialed) = &info.dialed {
                busy_groups.insert(group_of(dialed));
                if !self.is_operator(peer) {
                    outbound = outbound.saturating_add(1);
                }
            }
        }
        for _ in 0..BOOK_DIALS_PER_TICK {
            if outbound >= target {
                break;
            }
            let random = self.next_random();
            let Some(address) =
                self.book
                    .pick(now, self.retry_after, &busy_peers, &busy_groups, random)
            else {
                break;
            };
            self.book.mark_attempt(&address, now);
            let Some(peer) = peer_of(&address) else {
                continue;
            };
            busy_peers.insert(peer);
            busy_groups.insert(group_of(&address));
            let banned = self.bans.is_peer_banned(&peer, now)
                || ip_of(&address).is_some_and(|ip| self.bans.is_ip_banned(&ip, now));
            if banned {
                continue;
            }
            if self.swarm.dial(address.clone()).is_ok() {
                self.dialing.insert(peer, (address, now));
                outbound = outbound.saturating_add(1);
            } else {
                self.book.mark_failure(&address);
                self.book_dirty = true;
            }
        }
    }

    /// Menyimpan buku alamat ke `peers.json` bila ada. Gagal menyimpan tidak menghentikan node; dicoba lagi pada selang
    /// berikutnya.
    fn save_book(&mut self) {
        let saved = self
            .peers_file
            .as_deref()
            .is_none_or(|path| self.book.save(path).is_ok());
        if saved {
            self.book_dirty = false;
        }
    }

    /// Bilangan acak 64 bit dari `RandomState` (kunci acak sistem operasi per proses), tanpa dependensi baru.
    fn next_random(&mut self) -> u64 {
        self.random_counter = self.random_counter.wrapping_add(1);
        let mut hasher = self.random.build_hasher();
        hasher.write_u64(self.random_counter);
        hasher.finish()
    }

    fn on_failure(
        &mut self,
        peer: PeerId,
        kind: ProtocolKind,
        request: RequestId,
        error: &OutboundFailure,
    ) {
        let timed_out = matches!(error, OutboundFailure::Timeout);
        match error {
            OutboundFailure::Timeout => {
                let mut too_many = false;
                if let Some(info) = self.peers.get_mut(&peer) {
                    info.timeouts = info.timeouts.saturating_add(1);
                    if info.timeouts >= MAX_TIMEOUTS {
                        info.timeouts = 0;
                        too_many = true;
                    }
                }
                if too_many {
                    self.disconnect(&peer);
                }
            }
            // Rekan tidak mendukung protokol jaringan ini (jaringan lain): putus tanpa sanksi.
            OutboundFailure::UnsupportedProtocols => self.disconnect(&peer),
            _ => {}
        }
        self.events.push_back(NetEvent::RequestFailed {
            peer,
            kind,
            request,
            timed_out,
        });
    }

    fn reset_timeouts(&mut self, peer: &PeerId) {
        if let Some(info) = self.peers.get_mut(peer) {
            info.timeouts = 0;
        }
    }

    /// `true` jika `peer` adalah rekan operator: `PeerId`-nya tertulis di alamat `--connect`, atau koneksinya didial ke
    /// salah satu alamat `--connect` (DEC-41).
    fn is_operator(&self, peer: &PeerId) -> bool {
        if self
            .connect
            .iter()
            .any(|address| peer_of(address) == Some(*peer))
        {
            return true;
        }
        self.peers
            .get(peer)
            .and_then(|info| info.dialed.as_ref())
            .is_some_and(|dialed| {
                self.connect
                    .iter()
                    .any(|address| strip_p2p(address) == *dialed)
            })
    }

    fn apply_punishment(&mut self, peer: PeerId, offense: Offense) {
        // DEC-41: rekan operator (biasanya seed resmi, sering satu-satunya rekan) tidak diblokir. Blokir 24 jam karena
        // gangguan sesaat memutus node dari jaringan; rekan itu dipilih operator sendiri, jadi cukup diputus lalu
        // disambung ulang pada selang berikutnya.
        let banned = !self.is_operator(&peer);
        if banned {
            let ip = self.peers.get(&peer).and_then(|info| info.ip);
            self.bans.ban(peer, ip, Instant::now());
            // Memutus semua koneksi rekan ini dan menolak koneksi barunya.
            self.swarm.behaviour_mut().banned.block_peer(peer);
        } else {
            self.disconnect(&peer);
        }
        self.events.push_back(NetEvent::PeerPunished {
            peer,
            offense,
            banned,
        });
    }

    fn on_tick(&mut self) {
        let now = Instant::now();
        for peer in self.bans.expire(now) {
            self.swarm.behaviour_mut().banned.unblock_peer(peer);
        }
        let targets: Vec<Multiaddr> = self
            .connect
            .iter()
            .filter(|address| self.should_dial(address, now))
            .cloned()
            .collect();
        for address in targets {
            let _ = self.swarm.dial(address);
        }
        self.dialing
            .retain(|_, (_, since)| now.saturating_duration_since(*since) < DIAL_STALE);
        self.dial_from_book(now);
        if self.book_dirty {
            self.save_book();
        }
    }

    /// Mengirim `status` ke setiap rekan yang terhubung. Jawabannya memperbarui ujung chain rekan
    /// ([`NetEvent::PeerStatus`]), dan lalu lintas ini menjaga koneksi lewat proxy tetap hidup.
    fn on_keepalive(&mut self) {
        let peers: Vec<PeerId> = self.peers.keys().copied().collect();
        for peer in peers {
            let status = self.local_status.clone();
            self.swarm
                .behaviour_mut()
                .status
                .send_request(&peer, Ok(status));
        }
    }

    fn should_dial(&self, address: &Multiaddr, now: Instant) -> bool {
        if let Some(peer) = peer_of(address)
            && (self.peers.contains_key(&peer) || self.bans.is_peer_banned(&peer, now))
        {
            return false;
        }
        if ip_of(address).is_some_and(|ip| self.bans.is_ip_banned(&ip, now)) {
            return false;
        }
        let plain = strip_p2p(address);
        !self
            .peers
            .values()
            .any(|info| info.dialed.as_ref() == Some(&plain))
    }
}

impl Drop for Network {
    /// Buku alamat disimpan saat layanan berhenti, sehingga node yang dibuka ulang tersambung lagi tanpa seed.
    fn drop(&mut self) {
        if self.book_dirty {
            self.save_book();
        }
    }
}

#[cfg(test)]
mod tests;
