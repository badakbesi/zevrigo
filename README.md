# Zevrigo Coin (ZVRG)

<img src="merek/zevrigo-tanda-192.png" alt="Logo Zevrigo" width="96">

Zevrigo Coin (ZVRG) adalah blockchain Layer 1 Proof of Work yang ditulis dari nol dalam Rust dengan fokus utama pada kebenaran konsensus, determinisme penuh, dan ketiadaan hak istimewa. Lapisan dasarnya sengaja minimal: model UTXO, tanpa VM serbaguna, dan hanya mendukung tipe transaksi bawaan (transfer, multi-tanda tangan MuSig2, serta kunci waktu).

Dokumentasi arsitektur, peta modul, keputusan desain, dan glosarium tersedia di [docs/README.md](docs/README.md) (serta aturan pengembangan lengkap di [AGENTS.md](AGENTS.md)).

## Prasyarat

- Linux / WSL2 pada filesystem Linux asli (bukan `/mnt/c`)
- Rust `1.99.0` melalui `rustup` (otomatis dikunci lewat `rust-toolchain.toml` beserta `rustfmt` dan `clippy`)
- Perangkat sistem: `make`, `bash`, `git`, `cmake`, dan compiler C++ `g++` (wajib sejak PoW RandomX, DEC-01; di Ubuntu: `sudo apt-get install -y cmake g++`)

## Verifikasi

Jalankan pemeriksaan format, lint `clippy`, larangan `unsafe`/floating point, dan seluruh test dengan satu perintah:

```bash
make check-all
```

## Menjalankan node regtest

```bash
cargo run -p zevrigo-node -- --regtest --datadir ~/.zevrigo
```

Node membuat chain regtest di `~/.zevrigo/regtest/chain.redb`, lalu mencetak alamat JSON-RPC (bawaan
`http://127.0.0.1:28801`). Delapan metode RPC dijelaskan di
[docs/reference/spesifikasi-rpc-v0.1.md](docs/reference/spesifikasi-rpc-v0.1.md). Hentikan dengan Ctrl-C.

Node juga mendengar P2P di `127.0.0.1:28800` dan mencetak alamatnya sebagai `p2p: /ip4/127.0.0.1/tcp/28800/p2p/<PeerId>`.
Node kedua di folder lain bisa tersambung dan mengejar chain:

```bash
cargo run -p zevrigo-node -- --regtest --datadir ~/.zevrigo-2 --rpc-port 28811 --p2p-port 28810 \
  --connect /ip4/127.0.0.1/tcp/28800/p2p/<PeerId>
```

Dengan `--p2p-ws-port <port>` node juga mendengar P2P WebSocket di `/ip4/127.0.0.1/tcp/<port>/ws` (baris `p2p-ws: ...`),
untuk seed di belakang proxy TLS seperti Cloudflare Tunnel. `--connect` juga menerima nama, misalnya
`/dns4/<nama>/tcp/443/tls/sni/<nama>/ws/p2p/<PeerId>`.

Blok dan transaksi baru disiarkan antar node. Aturan lengkap di [docs/reference/spesifikasi-p2p-v0.1.md](docs/reference/spesifikasi-p2p-v0.1.md).

Opsi `--testnet` (PoW RandomX, port bawaan RPC 18801 dan P2P 18800) sudah ada, tetapi testnet publik belum diluncurkan:
cap waktu genesis testnet ditetapkan saat peluncuran (TSK-F06). Node, miner, CLI, dan wallet desktop sudah mendukung testnet.

## Menambang di regtest

Dengan node berjalan, di terminal lain:

```bash
cargo run -p zevrigo-miner -- --payout <kunci x-only hex 64> --blocks 10
```

Miner mengambil template dari node, mencari nonce dengan PoW dev regtest, dan mengirim blok. Hadiah coinbase bisa
dibelanjakan setelah 100 blok. `--blocks 0` (bawaan) menambang terus sampai Ctrl-C.

Untuk node `--testnet`, tambahkan `--testnet`: miner memakai RandomX mode cepat (butuh sekitar 2,5 GB RAM; dataset disusun
sekitar satu menit setiap kunci berganti). `--light` memakai mode ringan (256 MB, jauh lebih lambat).

## Wallet CLI regtest

```bash
cargo run -p zevrigo-cli -- --datadir ~/.zevrigo wallet-create
cargo run -p zevrigo-cli -- --datadir ~/.zevrigo address
cargo run -p zevrigo-cli -- --datadir ~/.zevrigo balance
cargo run -p zevrigo-cli -- --datadir ~/.zevrigo send --to <alamat rzvrg1...> --amount 1.5
```

Wallet disimpan di `~/.zevrigo/regtest/wallet.json` (izin `0600`), dikunci password: `wallet-create` meminta
password baru dua kali (minimal 8 karakter, tidak tampil di layar), dan `send` serta `wallet-show-phrase` memintanya
lagi. `address` dan `balance` tidak butuh password. Simpan frasa 24 kata yang dicetak `wallet-create`; password tidak
bisa memulihkan wallet tanpa frasa itu. Wallet lama tanpa password dikunci dengan `wallet-encrypt`. Untuk skrip regtest,
password boleh diberikan lewat variabel lingkungan `ZEVRIGO_WALLET_PASSWORD`. `address` juga mencetak payload hex yang bisa dipakai untuk `--payout`
miner, sehingga hadiah tambang masuk ke wallet. `balance` juga menampilkan transaksi yang masih menunggu di mempool
(`keluar tertunda`, `masuk tertunda`, dan saldo setelah semuanya masuk blok), dan `send` tidak memilih koin yang sedang
dibelanjakan. Aturan lengkap di [docs/reference/spesifikasi-wallet-v0.1.md](docs/reference/spesifikasi-wallet-v0.1.md).

Untuk node `--testnet`, tambahkan `--testnet` ke setiap perintah: wallet disimpan terpisah di
`~/.zevrigo/testnet/wallet.json`, alamatnya `tzvrg1...`, dan RPC bawaan `http://127.0.0.1:18801`.

## Aplikasi dan jaringan demo

Aplikasi punya repo sendiri sejak DEC-39, di folder induk `zevrigo_lab` di sebelah repo ini (crate inti dipakai lewat
`../zevrigo`). Repo inti ini tidak memuat kode, rilis, workflow, atau dokumen aplikasi.

| Repo | Isi |
|---|---|
| [zevrigo-explorer](https://github.com/badakbesi/zevrigo-explorer) | Explorer (MOD-14): indexer SQLite, API JSON baca saja, antarmuka web; uji ujung ke ujung FTR-05 |
| [zevrigo-wallet](https://github.com/badakbesi/zevrigo-wallet) | Wallet desktop Windows dan Android (MOD-15): kode, rilis `wallet-v<versi>`, Code signing policy |
| [zevrigo-demo](https://github.com/badakbesi/zevrigo-demo) | Layanan demo regtest (MOD-18), paket Docker, dan jaringan demo publik di VPS dan PC (DEC-38) |
| [zevrigo-blog](https://github.com/badakbesi/zevrigo-blog) | Blog resmi, situs statis |

Situs utama: https://zevrigo.online. Jaringan demo regtest publik: explorer https://demo-explorer.zevrigo.online. Node lain bergabung ke seed
dengan:

```bash
zevrigo-node --regtest --datadir ~/.zevrigo \
  --connect /dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws/p2p/<PeerId seed>
```

## Sumber resmi

Repo, jaringan, rilis, domain, dan logo resmi Zevrigo tercantum di [SUMBER_RESMI.md](SUMBER_RESMI.md). Logo resmi:
[merek/](merek/README.md).

## Lisensi

Dilisensikan dengan salah satu dari berikut, sesuai pilihan Anda (DEC-02):

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

Kontribusi yang dikirim untuk dimasukkan ke proyek ini, kecuali dinyatakan lain, dilisensikan ganda seperti di atas
tanpa syarat tambahan.
