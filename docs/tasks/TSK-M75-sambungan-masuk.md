# TSK-M75 · Sambungan masuk opsional · 🟢 · Prioritas: Sedang
- **Jenis:** task modul · **Modul:** MOD-06 (`service`), MOD-11 (`daemon`)
- **Bergantung pada:** TSK-M74 (alamat diumumkan lewat `addr`)
- **Keputusan:** DEC-43 (pencipta 23 November 2026)
- **Ditulis:** Quick, 23 November 2026

## Masalah
P2P hanya mendengar di `127.0.0.1`, jadi node peserta tidak pernah bisa menjadi rekan node lain, dan tidak ada cara memberi tahu
rekan alamat yang bisa dihubungi.

## Perubahan
| Bagian | Isi |
|---|---|
| `--p2p-bind <ip>` (`Config::p2p_bind`) | Alamat IP dengar P2P TCP, bawaan `127.0.0.1`; `0.0.0.0` atau `::` menerima sambungan masuk. Bukan alamat IP = `InvalidBind`, ganda ditolak. Bila bukan loopback, log peringatan: IP terlihat oleh rekan, izinkan port di firewall. WebSocket (`--p2p-ws-port`) dan RPC tetap `127.0.0.1` |
| `--p2p-publik <multiaddr>` (`Config::public`) | Boleh berulang; wajib `/tcp`, tanpa `/p2p` (`InvalidPublic`). Diteruskan ke `NetworkConfig::public`; MOD-06 menambahkan `/p2p/<PeerId>` dan mengumumkannya di `Addrs`. Tanpa opsi ini node tidak mengumumkan dirinya (tanpa tebakan alamat, tanpa UPnP) |
| `p2p_listen_address(bind, port)` | `/ip4/<bind>/tcp/<port>` atau `/ip6/<bind>/tcp/<port>` |
| Bantuan `--help` | Dua opsi baru dan letak `peers.json` |

Seed VPS kelak mengumumkan alamat `wss` resminya (`--p2p-publik /dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws`, Q191).
Blokir per IP (Spesifikasi P2P Bagian 7) berlaku untuk IP asli rekan masuk. Panduan peserta wajib memperingatkan bahwa membuka
port menampakkan IP.

## Uji
- `daemon::tests::parses_inbound_options`: bawaan `127.0.0.1` tanpa alamat publik, `0.0.0.0` dan dua alamat publik (TCP dan
  seed WebSocket), alamat dengar IPv4 dan IPv6, `peers_path`, galat (`localhost`, ganda, tanpa nilai, `/udp`, alamat dengan
  `/p2p`).
- Ujung ke ujung (Q190): daemon A dengan `--p2p-bind 0.0.0.0 --p2p-publik /ip4/127.0.0.1/tcp/<port>` mendengar di semua
  antarmuka dan ditemukan node B lewat seed S.
