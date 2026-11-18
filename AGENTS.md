# AGENTS.md: Zevrigo Coin (ZVRG)

## Tentang proyek
Zevrigo Coin adalah blockchain Layer 1 Proof of Work yang ditulis dari nol dalam Rust.
Lapisan dasarnya sengaja minimal: model UTXO, tanpa VM serbaguna, dan hanya tipe transaksi
bawaan (transfer, multi-tanda tangan, kunci waktu).

Sebagian besar kode di repo ini adalah KODE KONSENSUS. Bug di sini bisa memecah jaringan,
mencetak koin di luar jadwal, atau membuat dana pengguna hilang. Kebenaran dan determinisme
selalu lebih penting daripada kecepatan menulis atau kerapian.

Prinsip yang tidak bisa ditawar:
- Tidak ada kunci admin, tombol pembekuan, atau hak istimewa untuk pihak mana pun, termasuk pembuat proyek.
- Aturan konsensus hanya ditulis di satu tempat (crate `consensus`).
- Siapa pun harus bisa membangun ulang program dan mendapat hasil identik (reproducible build).

## Pembagian peran (DEC-16)
| Peran | Tugas |
|---|---|
| Pencipta | Keputusan pencipta: arah proyek, parameter koin, nama, lisensi, prioritas. Meneruskan satu baris perintah ke agent |
| Quick (penasihat) | Rancangan, spesifikasi, keputusan teknis, kartu task, kode, test, dan dokumen. Menulis langsung di repo-repo zevrigo_lab, lalu menulis skrip pemeriksaan di `../ai_advisory_hub/langkah/` (akar zevrigo_lab, DEC-39) |
| Agent (pelaksana) | Menjalankan skrip langkah persis: format, lint, test, dan commit. Tidak mengedit file dan tidak memperbaiki error sendiri |

Untuk agent, bagian "Format laporan akhir" di bawah diganti oleh skrip langkah: skrip menulis output lengkap ke folder
langkah, dan agent hanya menulis baris terakhirnya.

## Parameter protokol (sumber kebenaran)
JANGAN mengubah angka di tabel ini. Kalau menurut Anda ada yang salah, berhenti dan laporkan.

| Parameter | Nilai |
|---|---|
| Satuan | 1 ZVRG = 10^18 quanta. Semua jumlah disimpan sebagai `u128` dalam quanta |
| Waktu blok target | 120 detik |
| Blok per tahun (konstanta) | 262_800 |
| Pasokan utama `S` (hasil tambang) | 100_000_000 ZVRG = 100_000_000 × 10^18 quanta (DEC-30) |
| Alokasi genesis pencipta `G` | 10_000_000 ZVRG (`S / 10`), hanya di blok genesis, DI LUAR `S`, tanpa kunci waktu (DEC-30) |
| Konstanta paruh `D` | 2_274_842 |
| Hadiah ekor `T` | tepat 2 ZVRG = 2 × 10^18 quanta per blok (DEC-30) |
| `M` | total quanta hasil tambang yang sudah keluar SEBELUM blok yang sedang dihitung (`G` tidak dihitung) |
| Hadiah dasar | `base(M) = max((S − M) / D, T)`, pembagian bilangan bulat dibulatkan ke bawah. Jika `M ≥ S`, `(S − M)` dianggap 0 |
| Slow start | untuk tinggi `h` dengan `1 ≤ h < 10_080`: `subsidy = base(M) × h / 10_080` (bilangan bulat). Untuk `h ≥ 10_080`: `subsidy = base(M)` |
| Genesis (`h = 0`) | subsidy tambang 0. Hanya berisi alokasi `G` |
| Biaya transaksi | seluruhnya untuk penambang, tanpa pembakaran |
| Kematangan coinbase | 100 blok |
| Ukuran blok maksimum | 100_000 byte |
| Hash umum | BLAKE3 |
| Tanda tangan | Schnorr BIP340 di kurva secp256k1. MuSig2 untuk multi-tanda tangan |
| PoW | varian RandomX (hanya salt Argon2 yang diganti: `RandomZVRG\x01`, DEC-01). Kunci diganti setiap 2_048 blok, jeda 64 blok |
| Penyesuaian kesulitan | ASERT per blok, waktu paruh 720 blok |
| Cap waktu blok | harus > median 11 blok terakhir dan ≤ waktu lokal node + 3_600 detik |
| Pemilihan chain | chain sah dengan kerja kumulatif terbanyak. TANPA batas kedalaman reorg. Tanpa checkpoint |
| Jaringan P2P | rust-libp2p 0.57 (DEC-24), protokol di docs/reference/spesifikasi-p2p-v0.1.md (bukan bagian konsensus) |

Nilai berikut baru ditetapkan saat testnet. JANGAN dikarang. Tulis sebagai konstanta
bertanda `// TODO(param): <nama>` dengan nilai placeholder yang jelas tidak valid:
kesulitan awal mainnet, alamat penerima alokasi genesis mainnet, hash blok Bitcoin jangkar
beserta tingginya, magic bytes/ID jaringan, dan nomor SLIP-44 mainnet untuk jalur turunan wallet.
Format alamat sudah ditetapkan (bech32m, DEC-11, Spesifikasi Format Bagian 8).

## Struktur repo dan peta modul
Struktur ini TETAP. Jangan menambah, menghapus, atau mengganti nama crate, aplikasi, repo, atau folder tingkat atas tanpa
izin. Perubahan struktur dicatat sebagai keputusan (DEC) di `docs/decisions/`.

Sejak DEC-39 (17 November 2026) proyek terdiri dari beberapa repo di folder induk **`zevrigo_lab`** di komputer pencipta
(`C:\zevrigo_lab`, WSL `/mnt/c/zevrigo_lab`). Repo ini adalah **repo inti**: sumber kebenaran parameter protokol, aturan
konsensus, larangan mutlak, pembagian peran, keputusan, peta modul, dan penomoran DEC/MOD/TSK/FTR untuk semua repo. Repo
aplikasi punya AGENTS.md sendiri yang merujuk file ini.

```
zevrigo_lab/                  # folder induk di komputer pencipta (bukan repo)
├── zevrigo/                  # REPO INTI (repo ini)
│   ├── AGENTS.md             # aturan semua repo
│   ├── SUMBER_RESMI.md       # repo, jaringan, rilis, domain, dan logo resmi (DEC-40)
│   ├── Cargo.toml            # workspace: members, dependencies, lints
│   ├── Cargo.lock  rust-toolchain.toml  clippy.toml  Makefile
│   ├── crates/               # MOD-01..MOD-10: library inti
│   ├── apps/                 # MOD-11 node, MOD-12 cli, MOD-13 miner; merchant/ (MOD-16 belum dibuat)
│   ├── tests/
│   │   ├── integration/      # MOD-17: uji lintas crate
│   │   └── vectors/          # vektor uji konsensus (data)
│   ├── scripts/              # skrip pemeriksaan (check-forbidden)
│   ├── vendor/               # randomx-rs + RandomX dengan salt ZVRG (DEC-01, vendor/README.md), juga untuk zevrigo-explorer
│   ├── docs/                 # README, system-map, modules, features, tasks, decisions, glossary, reference
│   ├── merek/                # logo resmi, favicon, ikon aplikasi (DEC-40); repo aplikasi memakai salinannya
│   └── .github/workflows/    # CI (make check-all)
├── zevrigo-wallet/           # MOD-15 wallet desktop dan Android; rilis wallet-v*
├── zevrigo-explorer/         # MOD-14 explorer: crate di akar repo, antarmuka di web/, uji FTR-05
├── zevrigo-demo/             # MOD-18 layanan demo regtest; paket Docker dan compose jaringan demo (DEC-38)
├── zevrigo-blog/             # blog resmi, situs statis Next.js (DEC-37)
├── ai_advisory_hub/          # ruang kerja Quick: status, antrean, langkah (di luar semua repo, DEC-16)
└── .agent/  .vscode/         # aturan agent Antigravity dan pengaturan editor untuk workspace lab
```

**Pemisahan DEC-39 selesai di repo ini (18 November 2026).** Salinan `apps/explorer`, `apps/wallet`, `apps/demo`,
`apps/blog`, uji FTR-05, paket Docker dan compose, workflow rilis wallet, Code signing policy, dan dokumen aplikasi
(kartu MOD-14, MOD-15, MOD-18, kartu task aplikasi, spesifikasi explorer dan Docker) sudah dihapus dari repo ini; semuanya
ada di repo aplikasinya. Repo inti tidak memuat kode, tag, rilis, workflow, atau dokumen aplikasi (AGENDA P15). Penomoran
DEC/MOD/TSK/FTR tetap di sini; indeks (`docs/tasks/README.md`, `docs/reference/README.md`) menautkan kartu dan spesifikasi
aplikasi di GitHub repo masing-masing.

| ID | Repo | Path di repo | Paket | Fungsi | Bergantung pada |
|---|---|---|---|---|---|
| MOD-01 | zevrigo | crates/primitives | zevrigo-primitives | Tipe dasar: jumlah (u128 quanta), tinggi blok, hash, parameter chain, struktur blok dan transaksi | — |
| MOD-02 | zevrigo | crates/crypto | zevrigo-crypto | BLAKE3, Schnorr BIP340, MuSig2 | MOD-01 |
| MOD-03 | zevrigo | crates/pow | zevrigo-pow | Proof of Work: antarmuka, PoW dev regtest (DEC-09), RandomX testnet/mainnet lewat randomx-rs (DEC-01) | MOD-01, 02 (DEC-14) |
| MOD-04 | zevrigo | crates/consensus | zevrigo-consensus | Satu-satunya tempat aturan konsensus: pasokan, validasi, ASERT, cap waktu | MOD-01, 02, 03 |
| MOD-05 | zevrigo | crates/storage | zevrigo-storage | Penyimpanan blok, set UTXO, pruning | MOD-01 |
| MOD-06 | zevrigo | crates/network | zevrigo-network | P2P (rust-libp2p) | MOD-01 |
| MOD-07 | zevrigo | crates/mempool | zevrigo-mempool | Antrean transaksi | MOD-01, 04 |
| MOD-08 | zevrigo | crates/rpc | zevrigo-rpc | Tipe dan klien/server JSON-RPC | MOD-01 |
| MOD-09 | zevrigo | crates/wallet-core | zevrigo-wallet-core | Kunci, penyusunan dan penandatanganan transaksi | MOD-01, 02 |
| MOD-10 | zevrigo | crates/merchant-core | zevrigo-merchant-core | Tagihan watch-only, status pembayaran | MOD-01, 09 |
| MOD-11 | zevrigo | apps/node | zevrigo-node (bin) | Daemon node | MOD-01, 02, 04, 05, 06, 07, 08 (DEC-19) |
| MOD-12 | zevrigo | apps/cli | zevrigo-cli (bin + pustaka) | Alat baris perintah ke node; pustaka wallet untuk MOD-15 (DEC-31) | MOD-01, 08, 09 (DEC-21) |
| MOD-13 | zevrigo | apps/miner | zevrigo-miner (bin) | Penambang CPU | MOD-01, 03, 08 |
| MOD-14 | zevrigo-explorer | akar repo; antarmuka `web/` | zevrigo-explorer (bin) | Indexer, API, dan antarmuka web Next.js + Tailwind + shadcn/ui (DEC-26), hanya data sistem (DEC-28) | MOD-01, 02, 04, 08 (DEC-25, DEC-28); dev (uji FTR-05): MOD-05, 07, 09, 11, 13 |
| MOD-15 | zevrigo-wallet | `src-tauri/` (crate), `src/` (antarmuka React) | zevrigo-wallet (bin + pustaka) | Wallet Tauri 2, desktop Windows dan Android | MOD-01, 09, 12 (DEC-31) |
| MOD-16 | — | (belum dibuat) | — | Aplikasi pelaku usaha, Tauri | MOD-08, 10 |
| MOD-17 | zevrigo | tests/integration | zevrigo-integration-tests | Uji integrasi lintas crate | semua crate inti; dev: MOD-11, MOD-13 (DEC-20), MOD-12 (DEC-23) |
| MOD-18 | zevrigo-demo | akar repo | zevrigo-demo (bin) | Layanan demo regtest: gerbang RPC (TSK-M53), upacara genesis (TSK-M62), faucet, penyaji situs statis untuk blog (TSK-M64, DEC-37), pendaftaran (DEC-34, DEC-36) | MOD-01, 02, 08, 09 |
| — | zevrigo-blog | akar repo | zevrigo-blog (npm) | Blog resmi, situs statis Next.js, artikel Markdown (DEC-37) | — (disajikan MOD-18 mode `web`) |

Aturan arah dependensi:
- Crate di `crates/` tidak boleh bergantung pada apa pun di `apps/`, `tests/`, atau repo aplikasi.
- Repo inti tidak bergantung pada repo aplikasi, tanpa pengecualian (sejak 18 November 2026 uji FTR-05 ada di
  `zevrigo-explorer`).
- Logo, favicon, dan ikon aplikasi hanya dari `merek/` repo ini (DEC-40); repo aplikasi memakai salinan berkas yang sama
  (sidik SHA-256 di `SUMBER_RESMI.md`). Jangan membuat atau mengubah logo tanpa keputusan pencipta.
- Repo aplikasi memakai crate inti lewat `[workspace.dependencies]` dengan `path = "../zevrigo/<path>"` (sementara; kelak
  dependensi git ke repo inti dengan tag). Versi paket pihak ketiga sama dengan `Cargo.toml` repo inti; mengubahnya wajib izin.
- **DEC-01:** setiap repo yang membawa MOD-03 (juga lewat MOD-04) wajib memasang `[patch.crates-io] randomx-rs = { path =
  "../zevrigo/vendor/randomx-rs" }` dan memeriksa sumbernya di `make check-all` (`make check-randomx` di zevrigo-explorer).
  Tanpa patch, Cargo diam-diam memakai randomx-rs crates.io dengan salt resmi RandomX dan hash PoW berbeda dengan node.
- Dependensi antar modul hanya boleh mengikuti kolom "Bergantung pada". Dependensi baru antar modul wajib minta izin dan dicatat sebagai DEC.
- MOD-15 (repo zevrigo-wallet): crate Rust di `src-tauri/` (satu-satunya anggota workspace), antarmuka di akar repo (npm,
  package-lock.json, `make wallet-check`). Di Linux, kompilasi MOD-15 butuh WebKitGTK 4.1 (libwebkit2gtk-4.1-dev,
  librsvg2-dev, libxdo-dev, libayatana-appindicator3-dev). MOD-16 diinisialisasi lewat task tersendiri. MOD-18 (repo
  zevrigo-demo, DEC-36) hanya untuk demo regtest; pembungkus Tauri 2 APK Explorer (bagian MOD-14, TSK-M57) belum dibuat.
  Blog (repo zevrigo-blog, DEC-37) adalah situs statis Next.js (npm, package-lock.json sama dengan `web/` zevrigo-explorer
  kecuali nama dan versi, `make blog-check`), artikel di `posts/*.md`, disajikan `zevrigo-demo web`; bukan crate.
- Paket Docker regtest (repo zevrigo-demo, `make docker-build`) dibangun dengan konteks folder induk `zevrigo_lab`: node,
  CLI, dan miner dari repo ini; explorer dan antarmuka web, demo, dan blog dari repo masing-masing. Yang masuk konteks hanya
  daftar izin di `zevrigo-demo/Dockerfile.dockerignore`.

## Perintah
```bash
make fmt         # cargo fmt --all
make fmt-check   # cargo fmt --all -- --check
make lint        # cargo clippy --workspace --all-targets -- -D warnings
make test        # cargo test --workspace
make test-slow   # cargo test --workspace --release -- --ignored
make check-forbidden  # gagal jika ada f32/f64 atau kata kunci unsafe di kode proyek
make check-all   # fmt-check + lint + check-forbidden + test
```

Repo aplikasi punya Makefile sendiri (`make check-all`, dan `wallet-check`, `web-check`, `blog-check`, atau
`docker-build` di zevrigo-demo); lihat AGENTS.md repo masing-masing.

## Larangan mutlak: `unsafe` dan floating point
Berlaku untuk SEMUA kode milik proyek ini: setiap crate di `crates/`, setiap aplikasi di `apps/`,
`tests/`, build script, contoh, dan benchmark. Tidak ada pengecualian, termasuk untuk test.

1. **Tanpa `unsafe`.** Dilarang menulis blok, fungsi, trait, atau `impl` yang `unsafe`.
   - `[workspace.lints.rust] unsafe_code = "forbid"`, dan setiap `lib.rs`/`main.rs` diawali `#![forbid(unsafe_code)]`.
   - `forbid` tidak boleh diturunkan dengan `#[allow]` di mana pun.
   - Jika suatu tugas tampaknya butuh `unsafe` (misalnya FFI ke C/C++), BERHENTI dan laporkan. Jangan mencari jalan pintas.
2. **Tanpa floating point.** Dilarang memakai tipe `f32`/`f64`, literal desimal (`1.5`, `2e3`),
   konversi `as f32`/`as f64`, atau fungsi matematika float.
   - Persentase, rasio, dan statistik dihitung dengan bilangan bulat, misalnya basis poin
     (1/10_000) atau pasangan pembilang/penyebut.
   - Jumlah koin selalu `u128` quanta. Ditampilkan sebagai teks lewat `Amount::to_zvrg_string`.
   - Lint wajib di `[workspace.lints.clippy]`: `float_arithmetic`, `float_cmp`, `float_cmp_const`,
     `lossy_float_literal`, `cast_precision_loss`, `imprecise_flops`, `suboptimal_flops`, semuanya `"deny"`.
   - UI berbasis TypeScript (tugas berikutnya): jumlah koin tidak boleh disimpan di tipe `number`.
     Pakai `string` atau `bigint`.
3. **Pemeriksaan otomatis (`make check-forbidden`):** gagal jika `grep -rnwE 'f32|f64'` menemukan
   kecocokan di file `*.rs` milik proyek, atau jika kata kunci `unsafe` muncul di luar komentar.
   Target ini termasuk di `make check-all`.
4. **Catatan dependensi pihak ketiga:** larangan ini mengikat kode yang kita tulis. Pustaka standar
   Rust dan banyak dependensi berisi `unsafe` atau float di dalamnya. Menambah dependensi tetap wajib
   minta izin dulu, dan jangan memilih dependensi yang memaksa kode kita memakai float atau `unsafe`.

## Aturan wajib untuk crate konsensus (`primitives`, `crypto`, `pow`, `consensus`, `storage`)
1. **Determinisme.** Selain larangan float di atas, dilarang memakai waktu sistem, angka acak, atau urutan
   iterasi `HashMap`/`HashSet` yang memengaruhi hasil. Gunakan `BTreeMap` atau `Vec` terurut.
2. **Aritmetika aman.** Semua operasi pada jumlah dan tinggi blok memakai `checked_*`, atau
   `saturating_*` jika memang disengaja dan dijelaskan di komentar. Di `lib.rs` crate konsensus aktifkan:
   `#![deny(clippy::arithmetic_side_effects, clippy::float_arithmetic, clippy::indexing_slicing)]`
3. **Tanpa panic di kode produksi.** Dilarang `unwrap`, `expect`, `panic!`, atau indexing yang bisa
   panic. Kembalikan `Result` dengan tipe error dari `thiserror`.
4. **Tanpa `unsafe`**, tanpa pengecualian (lihat larangan mutlak di atas). Di crate `pow`: PoW dev regtest (TSK-M05, DEC-09) dan RandomX lewat crate `randomx-rs` (DEC-01, TSK-M06). Hanya MOD-03 yang boleh memakai `randomx-rs`; `unsafe` dan float hanya ada di dalam pustaka itu.
5. **Encoding kanonis.** Satu nilai hanya boleh punya satu representasi byte.
6. Jangan menulis aturan konsensus di luar crate `consensus`.

## Aturan test
- Unit test diletakkan di `src/<modul>/tests.rs` dan dimuat dengan `#[cfg(test)] mod tests;`. Kode dan test tetap di file terpisah.
- Integration test diletakkan di `<crate>/tests/`.
- Test lambat (misalnya simulasi bertahun-tahun) diberi `#[ignore]` dan dijalankan lewat `make test-slow`.
- Setiap rumus konsensus wajib punya test nilai pasti (vektor uji) dan property test (`proptest`).
- DILARANG menonaktifkan, melewati, atau melemahkan test supaya lolos. Kalau test benar tapi kode tidak lolos, perbaiki kodenya. Kalau test-nya yang Anda curigai salah, berhenti dan laporkan.

## Dependensi
- Menambah dependensi baru wajib minta izin dulu, kecuali yang tercantum di prompt tugas.
- Sudah diizinkan: `thiserror` dan `proptest` (khusus dev-dependency).
- Semua versi diatur di `[workspace.dependencies]`. File `Cargo.lock` wajib di-commit.

## Git
- Format commit mengikuti Conventional Commits (`feat:`, `fix:`, `test:`, `chore:`, `docs:`).
- Satu tugas sama dengan satu commit, atau beberapa commit kecil yang saling berkaitan.
- Jangan `git push`, jangan force, dan jangan mengubah riwayat commit. Push dilakukan pengguna sendiri (DEC-32).
- Jangan membuat, menghapus, atau mengubah repo remote maupun pengaturan `git remote`.
- Jangan memakai GitHub CLI (`gh`), akun plugin, token, atau akun online milik pengguna dalam bentuk apa pun tanpa izin tertulis di prompt tugas.
- Jangan mengubah identitas commit atau konfigurasi Git (`git config`, baik lokal maupun global).
  Jika identitas belum diset, BERHENTI dan minta pengguna mengaturnya.

Alasan: akun, remote, dan identitas commit adalah keputusan pengguna, bukan agent.
Identitas repo dan commit sudah diputuskan pengguna (DEC-08): akun GitHub `badakbesi` (`github.com/badakbesi/<repo>`, diatur
pengguna sendiri di `git config`). Kelima repo dipublikasikan bersih tanpa riwayat sebelum 18 November 2026.
Jangan menggantinya, dan jangan menambah akun atau remote lain.

## Berhenti dan tanya bila
- Ada parameter protokol yang tampak bertentangan atau belum ditentukan.
- Tugas butuh dependensi baru, `sudo`, atau perubahan di luar ruang lingkup.
- Ada keraguan apakah suatu perilaku termasuk aturan konsensus.
- Perlu menjalankan perintah destruktif (menghapus file di luar yang dibuat tugas ini, reset, force).

## Definition of done
- `make check-all` lolos tanpa warning.
- `make test-slow` lolos, jika tugas menambah test lambat.
- Tidak ada `TODO` baru selain `TODO(param)` yang memang disengaja.
- Laporan akhir sudah ditulis dengan format di bawah.

## Format laporan akhir
1. Ringkasan perubahan dan alasannya
2. Daftar file yang dibuat atau diubah
3. Output lengkap `make check-all` (dan `make test-slow` jika relevan)
4. Asumsi yang diambil, risiko, dan pertanyaan terbuka
