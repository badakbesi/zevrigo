# TSK-M73 · Seed resmi bawaan · 🟢 · Prioritas: Tinggi
- **Jenis:** task modul · **Modul:** MOD-11 (`daemon`)
- **Bergantung pada:** TSK-M28 (P2P di daemon), TSK-M45 (seed WebSocket), TSK-M67 (rekan operator tanpa blokir, DEC-41)
- **Keputusan:** DEC-43 (pencipta 23 November 2026)
- **Ditulis:** Quick, 23 November 2026

## Masalah
Node hanya menyambung ke alamat `--connect` yang diketik operator. Penguji jaringan demo harus menyalin alamat panjang berisi
`PeerId` seed (`/dns4/demo-seed-p2p.zevrigo.online/…/ws/p2p/12D3Koo…`). Pencipta: "seharusnya node tersambung secara otomatis".

## Perubahan
| Bagian | Isi |
|---|---|
| `daemon::REGTEST_SEEDS`, `TESTNET_SEEDS`, `official_seeds` | Seed resmi per jaringan, lengkap dengan `PeerId`. Regtest: seed jaringan demo. Testnet dan mainnet: kosong sampai lokasi seed ditetapkan (TSK-F06) |
| `Config::official_seeds`, opsi | `--seed-resmi` (bawaan regtest mati, karena regtest juga untuk uji lokal) dan `--tanpa-seed-resmi` (bawaan testnet dan mainnet hidup); keduanya bersama ditolak (`SeedConflict`), opsi ganda ditolak |
| `daemon::dial_addresses` | Alamat `--connect` urut argumen, lalu seed resmi bila aktif, tanpa duplikat; dipakai `NetworkConfig::connect`, jadi seed diperlakukan sebagai rekan operator (tidak diblokir, disambung ulang tiap 10 detik; DEC-41) |
| Log | `seed resmi bawaan dipakai` (jumlah seed), atau peringatan bila jaringan belum punya seed resmi |

Tanpa perubahan MOD-06, konsensus, atau dependensi. Penemuan rekan dan sambungan masuk: TSK-M74, TSK-M75.

## Uji
- `daemon::tests::official_seeds_only_when_asked_on_regtest`: alamat seed regtest persis (multiaddr sah, `PeerId`), testnet
  dan mainnet kosong, regtest tanpa opsi tidak mendial seed, `--seed-resmi` mendial seed, `--connect` + `--seed-resmi` tanpa
  duplikat dan urutan tetap, testnet bawaan aktif, `--tanpa-seed-resmi`, galat opsi.
- Ujung ke ujung (Q189): node regtest baru dengan `--seed-resmi` saja tersambung ke seed jaringan demo lewat internet dan
  mengejar tinggi publik; node tanpa opsi tidak tersambung ke mana pun.
