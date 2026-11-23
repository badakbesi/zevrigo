# TSK-M74 · Penemuan rekan · 🟢 · Prioritas: Tinggi
- **Jenis:** task modul · **Modul:** MOD-06 (`message`, `protocol`, `addrbook`, `service`), MOD-11 (`daemon`, `p2p`), MOD-17
- **Bergantung pada:** TSK-M73 (seed resmi bawaan), TSK-M67 (rekan operator tanpa blokir, DEC-41)
- **Keputusan:** DEC-43 (pencipta 23 November 2026)
- **Ditulis:** Quick, 23 November 2026

## Masalah
Sesudah TSK-M73 node tersambung sendiri ke seed, tetapi hanya ke seed dan alamat `--connect`: jaringan berbentuk bintang, dan
seed mati berarti node terpisah. DEC-43 memilih protokol keenam sendiri (`addr`, pola `getaddr`/`addr` Bitcoin) dengan buku
alamat dan perlindungan eclipse, bukan Kademlia (paket baru, DHT lebih mudah diracuni).

## Perubahan
| Bagian | Isi |
|---|---|
| `message::GetAddr`, `message::Addrs` | `GetAddr` kosong; `Addrs` = `vec<varint(panjang) ‖ multiaddr biner>`, paling banyak 250, panjang 1 sampai 255 byte, multiaddr sah dan kanonis. Galat baru `TooManyAddrs`, `AddrLength`, `BadAddress` (pesan rusak, disanksi) |
| `protocol::ProtocolKind::Addr` | `/zvrg/<net>/addr/1`, protokol keenam |
| `addrbook::AddressBook` (baru) | Tabel baru dan teruji (2_000 / 500), `group_of` (IPv4 `/16`, IPv6 `/32`, domain DNS, IP lokal per alamat), batas 8 per kelompok dan 64 seluruhnya per sumber, buang alamat baru dengan gagal terbanyak saat penuh, buang sesudah 5 gagal, `pick` acak dari rekan dan kelompok yang belum dipakai, `shareable` (hanya teruji), `peers.json` (`to_json`/`from_json`/`load`/`save`, lewat file `.tmp` lalu rename) |
| `service::Network` | `GetAddr` saat terhubung dan setiap `addr_interval` (10 menit); menjawab `Addrs` (alamat publik, alamat diri rekan lain yang terhubung, alamat teruji; batas separuh selang per penanya); alamat yang berhasil didial masuk tabel teruji; dial dari buku alamat setiap selang sambung ulang (paling banyak 2, sampai 8 dikurangi jangkar); dial gagal dihitung; buku disimpan setiap selang bila berubah dan saat `Network` dibuang. `NetworkConfig`: `public`, `peers_file`, `allow_local`, `addr_interval`. Rekan versi lama tanpa `addr` tetap tersambung |
| MOD-11 | `peers_path` (`<datadir>/<jaringan>/peers.json`), regtest `allow_local`; `ProtocolKind::Addr` di `P2pService::on_failure` (tidak pernah dilaporkan) |
| Dependensi | `serde_json` di MOD-06 (DEC-10 bagian 2, sudah ada di Cargo.lock lewat MOD-08 dan MOD-11): Cargo.lock hanya menambah satu baris dependensi `zevrigo-network`, tanpa paket baru |

Bukan perubahan konsensus, genesis, atau `network_id`. Rincian: Spesifikasi P2P Bagian 2, 3, 4, 9, 12.

## Uji
- `message::tests`: `GetAddr` kosong, `Addrs` encoding, 250 alamat, 251 ditolak, panjang 0, byte bukan multiaddr, panjang
  melebihi sisa.
- `addrbook::tests`: kelompok jaringan, alamat lokal, syarat alamat, batas per sumber, buku penuh, gagal dan berhasil, `pick`
  (rekan dan kelompok sibuk, jeda coba ulang, tabel teruji), `shareable`, JSON, simpan/muat, file rusak.
- `service::tests`: node B yang hanya mengenal hub tersambung langsung dengan node A dari alamat yang diteruskan hub, dan
  buku alamatnya tersimpan; tanpa `allow_local` alamat 127.0.0.1 tidak masuk buku.
- MOD-17 `ftr04_peer_discovery`: lima node, hanya seed S dikenal; empat node saling tersambung langsung, blok tetap tersiar
  sesudah S mati, node yang dibuka ulang tanpa `--connect` tersambung dari `peers.json` dan mengejar. FTR-02 sampai FTR-04 tetap
  lolos.
- Ujung ke ujung (Q190): tiga daemon regtest di satu mesin; node B yang hanya mengenal S tersambung ke A, `peers.json` B memuat A.
