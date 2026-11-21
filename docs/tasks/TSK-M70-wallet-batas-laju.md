# TSK-M70 · Wallet tahan batas laju gerbang RPC · 🟡 · Prioritas: Tinggi
- **Jenis:** task modul · **Modul:** MOD-12 (`commands`), dipakai MOD-15 (wallet 0.3.1)
- **Bergantung pada:** TSK-M41 (riwayat), TSK-M53 (gerbang RPC, batas laju)
- **Ditulis:** Quick, 21 November 2026 (pencipta 00.21: "A sekarang, lalu B"; 08.40: lanjut)

## Masalah (21 November 2026)
Wallet Windows 0.3.0 yang baru dipasang menampilkan "RPC gagal: Request rejected `429`". Gerbang RPC demo membatasi setiap
pengunjung (`--rate 20`: 20 permintaan per detik, kapasitas 80). Riwayat wallet memindai blok dari genesis dengan 2 permintaan
per blok tanpa jeda; koneksi cepat melewati batas, dan karena tembolok riwayat baru disimpan di akhir, setiap refresh mulai lagi
dari genesis. Perbaikan A (Q173): gerbang `--rate 60`. Perbaikan B (task ini) di pustaka wallet.

## Perubahan
| Bagian | Isi |
|---|---|
| MOD-12 `commands` | `Cli::retry`: panggilan RPC yang dijawab `429` (`is_rate_limited`) diulang sesudah `RATE_LIMIT_WAIT` (1 detik), paling banyak `RATE_LIMIT_RETRIES` (8) kali; galat lain langsung dikembalikan. Dipakai semua panggilan `Cli` (info, scan_utxos, scan_mempool, get_block_hash, get_block, send_raw_transaction) |
| MOD-12 `history` | Tembolok `history.json` dan indeks alamat terpakai disimpan setiap `HISTORY_SAVE_EVERY` (100) blok selama pemindaian (`save_history`), sehingga pemindaian yang gagal dilanjutkan dari blok tersimpan terakhir |
| MOD-15 (repo wallet) | Saldo tetap tampil bila riwayat gagal; galat riwayat ditampilkan terpisah (versi 0.3.1) |

`send_raw_transaction` aman diulang: gerbang memeriksa batas laju sebelum membaca badan permintaan. Perintah CLI tidak berubah.
Bukan konsensus; tanpa dependensi baru.

## Uji
- MOD-12: server HTTP tiruan menjawab `429` dua kali lalu galat JSON-RPC: tiga permintaan, galat terakhir dikembalikan; selalu
  `429`: 9 permintaan lalu galat 429. `is_rate_limited`.
- Penyimpanan bertahap: FTR-03 (`ftr03_send_receive`, riwayat sampai tinggi 103 melewati titik simpan 100), hasil riwayat sama.
- Gerbang: `make check-all` repo inti, wallet `make check-all` + `wallet-check` (Q176).
