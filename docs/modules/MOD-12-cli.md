# MOD-12: CLI (`zevrigo-cli`)

- **ID:** `MOD-12`
- **Path:** `apps/cli`
- **Paket:** `zevrigo-cli` (bin)
- **Status:** 🟢 TSK-M19 selesai: `info`, `wallet-create`, `wallet-restore`, `address`, `balance`, `send`; regtest dan testnet (`--testnet`, TSK-M06 bagian C3b); `history` (TSK-M41)
- **Bergantung pada:** `MOD-01`, `MOD-08`, `MOD-09` (DEC-21)
- **Dipakai oleh:** `MOD-15` (DEC-31)

## Tujuan

Menyediakan alat baris perintah ringan bagi operator untuk memeriksa status dan berinteraksi dengan daemon `zevrigo-node` melalui JSON-RPC.

## Tanggung Jawab

- Pengiriman perintah administrasi baca/kirim transaksi ke endpoint RPC node dan pemformatan keluaran terminal secara deterministik.
- Perintah wallet regtest dan testnet (buat, pulihkan, alamat, saldo, kirim) dan file wallet sesuai [spesifikasi-wallet-v0.1.md](../reference/spesifikasi-wallet-v0.1.md) Bagian 7, memakai `MOD-09` (DEC-21).
- Riwayat (TSK-M41, modul `history`): `Cli::history` membaca blok chain aktif lewat `get_block_hash` dan `get_block`, mencatat transaksi yang menyentuh alamat wallet (gap limit 20, diperluas saat alamat terpakai ditemukan) dan transaksi tertunda di mempool, dengan tembolok `<folder>/<jaringan>/history.json`; node tidak mencatat alamat (DEC-28). Pustaka ini juga dipakai wallet desktop MOD-15 (DEC-31). Sumber acak `os_random` lewat `getrandom` (Unix dan Windows, DEC-10 bagian 11).
- Jaringan (TSK-M06 bagian C3b): `--regtest` (bawaan) atau `--testnet`; file wallet di `<folder>/<jaringan>/wallet.json` dengan `network` yang wajib cocok (`load_network`), alamat `rzvrg1…` atau `tzvrg1…`, RPC bawaan port 28801 atau 18801, dan `balance`/`send` menolak node jaringan lain (`CliError::WrongNetwork`).
- Kirim dari satu alamat (TSK-M63): `send --dari <alamat>` hanya memilih UTXO alamat jenis 0 wallet itu (UTXO yang sedang dibelanjakan mempool tidak dipilih) dan mengirim kembalian kembali ke alamat itu, tanpa menaikkan indeks kembalian (`Cli::send_from`; `Cli::send` = tanpa sumber). Alamat di luar wallet sampai batas pemindaian: `CliError::SourceNotOwned`; alamat tidak sah: `CliError::SourceAddress`. Untuk kantong treasury demo (isi ulang faucet, distribusi), agar saldo kantong lain tidak tersentuh.
- Pratinjau dan kirim semua (TSK-M68, pustaka saja, dipakai wallet MOD-15): `Cli::preview` menyusun rencana yang sama dengan kirim tanpa password dan tanpa mengirim (`Preview`: jumlah diterima, biaya, kembalian, input, ukuran). `Cli::send_spend` menerima `Spend::Amount` atau `Spend::All` (MOD-09 `plan_sweep`) dan `Expected` dari pratinjau; bila jumlah atau biaya rencana baru berbeda, tidak ada yang ditandatangani (`CliError::PreviewChanged`). `send` dan `send --dari` tidak berubah.
- Tahan batas laju (TSK-M70): setiap panggilan RPC yang dijawab `429` oleh gerbang RPC demo (MOD-18) diulang sesudah 1 detik, paling banyak `RATE_LIMIT_RETRIES` (8) kali (`is_rate_limited`). `history` menyimpan `history.json` dan indeks alamat terpakai setiap `HISTORY_SAVE_EVERY` (100) blok selama pemindaian, sehingga pemindaian yang gagal di tengah dilanjutkan dari blok tersimpan terakhir, bukan dari genesis.

## Bukan Tanggung Jawab

- Validasi konsensus lokal (`MOD-04`).
- Menulis sendiri logika frasa, kunci, alamat, atau tanda tangan; semuanya dari `MOD-09`.

## Task terkait
- [TSK-M19](../tasks/TSK-M19-cli-wallet.md) — 🟢 selesai (pustaka `zevrigo_cli`: `args`, `wallet_file`, `scan`, `commands`)
- [TSK-M36](../tasks/TSK-M36-saldo-tertunda.md) — 🟢 selesai (saldo dan pemilihan koin dengan transaksi tertunda)
- [TSK-M37](../tasks/TSK-M37-enkripsi-wallet.md) — 🟢 selesai (file wallet versi 2 dikunci password, `wallet-encrypt`, `wallet-show-phrase`, DEC-27)
- [TSK-M41](../tasks/TSK-M41-riwayat-wallet.md) — 🟢 riwayat transaksi dari blok, `getrandom` untuk Windows
- [TSK-M63](../tasks/TSK-M63-kirim-dari-alamat.md) — 🟢 `send --dari <alamat>` (kantong treasury demo)
- [TSK-M68](../tasks/TSK-M68-pratinjau-kirim-semua.md) — 🟢 pratinjau kirim dan kirim semua saldo (`preview`, `send_spend`; wallet 0.3.0)
- [TSK-M70](../tasks/TSK-M70-wallet-batas-laju.md) — 🟡 coba lagi saat `429`, riwayat disimpan bertahap (wallet 0.3.1)
