# TSK-M66 · Pemisahan repo dalam zevrigo_lab · 🟡 tahap 4 berjalan · Prioritas: Tinggi
- **Jenis:** task repo (struktur) · **Modul:** MOD-14, MOD-15, MOD-17, MOD-18, blog · **Keputusan:** DEC-39
- **Bergantung pada:** TSK-M65 · **Ditulis:** Quick, 17 November 2026
- **Keputusan pencipta (17 November 2026):** aplikasi, terutama wallet, punya repo dan proyek sendiri; folder induk
  `zevrigo_lab` di komputer pencipta; urutan: pisahkan, rapikan, dokumentasi, sesuaikan; urusan git menunggu pencipta

## Repo
| Repo | Isi | Crate inti |
|---|---|---|
| `zevrigo` | Inti: crates/, node, cli, miner, tests/, vendor/, docs/ inti | — |
| `zevrigo-wallet` | MOD-15, rilis wallet, code signing policy | primitives, wallet-core, cli; dev: rpc |
| `zevrigo-explorer` | MOD-14 + `web/` | primitives, crypto, consensus, rpc + patch randomx-rs (DEC-01) |
| `zevrigo-demo` | MOD-18 | primitives, crypto, rpc, wallet-core |
| `zevrigo-blog` | Blog statis (DEC-37) | — |

## Tahap
| Tahap | Isi | Status |
|---|---|---|
| 1 Pisahkan | Salin `apps/wallet`, `apps/explorer`, `apps/demo`, `apps/blog` (tanpa hasil build dan `gen/`) ke `C:\zevrigo_lab\<repo>` beserta kartu modul, kartu task, dan spesifikasi terkait. Tiap repo: Cargo.toml workspace (crate inti `path = "../zevrigo/..."`, versi pihak ketiga dan `[workspace.lints]` sama dengan inti), `Cargo.lock`, rust-toolchain, clippy.toml, check-forbidden.sh, Makefile, .gitignore, lisensi, AGENTS.md, README, CI (repo inti di-checkout di sebelahnya), rilis wallet | 🟢 17 November 2026 |
| 2 Rapikan | Q148 + Q148b: `Cargo.lock` dipangkas hanya dengan membuang paket (wallet 497, explorer 223, demo 226 paket), randomx-rs explorer = `vendor/` inti, `make check-all` tanpa warning (9 + 31 + 35 test), wallet-check, web-check (22 test), blog-check (8 test); Next.js 16.4.4 → 16.4.6 di explorer web dan blog (izin pencipta; package-lock keduanya tetap sama) | 🟢 17 November 2026 |
| 3 Dokumentasi | DEC-39, kartu ini, AGENTS.md (struktur dan peta modul), system-map (peta repo), MOD-14/15/17/18 inti dan salinannya, tautan dokumen di repo aplikasi, README `zevrigo_lab`, ruang kerja Quick | 🟢 17 November 2026 (belum commit) |
| 4 Sesuaikan | Paket Docker dan compose ke zevrigo-demo (build dari folder lab, Next.js 16.4.6); uji FTR-05 ke zevrigo-explorer; proyek Android wallet di zevrigo-wallet (`tauri android init`, Maven lokal rustls, wrapper Gradle lokal, aturan R8 `zevrigo-rustls.pro`); CI dan rilis tiap repo; halaman unduh menunjuk repo wallet; tautan lintas repo jadi URL GitHub | 🟡 18 November 2026: FTR-05 ke zevrigo-explorer (Q150); Docker dan compose ke zevrigo-demo, salinan `apps/`, workflow rilis wallet, Code signing policy, dan dokumen aplikasi dihapus dari repo inti, tautan lintas repo jadi URL GitHub (Q151). Sisa: Android wallet, CI dan rilis tiap repo |
| Git (pencipta) | Kelima repo mulai bersih tanpa riwayat lama (keputusan pencipta 18 November 2026): satu commit awal per repo; `badakbesi/zevrigo` dihapus lalu dibuat ulang; tag repo inti untuk dependensi git; berkas `.agent/` | ⏳ menunggu pencipta |

## Catatan
- Sejak 18 November 2026 (Q151) repo inti tidak memuat salinan aplikasi. Image Docker dibangun dari `zevrigo-demo` dengan
  konteks folder induk `zevrigo_lab` (Next.js 16.4.6); VPS masih menjalankan image lama sampai dibangun ulang.
- Perubahan wallet 0.2.1 (TSK-M58 bagian 4) belum di-commit; ada di zevrigo-wallet dan zevrigo-blog.
- Kartu task lama di salinan dokumen repo aplikasi dibiarkan apa adanya sebagai catatan sejarah (jalur `apps/...`).
- Hasil lengkap uji build: `ai_advisory_hub/langkah/q148/q148_hasil.md` dan `q148b_hasil.md`.

## Definition of done
- [x] Tahap 1 pisahkan
- [x] Tahap 2 rapikan (uji build)
- [x] Tahap 3 dokumentasi
- [ ] Tahap 4 sesuaikan
- [ ] Urusan git oleh pencipta
