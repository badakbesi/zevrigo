# Sumber resmi Zevrigo (ZVRG)

> Ditetapkan pencipta 18 November 2026 (DEC-40). Dokumen ini adalah rujukan untuk mengenali Zevrigo yang asli: jaringan, repo,
> rilis, domain, dan logo resmi. Hanya pencipta yang mengubahnya, lewat repo inti `badakbesi/zevrigo`. Domain resmi
> `zevrigo.online` sejak 22 November 2026 (DEC-42).

Zevrigo terdiri dari **pusat publik tunggal** dan **bagian desentral**:

| Pusat publik tunggal | Desentral |
|---|---|
| Satu titik yang sama dan terbuka untuk semua orang: repo resmi, rilis resmi, seed resmi, explorer, gerbang RPC, faucet, blog, dan domain resmi (saat ini di satu VPS, DEC-38) | Node, penambang, dan wallet milik peserta, di komputer siapa pun; kunci dipegang pemiliknya sendiri |

Pusat tidak memegang kunci siapa pun dan tidak bisa mengubah aturan chain. Tidak ada kunci admin, tombol pembekuan, atau hak
istimewa di protokol untuk pihak mana pun, termasuk pencipta ([AGENTS.md](AGENTS.md)).

## Repo resmi

Hanya lima repo ini, di akun GitHub `badakbesi`:

| Repo | Isi |
|---|---|
| [badakbesi/zevrigo](https://github.com/badakbesi/zevrigo) | Repo inti: aturan konsensus, node, miner, CLI, dokumen protokol, dan dokumen ini |
| [badakbesi/zevrigo-wallet](https://github.com/badakbesi/zevrigo-wallet) | Wallet desktop Windows dan Android; satu-satunya tempat rilis wallet |
| [badakbesi/zevrigo-explorer](https://github.com/badakbesi/zevrigo-explorer) | Block explorer |
| [badakbesi/zevrigo-demo](https://github.com/badakbesi/zevrigo-demo) | Layanan dan paket Docker jaringan demo |
| [badakbesi/zevrigo-blog](https://github.com/badakbesi/zevrigo-blog) | Blog resmi |

Riwayat pengembangan sebelum kelima repo dipublikasikan bersih (18 November 2026) tidak dipublikasikan. Kode commit yang
disebut dokumen sebelum tanggal itu merujuk riwayat tersebut.

## Jaringan

| Jaringan | Status | Pengenal (`network_id` = id blok genesis) |
|---|---|---|
| Mainnet | Belum diluncurkan | Ditetapkan saat peluncuran |
| Testnet | Belum diluncurkan | Ditetapkan saat peluncuran (TSK-F06); nilai di kode sekarang sementara |
| Regtest (jaringan demo publik) | Dimulai ulang 19 November 2026 (chain sebelumnya dihapus); koin demo tidak bernilai | `8b9eb3187250bbdfe1577ebcb668c30003213a9ee6a4424427b08e38da364dab` |

Genesis regtest sama di setiap komputer, jadi jaringan demo resmi dikenali dari seed resminya:

```
/dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws/p2p/12D3KooWABPBsu8gZWtL7bbeEPjg6gwRPDbp3GZidpTeUC95rRDo
```

| Layanan | Alamat |
|---|---|
| Situs utama (blog) | https://zevrigo.online |
| Explorer demo | https://demo-explorer.zevrigo.online |
| Gerbang RPC demo untuk wallet | https://demo-gerbang-rpc.zevrigo.online |
| Faucet demo | https://demo-faucet.zevrigo.online |

**Alamat lama (peralihan, DEC-42).** Wallet 0.3.1 dan aplikasi Explorer 0.1.0 memakai alamat lama. Alamat ini dilayani pusat
yang sama sampai wallet dan aplikasi Explorer versi baru terbit, dan paling sedikit 30 hari sesudahnya: explorer
https://zevrigo.ratuaurion.store, gerbang RPC https://zevrigo-rpc.ratuaurion.store, faucet
https://zevrigo-faucet.ratuaurion.store, dan seed
`/dns4/zevrigo-seed.ratuaurion.store/tcp/443/tls/sni/zevrigo-seed.ratuaurion.store/ws/p2p/12D3KooWABPBsu8gZWtL7bbeEPjg6gwRPDbp3GZidpTeUC95rRDo`.
Subdomain lain di `ratuaurion.store` bukan bagian Zevrigo.

Jaringan dengan genesis lain, atau yang tidak tersambung ke seed resmi, bukan jaringan Zevrigo walaupun memakai kode yang sama.
Di Proof of Work siapa pun bisa menjalankan node dengan aturan berbeda; chain resmi adalah chain sah dengan kerja kumulatif
terbanyak menurut aturan di repo inti, tanpa checkpoint.

**Risiko jaringan demo (diumumkan 19 November 2026).** Jaringan demo memakai Proof of Work regtest yang sangat murah. Siapa pun yang
menjalankan node dan penambang bisa membuat chain lebih panjang dan menulis ulang riwayat jaringan demo. Itu disengaja agar
siapa pun bisa mencoba menambang tanpa alat khusus. Koin demo tidak bernilai, tidak bisa dijual, dan bisa hilang saat reset
atau saat chain ditulis ulang. Jangan menerima koin demo sebagai pembayaran apa pun.

## Rilis

- **Wallet:** hanya di [badakbesi/zevrigo-wallet/releases](https://github.com/badakbesi/zevrigo-wallet/releases), tag
  `wallet-v<versi>`, dengan `SHA256SUMS.txt`. Bandingkan SHA-256 berkas yang diunduh dengan daftar itu dan dengan halaman unduh
  blog resmi.
- **APK Android** ditandatangani sertifikat dengan sidik SHA-256:
  `1575be5a086d9242ec0661108cba4fd71276d0e94a40bca33749f991e9e30c1a`.
  APK dengan sertifikat lain bukan rilis resmi.
- **Installer Windows** belum bertanda tangan kode; Windows bisa menampilkan peringatan SmartScreen. Jalankan hanya bila SHA-256
  cocok.
- **Node, miner, CLI:** belum ada rilis biner; bangun dari repo inti (`cargo build --release --locked`).

## Logo dan nama

Logo resmi ada di [merek/](merek/README.md). Sidik SHA-256:

| Berkas | SHA-256 |
|---|---|
| `merek/zevrigo-logo-lengkap.png` | `f7ddc77a52ea67f26b7e9eceb7c2a57bbfa5fe02344b63b55cd069a0d44456aa` |
| `merek/zevrigo-tanda-1024.png` | `51b3c8cb1e21ccd440e5450924efb9eeb1c0ffad10d7c8e1dc913fbcb21545c7` |
| `merek/zevrigo-tanda-512.png` | `928c6aa380b27f9b63623634b93e0de631495581db683b705b3bf24a7d27bdc5` |
| `merek/zevrigo-tanda-256.png` | `05eccc37a0ae941e1f29d9334638c97190d8240f2d4f53c7ff96b21e432a6b99` |
| `merek/zevrigo-tanda-192.png` | `88fe8c33affbfc0d02f134bce253bbe07b145729ab33f1e30092975350ae098c` |
| `merek/zevrigo-ikon-aplikasi-1024.png` | `ec2776ec7448fb44dacbbf2703c324a6b794adfd84777a2f3e1bc84a70110816` |
| `merek/favicon.ico` | `246194d71e4964d876d735a02031e338eaf1b75b28901a344456f7e66e1de8cf` |
| `merek/favicon-32.png` | `51182bd4a606b43e77e3ef3805eb72a0f67ffa9c55ee9381f9764b53245f1608` |
| `merek/favicon-16.png` | `a15be4df94552baa5baf1153a650680c1128eb0593e8b7270df0937b36872ffc` |
| `merek/apple-touch-icon.png` | `e777a3a043124ff9059cbc23143e531510e172edd72c2737254e89627677d3f4` |
| `merek/zevrigo-pratinjau-sosial-1280x640.png` | `18fceaaddfd5d3916c689d84d75595f1bb5a1c5f997f3782aba9b4ca58a07224` |
| `merek/zevrigo-logo-sumber.jpg` | `7818ff4d7dfd7bc805a667c8b3edc64acf1b251f2e2a1a5d875af0173744a528` |

Kode Zevrigo berlisensi MIT OR Apache-2.0 dan boleh disalin. Lisensi itu tidak memberi hak atas nama "Zevrigo", ticker
"ZVRG", maupun logo (Apache-2.0 Bagian 6). Proyek turunan wajib memakai nama, logo, dan genesis lain.

## Melaporkan tiruan

Laporkan repo, situs, aplikasi, atau jaringan yang mengaku Zevrigo tetapi tidak tercantum di sini lewat Issues di
[badakbesi/zevrigo](https://github.com/badakbesi/zevrigo/issues).
