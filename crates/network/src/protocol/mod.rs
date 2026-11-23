//! Nama protokol P2P (Spesifikasi P2P Bagian 2): `/zvrg/<net>/<nama>/1`, dengan `<net>` = 16 karakter hex huruf kecil
//! pertama `network_id`. Rekan dari jaringan lain gagal bernegosiasi protokol, sehingga tidak bisa membuka aliran.

use libp2p::StreamProtocol;
use libp2p::swarm::InvalidProtocol;
use zevrigo_primitives::Hash32;

/// Panjang awalan hex `network_id` di nama protokol.
const NETWORK_PREFIX_LEN: usize = 16;

/// Enam protokol request-response P2P v0.1 (`addr` sejak TSK-M74, DEC-43).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProtocolKind {
    /// `status`: `Status` → `Status`.
    Status,
    /// `headers`: `GetHeaders` → `Headers`.
    Headers,
    /// `blocks`: `GetBlocks` → `Blocks`.
    Blocks,
    /// `block`: `Block` → `Ack`.
    Block,
    /// `tx`: `Transaction` → `Ack`.
    Tx,
    /// `addr`: `GetAddr` → `Addrs` (penemuan rekan, TSK-M74).
    Addr,
}

impl ProtocolKind {
    /// Keenam protokol, urut seperti Spesifikasi P2P Bagian 2.
    pub const ALL: [Self; 6] = [
        Self::Status,
        Self::Headers,
        Self::Blocks,
        Self::Block,
        Self::Tx,
        Self::Addr,
    ];

    /// Nama pendek protokol.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Headers => "headers",
            Self::Blocks => "blocks",
            Self::Block => "block",
            Self::Tx => "tx",
            Self::Addr => "addr",
        }
    }
}

/// 16 karakter hex huruf kecil pertama `network_id`.
#[must_use]
pub fn network_prefix(network_id: &Hash32) -> String {
    let hex = network_id.to_string();
    hex.get(..NETWORK_PREFIX_LEN).unwrap_or(&hex).to_owned()
}

/// Nama lengkap protokol, misalnya `/zvrg/8b9eb3187250bbdf/status/1` untuk regtest.
#[must_use]
pub fn protocol_name(network_id: &Hash32, kind: ProtocolKind) -> String {
    format!("/zvrg/{}/{}/1", network_prefix(network_id), kind.name())
}

/// Nama protokol sebagai `StreamProtocol` libp2p.
pub(crate) fn stream_protocol(
    network_id: &Hash32,
    kind: ProtocolKind,
) -> Result<StreamProtocol, InvalidProtocol> {
    StreamProtocol::try_from_owned(protocol_name(network_id, kind))
}

#[cfg(test)]
mod tests;
