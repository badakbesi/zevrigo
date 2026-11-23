//! MOD-06: P2P Zevrigo Coin di atas rust-libp2p 0.57 (DEC-24), sesuai Spesifikasi P2P v0.1.
//!
//! - [`message`]: pesan P2P dan encoding kanonisnya (Bagian 3), termasuk `GetAddr` dan `Addrs` (TSK-M74).
//! - [`frame`]: bingkai `varint<32>(panjang) ‖ isi`, paling banyak 2 MiB (Bagian 3).
//! - [`protocol`]: nama protokol `/zvrg/<net>/<nama>/1` (Bagian 2), enam protokol termasuk `addr`.
//! - [`key`]: kunci identitas rekan Ed25519 di `<datadir>/<jaringan>/p2p_key` (Bagian 1).
//! - [`ban`]: blokir 24 jam per `PeerId` dan per alamat IP (Bagian 7).
//! - [`addrbook`]: buku alamat rekan `peers.json` (tabel baru dan teruji, kelompok jaringan, batas per sumber; DEC-43,
//!   TSK-M74, Bagian 12).
//! - [`service`]: swarm libp2p (TCP atau WebSocket, Noise, Yamux), lima protokol request-response, batas koneksi,
//!   sambung ulang, pertukaran `status` saat terhubung dan setiap 60 detik (keepalive), sanksi, serta penemuan rekan
//!   (`addr`, koneksi keluar dari buku alamat, alamat publik; TSK-M74, TSK-M75).
//! - `transport`: TCP yang juga mendial alamat `/dns4`, `/dns6`, dan `/dns` (nama di-resolve setiap dial, tanpa
//!   `libp2p-dns`), dasar transport WebSocket untuk seed di belakang proxy TLS (TSK-M45, DEC-33), serta
//!   [`is_websocket`].
//!
//! Keputusan sinkronisasi, pemilihan chain, dan validasi blok atau transaksi ada di MOD-11 dan MOD-04 (TSK-M28).

#![forbid(unsafe_code)]

pub mod addrbook;
pub mod ban;
mod codec;
pub mod frame;
pub mod key;
pub mod message;
pub mod protocol;
pub mod service;
mod transport;

pub use addrbook::{AddressBook, MAX_ENTRIES, PEERS_FILE};
pub use ban::{BAN_DURATION, BanList};
pub use key::{KeyError, P2P_KEY_FILE, keypair_from_secret, load_or_create};
pub use libp2p::identity::Keypair;
pub use libp2p::{Multiaddr, PeerId};
pub use message::{
    Ack, Addrs, Blocks, GetAddr, GetBlocks, GetHeaders, Headers, MAX_ADDRS, MAX_MESSAGE_SIZE,
    MessageError, Status, Wire,
};
pub use protocol::{ProtocolKind, protocol_name};
pub use service::{
    ADDR_INTERVAL, NetEvent, Network, NetworkConfig, NetworkError, Offense, Reply, RequestId,
};
pub use transport::is_websocket;

/// Identitas modul sesuai peta arsitektur di `AGENTS.md`.
pub const MODULE_ID: &str = "MOD-06";

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
