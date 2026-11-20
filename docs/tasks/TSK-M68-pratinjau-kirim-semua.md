# TSK-M68 · Pratinjau biaya dan kirim semua saldo · 🟡 · Prioritas: Tinggi
- **Jenis:** task modul · **Modul:** MOD-09 (`send`), MOD-12 (`commands`), dipakai MOD-15 (wallet)
- **Bergantung pada:** TSK-M16, TSK-M19, TSK-M36, TSK-M63
- **Ditulis:** Quick, 20 November 2026 (RENCANA_T9 F3 dan F4, wajib sebelum rilis wallet pertama; pencipta 19.10: "mulai sekarang")

## Masalah
Wallet hanya bisa melihat biaya pasti sesudah transaksi terkirim, dan tidak bisa mengosongkan saldo tanpa menebak biaya.

## Perubahan
| Bagian | Isi |
|---|---|
| MOD-09 `send` | `SweepRequest` dan `plan_sweep`: semua koin yang boleh dipakai (urutan sama dengan `plan`), satu output penerima = Σ input − biaya, tanpa kembalian. Varint jumlah penerima dicari seperti kembalian. `plan` memakai pemilih koin yang sama (`eligible_coins`), tanpa perubahan perilaku |
| MOD-12 `commands` | `Spend` (`Amount`/`All`), `Preview`, `Expected`; `Cli::preview` (tanpa password, tanpa kirim), `Cli::send_spend` (dengan pemeriksaan pratinjau, `CliError::PreviewChanged`). `send` dan `send_from` memakai penyusun rencana yang sama (`plan_send`), urutan langkah tetap (password dibuka sebelum pemindaian) |
| Dokumen | Spesifikasi Wallet v0.1 Bagian 5 (kirim semua, pratinjau), MOD-09, MOD-12 |

Bukan konsensus; tanpa dependensi baru; perintah CLI tidak berubah.

## Uji
- MOD-09: kirim semua koin T1 (2 input, 242 byte, biaya 242, ditandatangani), batas biaya dan varint (S1 sampai S3, tarif 0),
  tidak ada koin / saldo habis untuk biaya / koin ganda / tarif meluap / 1_100 koin (107_839 byte), proptest keseimbangan.
- MOD-12: pratinjau T1 sama dengan hasil kirim (byte transaksi T1 tetap), kirim semua 242 byte tanpa kembalian dan tanpa
  menaikkan indeks kembalian, pratinjau usang ditolak tanpa mengirim, password salah, wallet tanpa koin.
- Gerbang: `make check-all` repo inti, `make check-all` zevrigo-explorer, wallet MOD-15 (Q170).
