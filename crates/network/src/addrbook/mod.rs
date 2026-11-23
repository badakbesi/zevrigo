//! Buku alamat rekan (DEC-43, TSK-M74; Spesifikasi P2P Bagian 12): tabel **baru** (alamat yang didengar dari rekan) dan
//! tabel **teruji** (alamat yang pernah berhasil didial), disimpan di `<datadir>/<jaringan>/peers.json` sehingga node tidak
//! bergantung pada seed setiap kali dibuka.
//!
//! Perlindungan eclipse:
//! - alamat dikelompokkan per jaringan ([`group_of`]: IPv4 `/16`, IPv6 `/32`, nama DNS per domain); dari satu sumber paling
//!   banyak [`MAX_PER_GROUP_PER_SOURCE`] alamat per kelompok dan [`MAX_PER_SOURCE`] alamat seluruhnya, jadi satu rekan tidak
//!   bisa memenuhi buku alamat;
//! - koneksi keluar baru dipilih acak ([`AddressBook::pick`]) dari rekan dan kelompok yang belum dipakai koneksi keluar;
//! - alamat yang [`MAX_FAILURES`] kali berturut-turut gagal didial dibuang;
//! - alamat loopback dan privat ([`is_local`]) hanya diterima jika diizinkan (regtest, uji lokal); masing-masing menjadi
//!   kelompok sendiri.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::net::IpAddr;
use std::path::Path;
use std::time::{Duration, Instant};

use libp2p::multiaddr::Protocol;
use libp2p::{Multiaddr, PeerId};
use serde_json::{Value, json};

use crate::message::MAX_ADDR_LEN;
use crate::service::{ip_of, peer_of, strip_p2p};

/// Nama file buku alamat di folder jaringan (`<datadir>/<jaringan>/peers.json`).
pub const PEERS_FILE: &str = "peers.json";

/// Versi format `peers.json`.
pub const PEERS_VERSION: u64 = 1;

/// Alamat paling banyak di buku alamat (baru dan teruji).
pub const MAX_ENTRIES: usize = 2_000;

/// Alamat teruji paling banyak; alamat lain yang berhasil didial tetap di tabel baru.
pub const MAX_TRIED: usize = 500;

/// Alamat paling banyak per kelompok jaringan dari satu sumber.
pub const MAX_PER_GROUP_PER_SOURCE: usize = 8;

/// Alamat paling banyak dari satu sumber.
pub const MAX_PER_SOURCE: usize = 64;

/// Gagal didial berturut-turut sebelum alamat dibuang.
pub const MAX_FAILURES: u8 = 5;

/// Sumber alamat yang langsung berhasil didial (seed, `--connect`).
const SOURCE_DIALED: &str = "didial";

/// Sumber alamat yang dimuat dari `peers.json`.
const SOURCE_FILE: &str = "berkas";

/// `true` jika `ip` loopback, privat, link-local, tidak ditentukan, atau broadcast (tidak bisa dicapai dari internet).
#[must_use]
pub fn is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
        }
        IpAddr::V6(ip) => {
            let [first, ..] = ip.segments();
            ip.is_loopback()
                || ip.is_unspecified()
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Dua label terakhir nama DNS, huruf kecil (`demo-seed-p2p.zevrigo.online` menjadi `zevrigo.online`).
fn domain_of(name: &str) -> String {
    let name = name.trim_end_matches('.').to_ascii_lowercase();
    let mut labels: Vec<&str> = name.rsplit('.').take(2).collect();
    labels.reverse();
    labels.join(".")
}

/// Kelompok jaringan alamat: `ip4:a.b` (`/16`), `ip6:x:y` (`/32`), `dns:<domain>`, `lokal:<alamat tanpa /p2p>` untuk IP
/// lokal (setiap alamat kelompok sendiri, untuk uji di satu mesin), atau `lain`.
#[must_use]
pub fn group_of(address: &Multiaddr) -> String {
    for part in address.iter() {
        match part {
            Protocol::Ip4(ip) => {
                if is_local(IpAddr::V4(ip)) {
                    return format!("lokal:{}", strip_p2p(address));
                }
                let [a, b, _, _] = ip.octets();
                return format!("ip4:{a}.{b}");
            }
            Protocol::Ip6(ip) => {
                if is_local(IpAddr::V6(ip)) {
                    return format!("lokal:{}", strip_p2p(address));
                }
                let [a, b, ..] = ip.segments();
                return format!("ip6:{a:x}:{b:x}");
            }
            Protocol::Dns(name)
            | Protocol::Dns4(name)
            | Protocol::Dns6(name)
            | Protocol::Dnsaddr(name) => {
                return format!("dns:{}", domain_of(&name));
            }
            _ => {}
        }
    }
    "lain".to_owned()
}

/// Satu alamat di buku alamat.
#[derive(Clone, Debug)]
struct Entry {
    /// Alamat lengkap dengan `/p2p/<PeerId>`.
    address: Multiaddr,
    /// `PeerId` di alamat.
    peer: PeerId,
    /// Kelompok jaringan alamat ([`group_of`]).
    group: String,
    /// Kelompok jaringan rekan yang memberi tahu alamat ini, atau sumber khusus (didial, berkas).
    source: String,
    /// `true` jika pernah berhasil didial (tabel teruji).
    tried: bool,
    /// Gagal didial berturut-turut.
    failures: u8,
    /// Percobaan dial terakhir (hanya di memori).
    last_attempt: Option<Instant>,
}

/// Buku alamat rekan.
#[derive(Clone, Debug)]
pub struct AddressBook {
    allow_local: bool,
    entries: BTreeMap<String, Entry>,
}

impl AddressBook {
    /// Buku alamat kosong. `allow_local` menerima alamat loopback dan privat (regtest).
    #[must_use]
    pub const fn new(allow_local: bool) -> Self {
        Self {
            allow_local,
            entries: BTreeMap::new(),
        }
    }

    /// Jumlah alamat.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` jika buku alamat kosong.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Jumlah alamat teruji.
    #[must_use]
    pub fn tried_len(&self) -> usize {
        self.entries.values().filter(|entry| entry.tried).count()
    }

    /// `true` jika `address` ada di buku alamat.
    #[must_use]
    pub fn contains(&self, address: &Multiaddr) -> bool {
        self.entries.contains_key(&address.to_string())
    }

    /// `true` jika `address` ada di tabel teruji.
    #[must_use]
    pub fn is_tried(&self, address: &Multiaddr) -> bool {
        self.entries
            .get(&address.to_string())
            .is_some_and(|entry| entry.tried)
    }

    /// `PeerId` semua alamat.
    #[must_use]
    pub fn peers(&self) -> BTreeSet<PeerId> {
        self.entries.values().map(|entry| entry.peer).collect()
    }

    /// `PeerId` alamat teruji.
    #[must_use]
    pub fn tried_peers(&self) -> BTreeSet<PeerId> {
        self.entries
            .values()
            .filter(|entry| entry.tried)
            .map(|entry| entry.peer)
            .collect()
    }

    /// `PeerId` di `address` jika alamat boleh masuk buku: diakhiri `/p2p/<PeerId>`, memakai TCP (langsung atau di bawah
    /// WebSocket), encoding biner paling panjang 255 byte, dan bukan IP lokal kecuali diizinkan.
    #[must_use]
    pub fn acceptable(&self, address: &Multiaddr) -> Option<PeerId> {
        let peer = peer_of(address)?;
        let tcp = address.iter().any(|part| matches!(part, Protocol::Tcp(_)));
        let local = ip_of(address).is_some_and(is_local);
        let short = address.to_vec().len() <= MAX_ADDR_LEN;
        (tcp && short && (self.allow_local || !local)).then_some(peer)
    }

    /// Menambah alamat yang didengar dari rekan berkelompok `source` ke tabel baru. `false` jika alamat tidak diterima,
    /// sudah ada, batas sumber tercapai, atau buku penuh tanpa alamat baru yang bisa dibuang.
    pub fn add_heard(&mut self, address: &Multiaddr, source: &str) -> bool {
        let Some(peer) = self.acceptable(address) else {
            return false;
        };
        let key = address.to_string();
        if self.entries.contains_key(&key) {
            return false;
        }
        let group = group_of(address);
        let mut total = 0_usize;
        let mut same_group = 0_usize;
        for entry in self.entries.values().filter(|entry| entry.source == source) {
            total = total.saturating_add(1);
            if entry.group == group {
                same_group = same_group.saturating_add(1);
            }
        }
        if total >= MAX_PER_SOURCE || same_group >= MAX_PER_GROUP_PER_SOURCE {
            return false;
        }
        if self.entries.len() >= MAX_ENTRIES && !self.evict_one() {
            return false;
        }
        self.entries.insert(
            key,
            Entry {
                address: address.clone(),
                peer,
                group,
                source: source.to_owned(),
                tried: false,
                failures: 0,
                last_attempt: None,
            },
        );
        true
    }

    /// Membuang satu alamat baru (bukan teruji) dengan gagal terbanyak. `false` jika tidak ada.
    fn evict_one(&mut self) -> bool {
        let victim = self
            .entries
            .iter()
            .filter(|(_, entry)| !entry.tried)
            .max_by_key(|(_, entry)| entry.failures)
            .map(|(key, _)| key.clone());
        victim.is_some_and(|key| self.entries.remove(&key).is_some())
    }

    /// `address` berhasil didial: pindah ke tabel teruji (selama di bawah [`MAX_TRIED`]) dan hitungan gagal kembali 0.
    /// Alamat yang belum ada (seed, `--connect`) ditambahkan. `true` jika buku berubah.
    pub fn mark_success(&mut self, address: &Multiaddr) -> bool {
        let key = address.to_string();
        let room = self.tried_len() < MAX_TRIED;
        if let Some(entry) = self.entries.get_mut(&key) {
            let changed = entry.failures != 0 || (!entry.tried && room);
            entry.failures = 0;
            if room {
                entry.tried = true;
            }
            return changed;
        }
        let Some(peer) = self.acceptable(address) else {
            return false;
        };
        if self.entries.len() >= MAX_ENTRIES && !self.evict_one() {
            return false;
        }
        self.entries.insert(
            key,
            Entry {
                address: address.clone(),
                peer,
                group: group_of(address),
                source: SOURCE_DIALED.to_owned(),
                tried: room,
                failures: 0,
                last_attempt: None,
            },
        );
        true
    }

    /// `address` gagal didial. Sesudah [`MAX_FAILURES`] kali berturut-turut alamat dibuang. `true` jika alamat ada.
    pub fn mark_failure(&mut self, address: &Multiaddr) -> bool {
        let key = address.to_string();
        let Some(entry) = self.entries.get_mut(&key) else {
            return false;
        };
        entry.failures = entry.failures.saturating_add(1);
        if entry.failures >= MAX_FAILURES {
            self.entries.remove(&key);
        }
        true
    }

    /// Mencatat percobaan dial ke `address` pada `now`.
    pub fn mark_attempt(&mut self, address: &Multiaddr, now: Instant) {
        if let Some(entry) = self.entries.get_mut(&address.to_string()) {
            entry.last_attempt = Some(now);
        }
    }

    /// Menghapus semua alamat milik `peer` (misalnya node ini sendiri).
    pub fn remove_peer(&mut self, peer: &PeerId) {
        self.entries.retain(|_, entry| entry.peer != *peer);
    }

    /// Memilih alamat untuk koneksi keluar baru: bukan rekan di `busy_peers`, bukan kelompok di `busy_groups`, dan tidak
    /// dicoba dalam `retry` terakhir. Bit terendah `random` memilih tabel (teruji atau baru, jika keduanya ada), sisanya
    /// memilih alamat di tabel itu.
    #[must_use]
    pub fn pick(
        &self,
        now: Instant,
        retry: Duration,
        busy_peers: &BTreeSet<PeerId>,
        busy_groups: &BTreeSet<String>,
        random: u64,
    ) -> Option<Multiaddr> {
        let ready = |entry: &&Entry| {
            !busy_peers.contains(&entry.peer)
                && !busy_groups.contains(&entry.group)
                && entry
                    .last_attempt
                    .is_none_or(|last| now.saturating_duration_since(last) >= retry)
        };
        let tried: Vec<&Entry> = self
            .entries
            .values()
            .filter(|entry| entry.tried)
            .filter(ready)
            .collect();
        let fresh: Vec<&Entry> = self
            .entries
            .values()
            .filter(|entry| !entry.tried)
            .filter(ready)
            .collect();
        let pool = if ((random & 1) == 0 && !tried.is_empty()) || fresh.is_empty() {
            tried
        } else {
            fresh
        };
        let len = u64::try_from(pool.len()).ok().filter(|len| *len > 0)?;
        let index = usize::try_from((random >> 1) % len).ok()?;
        pool.get(index).map(|entry| entry.address.clone())
    }

    /// Paling banyak `limit` alamat teruji untuk jawaban `Addrs`, mulai dari posisi acak `random`.
    #[must_use]
    pub fn shareable(&self, limit: usize, random: u64) -> Vec<Multiaddr> {
        let tried: Vec<&Multiaddr> = self
            .entries
            .values()
            .filter(|entry| entry.tried)
            .map(|entry| &entry.address)
            .collect();
        let Some(len) = u64::try_from(tried.len()).ok().filter(|len| *len > 0) else {
            return Vec::new();
        };
        let start = usize::try_from(random % len).unwrap_or(0);
        tried
            .iter()
            .cycle()
            .skip(start)
            .take(limit.min(tried.len()))
            .map(|address| (*address).clone())
            .collect()
    }

    /// Isi `peers.json`: `{"versi": 1, "alamat": [{"alamat": "<multiaddr>", "teruji": bool, "gagal": n}, ...]}`.
    #[must_use]
    pub fn to_json(&self) -> String {
        let entries: Vec<Value> = self
            .entries
            .values()
            .map(|entry| {
                json!({
                    "alamat": entry.address.to_string(),
                    "teruji": entry.tried,
                    "gagal": entry.failures,
                })
            })
            .collect();
        let document = json!({ "versi": PEERS_VERSION, "alamat": entries });
        serde_json::to_string_pretty(&document).unwrap_or_default()
    }

    /// Membaca isi `peers.json`. `None` jika bukan JSON atau versinya lain; alamat yang tidak sah dilewati.
    #[must_use]
    pub fn from_json(text: &str, allow_local: bool) -> Option<Self> {
        let document: Value = serde_json::from_str(text).ok()?;
        if document.get("versi")?.as_u64()? != PEERS_VERSION {
            return None;
        }
        let mut book = Self::new(allow_local);
        for item in document.get("alamat")?.as_array()? {
            let Some(address) = item
                .get("alamat")
                .and_then(Value::as_str)
                .and_then(|text| text.parse::<Multiaddr>().ok())
            else {
                continue;
            };
            let tried = item.get("teruji").and_then(Value::as_bool).unwrap_or(false);
            let failures = item
                .get("gagal")
                .and_then(Value::as_u64)
                .map_or(0, |count| u8::try_from(count).unwrap_or(u8::MAX));
            book.insert_loaded(address, tried, failures);
        }
        Some(book)
    }

    fn insert_loaded(&mut self, address: Multiaddr, tried: bool, failures: u8) {
        if self.entries.len() >= MAX_ENTRIES || failures >= MAX_FAILURES {
            return;
        }
        let Some(peer) = self.acceptable(&address) else {
            return;
        };
        let key = address.to_string();
        if self.entries.contains_key(&key) {
            return;
        }
        let tried = tried && self.tried_len() < MAX_TRIED;
        self.entries.insert(
            key,
            Entry {
                group: group_of(&address),
                address,
                peer,
                source: SOURCE_FILE.to_owned(),
                tried,
                failures,
                last_attempt: None,
            },
        );
    }

    /// Memuat `path`. File yang tidak ada atau rusak menghasilkan buku kosong (alamat dikumpulkan lagi dari seed).
    #[must_use]
    pub fn load(path: &Path, allow_local: bool) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Self::from_json(&text, allow_local))
            .unwrap_or_else(|| Self::new(allow_local))
    }

    /// Menyimpan ke `path` lewat file sementara `<path>.tmp` lalu rename, sehingga file lama tidak pernah setengah tertulis.
    ///
    /// # Errors
    ///
    /// Galat I/O saat menulis atau rename.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, self.to_json())?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests;
