//! Daemon node regtest (TSK-M18 bagian B, TSK-M28): argumen baris perintah, folder data, logging, server RPC lokal,
//! P2P, dan berhenti bersih saat menerima SIGINT (Ctrl-C) atau SIGTERM.
//!
//! Tanpa `clap` dan `toml` (DEC-10 bagian 2): argumen dibaca sendiri dan tidak ada file konfigurasi. Chain disimpan di
//! `<datadir>/regtest/chain.redb` (RedbStore, MOD-05), kunci rekan P2P di `<datadir>/regtest/p2p_key`. Setelah server
//! RPC siap, daemon mencetak baris `rpc: http://<alamat>` ke stdout, lalu `p2p: /ip4/<alamat>/tcp/<port>/p2p/<PeerId>`
//! setelah P2P mendengar (Spesifikasi P2P v0.1 Bagian 8), dan `p2p-ws: .../ws/p2p/<PeerId>` jika `--p2p-ws-port`
//! dipakai (TSK-M45); log ditulis ke stderr.
//!
//! Seed resmi bawaan (DEC-43, TSK-M73): [`official_seeds`] per jaringan, didial bersama alamat `--connect` dan diperlakukan
//! sama (rekan operator, tidak diblokir; DEC-41). Regtest hanya dengan `--seed-resmi`, karena regtest juga dipakai untuk uji
//! lokal; testnet dan mainnet selalu, kecuali `--tanpa-seed-resmi`.
//!
//! Penemuan rekan dan sambungan masuk (DEC-43, TSK-M74, TSK-M75): buku alamat di `<datadir>/<jaringan>/peers.json`
//! (MOD-06); regtest menerima alamat lokal untuk uji di satu mesin. `--p2p-bind <ip>` memilih alamat dengar P2P TCP (bawaan
//! `127.0.0.1`), `--p2p-publik <multiaddr>` alamat yang diumumkan ke rekan. RPC tetap hanya di `127.0.0.1`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use thiserror::Error;
use tokio::sync::mpsc;
use zevrigo_consensus::{ChainParams, ParamsError};
use zevrigo_mempool::DEFAULT_MAX_BYTES;
use zevrigo_network::{
    KeyError, Multiaddr, Network as P2pNetwork, NetworkConfig, NetworkError, P2P_KEY_FILE,
    PEERS_FILE, load_or_create,
};
use zevrigo_primitives::Network;
use zevrigo_rpc::transport::default_port;
use zevrigo_rpc::{RpcBackend, RpcError, RpcServer, ServerError};
use zevrigo_storage::{ChainStore, RedbStore, StoreError};

use crate::chain::{Clock, Node, NodeError};
use crate::p2p::{P2pService, Relay, local_status};

/// Teks bantuan `--help`.
pub const USAGE: &str = concat!(
    "zevrigo-node (MOD-11): daemon node Zevrigo Coin\n",
    "\n",
    "Pemakaian:\n",
    "  zevrigo-node --regtest --datadir <folder> [--rpc-port <port>] [--p2p-port <port>]\n",
    "                   [--p2p-ws-port <port>] [--p2p-bind <ip>] [--p2p-publik <multiaddr>]...\n",
    "                   [--connect <multiaddr>]... [--seed-resmi]\n",
    "  zevrigo-node --testnet --datadir <folder> [opsi yang sama] [--tanpa-seed-resmi]\n",
    "  zevrigo-node --help | --version\n",
    "\n",
    "Opsi:\n",
    "  --regtest           jaringan regtest (PoW dev, untuk pengembangan)\n",
    "  --testnet           jaringan testnet (PoW RandomX; mainnet diluncurkan sesudah testnet)\n",
    "  --datadir <folder>  folder data; chain disimpan di <folder>/<jaringan>/chain.redb\n",
    "  --rpc-port <port>   port JSON-RPC di 127.0.0.1 (bawaan regtest 28801, testnet 18801; 0 = port bebas)\n",
    "  --p2p-port <port>   port P2P TCP (bawaan regtest 28800, testnet 18800; 0 = port bebas)\n",
    "  --p2p-bind <ip>     alamat IP dengar P2P TCP (bawaan 127.0.0.1). 0.0.0.0 atau :: menerima sambungan\n",
    "                      masuk dari luar: IP Anda terlihat oleh rekan, dan port perlu diizinkan firewall/router\n",
    "  --p2p-publik <addr> alamat yang diumumkan ke rekan lewat pertukaran alamat, misalnya\n",
    "                      /ip4/<IP publik>/tcp/28800 atau /dns4/<nama>/tcp/443/tls/sni/<nama>/ws (tanpa /p2p);\n",
    "                      boleh berulang. Tanpa opsi ini node tidak mengumumkan dirinya\n",
    "  --p2p-ws-port <port>\n",
    "                      port P2P WebSocket (/ws) di 127.0.0.1, untuk seed di belakang proxy TLS seperti\n",
    "                      Cloudflare Tunnel (bawaan mati; 0 = port bebas)\n",
    "  --connect <addr>    rekan P2P, misalnya /ip4/127.0.0.1/tcp/28800/p2p/<PeerId> atau\n",
    "                      /dns4/<nama>/tcp/443/tls/sni/<nama>/ws/p2p/<PeerId>; boleh berulang\n",
    "  --seed-resmi        sambung juga ke seed resmi bawaan jaringan ini (DEC-43). Bawaan: regtest tidak\n",
    "                      (regtest juga dipakai untuk uji lokal), testnet dan mainnet ya\n",
    "  --tanpa-seed-resmi  jangan sambung ke seed resmi bawaan, misalnya untuk uji lokal testnet\n",
    "\n",
    "Setelah siap, alamat RPC dicetak ke stdout sebagai `rpc: http://<alamat>`, lalu alamat P2P sebagai\n",
    "`p2p: <multiaddr>`, dan alamat WebSocket sebagai `p2p-ws: <multiaddr>` jika --p2p-ws-port dipakai.\n",
    "Buku alamat rekan disimpan di <folder>/<jaringan>/peers.json; node yang dibuka ulang tersambung lagi dari sana.\n",
    "Log ditulis ke stderr.\n",
    "Node berhenti bersih dengan Ctrl-C (SIGINT) atau SIGTERM.",
);

/// Nama file chain di folder jaringan.
pub const CHAIN_FILE: &str = "chain.redb";

/// Seed resmi jaringan demo regtest (DEC-43, TSK-M73; `SUMBER_RESMI.md`): seed VPS lewat Cloudflare Tunnel, WebSocket dengan
/// TLS. `PeerId` di akhir alamat diperiksa Noise, jadi node tidak tersambung ke seed tiruan di alamat yang sama.
pub const REGTEST_SEEDS: &[&str] = &[concat!(
    "/dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws",
    "/p2p/12D3KooWPdd2VH92LVRwJNrokxKQJCerctJNksErJn5PB53kbEsD"
)];

/// Seed resmi testnet: kosong sampai lokasi seed testnet diputuskan pencipta (TSK-F06).
pub const TESTNET_SEEDS: &[&str] = &[];

/// Seed resmi bawaan jaringan `network` (DEC-43). Mainnet belum diluncurkan, jadi kosong.
#[must_use]
pub const fn official_seeds(network: Network) -> &'static [&'static str] {
    match network {
        Network::Regtest => REGTEST_SEEDS,
        Network::Testnet => TESTNET_SEEDS,
        Network::Mainnet => &[],
    }
}

/// Perintah hasil membaca argumen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Mencetak [`USAGE`].
    Help,
    /// Mencetak versi paket.
    Version,
    /// Menjalankan daemon.
    Run(Config),
}

/// Konfigurasi daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// Jaringan: regtest atau testnet.
    pub network: Network,
    /// Folder data.
    pub datadir: PathBuf,
    /// Port RPC di 127.0.0.1. `0` memilih port bebas.
    pub rpc_port: u16,
    /// Port P2P TCP. `0` memilih port bebas.
    pub p2p_port: u16,
    /// Alamat IP dengar P2P TCP (`--p2p-bind`, TSK-M75). Bawaan `127.0.0.1`; `0.0.0.0` atau `::` menerima sambungan masuk
    /// dari luar.
    pub p2p_bind: IpAddr,
    /// Alamat publik yang diumumkan ke rekan (`--p2p-publik`, TSK-M75), tanpa `/p2p`, urut seperti di argumen.
    pub public: Vec<Multiaddr>,
    /// Port P2P WebSocket (`/ws`) di 127.0.0.1, untuk seed di belakang proxy TLS (TSK-M45). `None` = tidak mendengar
    /// WebSocket; `0` memilih port bebas.
    pub p2p_ws_port: Option<u16>,
    /// Alamat rekan dari `--connect`, urut seperti di argumen.
    pub connect: Vec<Multiaddr>,
    /// Sambung juga ke [`official_seeds`] (DEC-43): bawaan regtest `false` (`--seed-resmi`), testnet dan mainnet `true`
    /// (`--tanpa-seed-resmi`).
    pub official_seeds: bool,
}

/// Argumen tidak sah. Biner keluar dengan kode 2.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ArgError {
    /// Argumen tidak dikenali.
    #[error("argumen tidak dikenali: {0}")]
    Unknown(String),
    /// Opsi butuh nilai, tetapi nilainya tidak ada, kosong, atau berupa opsi lain.
    #[error("opsi {0} butuh nilai")]
    MissingValue(&'static str),
    /// Opsi disebut lebih dari sekali.
    #[error("opsi {0} disebut lebih dari sekali")]
    Duplicate(&'static str),
    /// Port RPC bukan bilangan 0 sampai 65535.
    #[error("port RPC tidak sah: {0}")]
    InvalidPort(String),
    /// Port P2P bukan bilangan 0 sampai 65535.
    #[error("port P2P tidak sah: {0}")]
    InvalidP2pPort(String),
    /// Port P2P WebSocket bukan bilangan 0 sampai 65535.
    #[error("port P2P WebSocket tidak sah: {0}")]
    InvalidP2pWsPort(String),
    /// Nilai `--p2p-bind` bukan alamat IP.
    #[error("alamat dengar P2P tidak sah (wajib alamat IP, misalnya 0.0.0.0): {0}")]
    InvalidBind(String),
    /// Nilai `--p2p-publik` bukan multiaddr dengan `/tcp`, atau memuat `/p2p`.
    #[error("alamat publik tidak sah (multiaddr dengan /tcp, tanpa /p2p): {0}")]
    InvalidPublic(String),
    /// Nilai `--connect` bukan multiaddr.
    #[error("alamat rekan tidak sah: {0}")]
    InvalidAddress(String),
    /// Jaringan belum tersedia.
    #[error("jaringan {0} belum tersedia (mainnet diluncurkan sesudah testnet)")]
    NetworkUnavailable(&'static str),
    /// Jaringan tidak dipilih.
    #[error("jaringan wajib dipilih: --regtest atau --testnet")]
    MissingNetwork,
    /// Lebih dari satu jaringan dipilih.
    #[error("hanya satu jaringan boleh dipilih: --regtest atau --testnet")]
    NetworkConflict,
    /// `--seed-resmi` digabung dengan `--tanpa-seed-resmi`.
    #[error("--seed-resmi dan --tanpa-seed-resmi tidak boleh digabung")]
    SeedConflict,
    /// Folder data tidak diisi.
    #[error("folder data wajib diisi: --datadir <folder>")]
    MissingDatadir,
    /// `--help` atau `--version` digabung dengan opsi lain.
    #[error("--help dan --version tidak boleh digabung dengan opsi lain")]
    Exclusive,
}

/// Galat saat menjalankan daemon. Biner keluar dengan kode 1.
#[derive(Debug, Error)]
pub enum DaemonError {
    /// Folder data, runtime, atau pemasangan sinyal gagal.
    #[error("galat I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Parameter jaringan tidak bisa disusun.
    #[error("parameter jaringan tidak sah: {0}")]
    Params(#[from] ParamsError),
    /// Penyimpanan tidak bisa dibuka (misalnya dipakai node lain).
    #[error("penyimpanan gagal: {0}")]
    Store(#[from] StoreError),
    /// Node tidak bisa dibuka di atas penyimpanan.
    #[error("node gagal dibuka: {0}")]
    Node(#[from] NodeError),
    /// Data chain tidak bisa dibaca.
    #[error("data chain gagal dibaca: {0}")]
    Rpc(#[from] RpcError),
    /// Server RPC tidak bisa dijalankan (misalnya port terpakai).
    #[error("server RPC gagal: {0}")]
    Server(#[from] ServerError),
    /// Kunci rekan P2P tidak bisa dibaca atau dibuat.
    #[error("kunci rekan P2P gagal: {0}")]
    P2pKey(#[from] KeyError),
    /// Layanan P2P tidak bisa dijalankan (misalnya port terpakai).
    #[error("P2P gagal: {0}")]
    P2p(#[from] NetworkError),
    /// Alamat di [`official_seeds`] bukan multiaddr (kesalahan pembangunan, dicegah uji).
    #[error("alamat seed resmi tidak sah: {0}")]
    Seed(String),
}

/// Port P2P bawaan per jaringan (Spesifikasi P2P v0.1 Bagian 1).
#[must_use]
pub const fn default_p2p_port(network: Network) -> u16 {
    match network {
        Network::Mainnet => 8_800,
        Network::Testnet => 18_800,
        Network::Regtest => 28_800,
    }
}

/// Membaca nilai opsi berikutnya. Nilai kosong atau yang diawali `--` ditolak.
fn value_of(
    iter: &mut impl Iterator<Item = String>,
    option: &'static str,
) -> Result<String, ArgError> {
    match iter.next() {
        Some(value) if !value.is_empty() && !value.starts_with("--") => Ok(value),
        _ => Err(ArgError::MissingValue(option)),
    }
}

/// Mencatat pilihan jaringan dari `flag`. Opsi yang sama dua kali atau dua jaringan berbeda ditolak.
fn choose_network(
    slot: &mut Option<Network>,
    chosen: Network,
    flag: &'static str,
) -> Result<(), ArgError> {
    match *slot {
        Some(previous) if previous == chosen => Err(ArgError::Duplicate(flag)),
        Some(_) => Err(ArgError::NetworkConflict),
        None => {
            *slot = Some(chosen);
            Ok(())
        }
    }
}

/// Membaca alamat `--p2p-publik`: multiaddr dengan `/tcp` (langsung atau di bawah WebSocket), tanpa `/p2p` (`PeerId` node
/// ditambahkan sendiri).
fn parse_public(text: &str) -> Option<Multiaddr> {
    let address = text.parse::<Multiaddr>().ok()?;
    let shown = address.to_string();
    (shown.contains("/tcp/") && !shown.contains("/p2p/")).then_some(address)
}

/// Mencatat pilihan seed resmi dari `flag`. Opsi yang sama dua kali, atau `--seed-resmi` bersama `--tanpa-seed-resmi`,
/// ditolak.
fn choose_seeds(slot: &mut Option<bool>, chosen: bool, flag: &'static str) -> Result<(), ArgError> {
    match *slot {
        Some(previous) if previous == chosen => Err(ArgError::Duplicate(flag)),
        Some(_) => Err(ArgError::SeedConflict),
        None => {
            *slot = Some(chosen);
            Ok(())
        }
    }
}

/// Membaca argumen (tanpa nama program). Tanpa argumen sama dengan `--help`.
///
/// # Errors
///
/// [`ArgError`] untuk argumen yang tidak sah.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Command, ArgError> {
    let args: Vec<String> = args.into_iter().collect();
    match args.as_slice() {
        [] => return Ok(Command::Help),
        [only] if only.as_str() == "--help" || only.as_str() == "-h" => return Ok(Command::Help),
        [only] if only.as_str() == "--version" => return Ok(Command::Version),
        _ => {}
    }
    let mut network: Option<Network> = None;
    let mut datadir: Option<PathBuf> = None;
    let mut rpc_port: Option<u16> = None;
    let mut p2p_port: Option<u16> = None;
    let mut p2p_ws_port: Option<u16> = None;
    let mut p2p_bind: Option<IpAddr> = None;
    let mut public: Vec<Multiaddr> = Vec::new();
    let mut connect: Vec<Multiaddr> = Vec::new();
    let mut seeds: Option<bool> = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--regtest" => choose_network(&mut network, Network::Regtest, "--regtest")?,
            "--testnet" => choose_network(&mut network, Network::Testnet, "--testnet")?,
            "--mainnet" => return Err(ArgError::NetworkUnavailable("mainnet")),
            "--datadir" => {
                if datadir.is_some() {
                    return Err(ArgError::Duplicate("--datadir"));
                }
                datadir = Some(PathBuf::from(value_of(&mut iter, "--datadir")?));
            }
            "--rpc-port" => {
                if rpc_port.is_some() {
                    return Err(ArgError::Duplicate("--rpc-port"));
                }
                let value = value_of(&mut iter, "--rpc-port")?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| ArgError::InvalidPort(value.clone()))?;
                rpc_port = Some(port);
            }
            "--p2p-port" => {
                if p2p_port.is_some() {
                    return Err(ArgError::Duplicate("--p2p-port"));
                }
                let value = value_of(&mut iter, "--p2p-port")?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| ArgError::InvalidP2pPort(value.clone()))?;
                p2p_port = Some(port);
            }
            "--p2p-ws-port" => {
                if p2p_ws_port.is_some() {
                    return Err(ArgError::Duplicate("--p2p-ws-port"));
                }
                let value = value_of(&mut iter, "--p2p-ws-port")?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| ArgError::InvalidP2pWsPort(value.clone()))?;
                p2p_ws_port = Some(port);
            }
            "--p2p-bind" => {
                if p2p_bind.is_some() {
                    return Err(ArgError::Duplicate("--p2p-bind"));
                }
                let value = value_of(&mut iter, "--p2p-bind")?;
                let ip = value
                    .parse::<IpAddr>()
                    .map_err(|_| ArgError::InvalidBind(value.clone()))?;
                p2p_bind = Some(ip);
            }
            "--p2p-publik" => {
                let value = value_of(&mut iter, "--p2p-publik")?;
                let address =
                    parse_public(&value).ok_or_else(|| ArgError::InvalidPublic(value.clone()))?;
                public.push(address);
            }
            "--connect" => {
                let value = value_of(&mut iter, "--connect")?;
                let address = value
                    .parse::<Multiaddr>()
                    .map_err(|_| ArgError::InvalidAddress(value.clone()))?;
                connect.push(address);
            }
            "--seed-resmi" => choose_seeds(&mut seeds, true, "--seed-resmi")?,
            "--tanpa-seed-resmi" => choose_seeds(&mut seeds, false, "--tanpa-seed-resmi")?,
            "--help" | "-h" | "--version" => return Err(ArgError::Exclusive),
            _ => return Err(ArgError::Unknown(arg)),
        }
    }
    let network = network.ok_or(ArgError::MissingNetwork)?;
    let datadir = datadir.ok_or(ArgError::MissingDatadir)?;
    Ok(Command::Run(Config {
        network,
        datadir,
        rpc_port: rpc_port.unwrap_or(default_port(network)),
        p2p_port: p2p_port.unwrap_or(default_p2p_port(network)),
        p2p_bind: p2p_bind.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        public,
        p2p_ws_port,
        connect,
        official_seeds: seeds.unwrap_or(!matches!(network, Network::Regtest)),
    }))
}

/// Alamat rekan yang didial node: alamat `--connect` (urut argumen), lalu [`official_seeds`] jaringan itu bila
/// `official_seeds` aktif, tanpa duplikat.
///
/// # Errors
///
/// [`DaemonError::Seed`] jika alamat seed resmi bukan multiaddr.
pub fn dial_addresses(config: &Config) -> Result<Vec<Multiaddr>, DaemonError> {
    let mut addresses = config.connect.clone();
    if config.official_seeds {
        for seed in official_seeds(config.network) {
            let address = seed
                .parse::<Multiaddr>()
                .map_err(|_| DaemonError::Seed((*seed).to_owned()))?;
            if !addresses.contains(&address) {
                addresses.push(address);
            }
        }
    }
    Ok(addresses)
}

/// Nama folder jaringan di dalam folder data.
const fn network_dir(network: Network) -> &'static str {
    match network {
        Network::Mainnet => "mainnet",
        Network::Testnet => "testnet",
        Network::Regtest => "regtest",
    }
}

/// Path file chain: `<datadir>/<jaringan>/chain.redb`.
#[must_use]
pub fn chain_path(config: &Config) -> PathBuf {
    config
        .datadir
        .join(network_dir(config.network))
        .join(CHAIN_FILE)
}

/// Path kunci rekan P2P: `<datadir>/<jaringan>/p2p_key`.
#[must_use]
pub fn p2p_key_path(config: &Config) -> PathBuf {
    config
        .datadir
        .join(network_dir(config.network))
        .join(P2P_KEY_FILE)
}

/// Path buku alamat rekan: `<datadir>/<jaringan>/peers.json` (TSK-M74).
#[must_use]
pub fn peers_path(config: &Config) -> PathBuf {
    config
        .datadir
        .join(network_dir(config.network))
        .join(PEERS_FILE)
}

/// Alamat dengar P2P TCP `/ip4/<bind>/tcp/<port>` atau `/ip6/<bind>/tcp/<port>` (bawaan `bind` = `127.0.0.1`).
///
/// # Errors
///
/// [`NetworkError::Listen`] jika alamat tidak bisa disusun.
pub fn p2p_listen_address(bind: IpAddr, port: u16) -> Result<Multiaddr, NetworkError> {
    let text = match bind {
        IpAddr::V4(ip) => format!("/ip4/{ip}/tcp/{port}"),
        IpAddr::V6(ip) => format!("/ip6/{ip}/tcp/{port}"),
    };
    text.parse::<Multiaddr>()
        .map_err(|error| NetworkError::Listen {
            address: text.clone(),
            reason: error.to_string(),
        })
}

/// Alamat dengar P2P WebSocket `/ip4/127.0.0.1/tcp/<port>/ws` (TSK-M45). TLS diputus proxy di depannya (cloudflared).
///
/// # Errors
///
/// [`NetworkError::Listen`] jika alamat tidak bisa disusun.
pub fn p2p_ws_listen_address(port: u16) -> Result<Multiaddr, NetworkError> {
    let text = format!("/ip4/127.0.0.1/tcp/{port}/ws");
    text.parse::<Multiaddr>()
        .map_err(|error| NetworkError::Listen {
            address: text.clone(),
            reason: error.to_string(),
        })
}

/// Memasang logger ke stderr (level INFO, tanpa warna). Jika logger global sudah ada, logger lama tetap dipakai.
pub fn init_logging() {
    // Hanya gagal jika subscriber global sudah dipasang sebelumnya; itu tidak menghalangi daemon.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .try_init();
}

/// Jam sistem dalam detik Unix. Jam sebelum 1970 dianggap 0.
fn system_clock() -> Clock {
    Box::new(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs())
    })
}

/// Menunggu SIGINT atau SIGTERM. Penangkap sinyal dipasang saat fungsi ini dipanggil, sebelum server berjalan,
/// sehingga sinyal yang datang sesudahnya tidak hilang. Wajib dipanggil di dalam runtime tokio.
#[cfg(unix)]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut interrupt = signal(SignalKind::interrupt())?;
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(async move {
        tokio::select! {
            _ = interrupt.recv() => {}
            _ = terminate.recv() => {}
        }
    })
}

/// Menunggu Ctrl-C (bukan Unix).
#[cfg(not(unix))]
fn shutdown_signal() -> std::io::Result<impl Future<Output = ()>> {
    Ok(async {
        // Galat berarti sinyal tidak bisa dipasang; node berhenti seperti menerima Ctrl-C.
        let _ = tokio::signal::ctrl_c().await;
    })
}

/// Menjalankan server RPC dan P2P sampai sinyal berhenti diterima.
async fn serve<S: ChainStore + Send + 'static>(
    node: Arc<Node<S>>,
    config: &Config,
) -> Result<(), DaemonError> {
    let shutdown = shutdown_signal()?;
    let (sender, receiver) = mpsc::unbounded_channel();
    let relay = Arc::new(Relay::new(Arc::clone(&node), sender));
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, config.rpc_port));
    let server = RpcServer::start(addr, relay).await?;
    let local = server.local_addr();
    tracing::info!(%local, "server RPC berjalan");
    println!("rpc: http://{local}");

    let keypair = load_or_create(&p2p_key_path(config))?;
    let mut p2p_config = NetworkConfig::new(node.params().network_id());
    p2p_config.listen = vec![p2p_listen_address(config.p2p_bind, config.p2p_port)?];
    if let Some(port) = config.p2p_ws_port {
        p2p_config.listen.push(p2p_ws_listen_address(port)?);
    }
    p2p_config.connect = dial_addresses(config)?;
    p2p_config.public = config.public.clone();
    p2p_config.peers_file = Some(peers_path(config));
    // Regtest juga dipakai untuk uji beberapa node di satu mesin, jadi alamat lokal diterima (DEC-43).
    p2p_config.allow_local = matches!(config.network, Network::Regtest);
    if !config.p2p_bind.is_loopback() {
        tracing::warn!(
            bind = %config.p2p_bind,
            "P2P menerima sambungan masuk dari luar: IP Anda terlihat oleh rekan; izinkan port P2P di firewall"
        );
    }
    for address in &config.public {
        tracing::info!(%address, "alamat publik diumumkan ke rekan");
    }
    if config.official_seeds {
        let seeds = official_seeds(config.network).len();
        if seeds == 0 {
            tracing::warn!(
                "jaringan ini belum punya seed resmi bawaan; tambahkan rekan dengan --connect"
            );
        } else {
            tracing::info!(seeds, "seed resmi bawaan dipakai");
        }
    }
    let network = P2pNetwork::new(keypair, p2p_config, local_status(&node)?)?;
    let p2p = P2pService::new(Arc::clone(&node), network).run(receiver);
    tokio::select! {
        () = shutdown => {}
        () = p2p => {}
    }
    tracing::info!("sinyal berhenti diterima");
    server.stop().await;
    tracing::info!("node berhenti");
    Ok(())
}

/// Menjalankan daemon sampai menerima SIGINT atau SIGTERM.
///
/// # Errors
///
/// [`DaemonError`] jika folder data, penyimpanan, node, runtime, sinyal, atau server RPC gagal.
pub fn run(config: &Config) -> Result<(), DaemonError> {
    let path = chain_path(config);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let store = RedbStore::create(&path)?;
    let params = ChainParams::for_network(config.network)?;
    let node = Arc::new(Node::open(
        store,
        params,
        DEFAULT_MAX_BYTES,
        system_clock(),
    )?);
    let info = node.chain_info()?;
    tracing::info!(
        height = info.height,
        path = %path.display(),
        network = network_dir(config.network),
        "chain dibuka"
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(Arc::clone(&node), config))
}

#[cfg(test)]
mod tests;
