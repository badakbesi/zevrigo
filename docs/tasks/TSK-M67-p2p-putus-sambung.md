# TSK-M67 · Perbaikan P2P: putus di tengah pesan, rekan `--connect`, mempool saat tersambung · 🟡 · Prioritas: Tinggi
- **Jenis:** task modul · **Modul:** MOD-06 (`frame`, `codec`, `service`), MOD-11 (`p2p`, `chain`), MOD-17 (`ftr04_multi_node`)
- **Keputusan:** DEC-41 · **Bergantung pada:** TSK-M14, TSK-M28, TSK-M45
- **Ditulis:** Quick, 20 November 2026 (pencipta: "lanjut perbaikan P2P dulu")

## Insiden (20 November 2026, jaringan demo regtest)
Pukul 04:31:16 UTC node penambang dan node treasury di PC, pada detik yang sama, memberi sanksi kepada seed VPS
(`BadMessage(Truncated)`) dan memblokirnya 24 jam. Seed adalah satu-satunya rekan keduanya (`--connect`), jadi blok dari
penambang tidak sampai ke jaringan publik: publik berhenti di tinggi 384, PC menambang sampai 531. Dipulihkan dengan
menyalakan ulang node PC (blokir hanya di memori, Q166), tanpa reorg. Dua transaksi tetap tertahan di mempool node VPS karena
transaksi hanya disiarkan saat pertama masuk mempool; dibuang dengan menyalakan ulang node VPS (Q167).

Sebab di kode: `frame::read_frame` melaporkan aliran yang berakhir sebelum pesan lengkap sebagai `MessageError::Truncated`,
yang disanksi seperti pesan rusak. Aliran seperti itu terjadi saat koneksi putus di tengah pesan (Cloudflare Tunnel), dan
juga saat rekan menutup aliran tanpa menjawab (misalnya `Reply` dibuang karena galat).

## Perubahan
| Bagian | Isi |
|---|---|
| MOD-06 `frame` | Akhir aliran = galat I/O `UnexpectedEof`; varian `MessageError::Truncated` dihapus. request-response melaporkannya sebagai permintaan yang gagal (`RequestFailed`), tanpa sanksi |
| MOD-06 `service` | Rekan operator (`PeerId` di alamat `--connect`, atau koneksi yang didial ke alamat itu) tidak diblokir: hanya diputus, lalu disambung ulang pada selang berikutnya. `NetEvent::PeerPunished { peer, offense, banned }` |
| MOD-11 `p2p` | Log sanksi membedakan diblokir dan diputus saja. Isi mempool (`MEMPOOL_ON_CONNECT` = 64 transaksi, urut kedatangan) dikirim sekali per koneksi ke rekan yang `status`-nya menunjukkan ujung sama atau lebih berat |
| MOD-11 `chain` | `Node::mempool_transactions(limit)` |
| Dokumen | Spesifikasi P2P Bagian 3, 4 (langkah 5), 6, 7, 10, 11; DEC-41 |

## Uji
- `frame`: aliran kosong, awalan terpotong, dan isi terpotong menghasilkan galat I/O `UnexpectedEof`.
- `service`: rekan masuk yang mengirim `Blocks` dengan jumlah entri salah tetap diblokir; rekan `--connect` yang sama hanya
  diputus (`banned = false`), tidak diblokir, lalu tersambung lagi dan bertukar `status`.
- MOD-17 `ftr04_multi_node::mempool_reaches_peer_that_connects_later`: transaksi masuk mempool A saat B terputus; B
  tersambung lagi dengan ujung yang sama dan menerima transaksi itu tanpa blok baru; B menambang blok 100 berisi transaksi itu.
- Gerbang: `make check-all` repo inti, `make check-all` zevrigo-explorer (uji FTR-05 memakai node).

## Sesudahnya
Image Docker baru dari repo inti ini dipasang di PC (penambang, treasury) dan VPS (seed, faucet, pengamat), lewat skrip
pencipta (pola Q159). Sampai itu, sanksi `Truncated` dari seed bisa terulang (pemulihan: Q166).
