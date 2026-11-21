//! Aksi CLI: ringkasan chain dan perintah wallet regtest dan testnet (TSK-M19, DEC-21, TSK-M06 bagian C3b).
//!
//! Jaringan dipilih di [`Config`]: file wallet di folder jaringannya, alamat ber-HRP jaringan itu, dan `balance` serta
//! `send` hanya mau bekerja dengan node di jaringan yang sama. Kunci akun sama untuk regtest dan testnet (`coin` 1).
//!
//! `wallet-create`, `wallet-restore`, `wallet-encrypt`, `wallet-show-phrase`, dan `address` berjalan tanpa node. `info`, `balance`, dan `send` memanggil
//! node lewat RPC (MOD-08) dengan runtime tokio satu thread. `balance` dan `send` memperhitungkan transaksi tertunda
//! di mempool node (`scan_mempool`, TSK-M36): UTXO yang sedang dibelanjakan tidak dihitung bisa dipakai dan tidak
//! dipilih lagi, output tertunda ke wallet dilaporkan sebagai masuk tertunda.
//!
//! File wallet baru dikunci password (DEC-27, TSK-M37): frasa terenkripsi Argon2id + XChaCha20-Poly1305 (MOD-09),
//! kunci akun publik disimpan terbuka. Password hanya diminta saat kunci rahasia dibutuhkan (`wallet-create`,
//! `wallet-restore`, `wallet-encrypt`, `wallet-show-phrase`, `send`); `address` dan `balance` memakai kunci akun publik. Frasa, kunci, alamat, pemilihan koin, dan tanda tangan
//! seluruhnya dari MOD-09. Entropi frasa dan `aux_rand` tanda tangan diberikan pemanggil.
//!
//! `history` (TSK-M41) menghitung riwayat transaksi wallet dari blok chain aktif lewat RPC `get_block`, dengan tembolok
//! di `<folder>/<jaringan>/history.json` (modul [`crate::history`]); node tidak mencatat alamat siapa pun (DEC-28).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;
use tokio::runtime::Runtime;
use zeroize::Zeroizing;
use zevrigo_primitives::{
    Amount, Block, Decode, Encode, Hash32, Network, OutPoint, Output, OutputKind,
};
use zevrigo_rpc::{
    ChainInfo, Hex32, MAX_SCAN_PAYLOADS, MempoolOutput, MempoolScan, RpcClient, reject_reason,
};
use zevrigo_wallet_core::{
    AddressError, Bip32Error, CHANGE_BRANCH, Coin, ExtendedPrivKey, ExtendedPubKey, HARDENED,
    KdfParams, Mnemonic, MnemonicError, NONCE_LEN, Plan, RECEIVE_BRANCH, SALT_LEN, Sealed,
    SendError, SendRequest, SweepRequest, VaultError, account_key, decode_address, encode_address,
    plan, plan_sweep, seal, sign_plan, unseal,
};

use crate::args::{Action, Config};
use crate::history::{self, History, HistoryError, HistoryState, HistoryTip, WalletKeys};
use crate::scan::{ScanResult, scan_wallet, wallet_payloads};
use crate::wallet_file::{self, NextIndex, WalletFile, WalletFileError};

/// Galat menjalankan aksi. Biner keluar dengan kode 1.
#[derive(Debug, Error)]
pub enum CliError {
    /// Runtime tokio tidak bisa dibuat.
    #[error("runtime gagal dibuat: {0}")]
    Runtime(std::io::Error),
    /// Sumber acak sistem operasi tidak bisa dibaca.
    #[error("sumber acak sistem operasi gagal dibaca: {0}")]
    Random(std::io::Error),
    /// Panggilan RPC gagal (node tidak bisa dihubungi atau membalas error).
    #[error("RPC gagal: {message}")]
    Rpc {
        /// Keterangan dari klien RPC.
        message: String,
    },
    /// Node menolak transaksi (error 1002).
    #[error("node menolak transaksi: aturan {reason}")]
    Rejected {
        /// Nama aturan yang dilanggar, dari node.
        reason: String,
    },
    /// Node berjalan di jaringan lain dari jaringan CLI.
    #[error(
        "node berjalan di jaringan {actual}, bukan {expected} (pilih --regtest atau --testnet)"
    )]
    WrongNetwork {
        /// Jaringan CLI.
        expected: String,
        /// Jaringan node.
        actual: String,
    },
    /// File wallet.
    #[error(transparent)]
    File(#[from] WalletFileError),
    /// Frasa atau passphrase tidak sah.
    #[error("frasa atau passphrase tidak sah: {0}")]
    Mnemonic(#[from] MnemonicError),
    /// Turunan kunci gagal.
    #[error("turunan kunci gagal: {0}")]
    Key(#[from] Bip32Error),
    /// Alamat penerima tidak sah.
    #[error("alamat penerima tidak sah: {0}")]
    Address(#[from] AddressError),
    /// Alamat `--dari` tidak sah (TSK-M63).
    #[error("alamat --dari tidak sah: {0}")]
    SourceAddress(AddressError),
    /// Alamat `--dari` bukan alamat jenis 0 wallet ini, sampai batas pemindaian (TSK-M63).
    #[error("alamat --dari {address} bukan alamat wallet ini")]
    SourceNotOwned {
        /// Alamat yang ditulis.
        address: String,
    },
    /// Transaksi tidak bisa disusun.
    #[error("transaksi tidak bisa disusun: {0}")]
    Send(#[from] SendError),
    /// Semua indeks non-hardened di satu cabang sudah terpakai.
    #[error("indeks alamat habis")]
    IndexExhausted,
    /// Aksi wallet tanpa folder data (tidak terjadi untuk konfigurasi dari `parse_args`).
    #[error("folder data wajib untuk perintah wallet")]
    MissingDatadir,
    /// Password baru lebih pendek dari [`MIN_PASSWORD_CHARS`].
    #[error("password terlalu pendek: minimal 8 karakter")]
    PasswordTooShort,
    /// Password baru dan ulangannya tidak sama.
    #[error("kedua password tidak sama")]
    PasswordMismatch,
    /// Password tidak bisa dibaca dari terminal.
    #[error("password tidak bisa dibaca: {0}")]
    PasswordInput(std::io::Error),
    /// Password salah, atau frasa terenkripsi di file wallet diubah.
    #[error("password salah atau file wallet rusak")]
    WrongPassword,
    /// Enkripsi atau parameter Argon2id gagal.
    #[error("enkripsi wallet gagal: {0}")]
    Vault(VaultError),
    /// Passphrase menghasilkan kunci akun lain daripada yang tersimpan di file wallet.
    #[error("passphrase tidak cocok dengan kunci akun di file wallet")]
    PassphraseMismatch,
    /// `wallet-encrypt` untuk wallet yang sudah terenkripsi.
    #[error("wallet sudah dikunci password")]
    AlreadyEncrypted,
    /// Riwayat tidak bisa dihitung dari blok.
    #[error("riwayat gagal dihitung: {0}")]
    History(#[from] HistoryError),
    /// File tembolok riwayat tidak bisa ditulis.
    #[error("file riwayat {path}: {message}")]
    HistoryFile {
        /// Path file tembolok.
        path: String,
        /// Galat dari sistem operasi.
        message: String,
    },
    /// Chain aktif node berubah (reorg) selama pemindaian riwayat; ulangi perintahnya.
    #[error("chain node berubah selama pemindaian riwayat; ulangi perintahnya")]
    ChainChanged,
    /// Jumlah atau biaya kirim berubah sejak pratinjau yang dilihat pengguna (saldo atau mempool berubah). Tidak ada yang
    /// ditandatangani atau dikirim (RENCANA_T9 F3).
    #[error(
        "jumlah atau biaya berubah sejak pratinjau: sekarang penerima menerima {amount} quanta dengan biaya {fee} quanta; periksa lagi"
    )]
    PreviewChanged {
        /// Jumlah yang akan diterima penerima sekarang, dalam quanta.
        amount: u128,
        /// Biaya sekarang, dalam quanta.
        fee: u128,
    },
}

impl From<VaultError> for CliError {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Decrypt => Self::WrongPassword,
            other => Self::Vault(other),
        }
    }
}

/// Panjang password wallet paling sedikit, dalam karakter.
pub const MIN_PASSWORD_CHARS: usize = 8;

/// Variabel lingkungan berisi password wallet, untuk skrip. Jika ada, terminal tidak ditanya.
pub const PASSWORD_ENV: &str = "ZEVRIGO_WALLET_PASSWORD";

/// Pertanyaan password ke pengguna.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordPrompt {
    /// Password wallet yang sudah ada.
    Unlock,
    /// Password baru.
    New,
    /// Ulangan password baru.
    Confirm,
}

impl PasswordPrompt {
    /// Teks pertanyaan di terminal.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Unlock => "password wallet: ",
            Self::New => "password baru (minimal 8 karakter): ",
            Self::Confirm => "ulangi password baru: ",
        }
    }
}

/// Password dan bahan acak untuk mengunci frasa di file wallet baru.
pub struct Protection<'a> {
    /// Password wallet.
    pub password: &'a str,
    /// Parameter Argon2id (CLI: [`KdfParams::DEFAULT`]).
    pub kdf: KdfParams,
    /// Salt acak.
    pub salt: [u8; SALT_LEN],
    /// Nonce acak.
    pub nonce: [u8; NONCE_LEN],
}

impl Protection<'_> {
    fn seal(&self, phrase: &str) -> Result<Sealed, CliError> {
        Ok(seal(
            phrase.as_bytes(),
            self.password.as_bytes(),
            self.kdf,
            self.salt,
            self.nonce,
        )?)
    }
}

/// Memeriksa password baru: minimal [`MIN_PASSWORD_CHARS`] karakter, dan sama dengan ulangannya.
///
/// # Errors
///
/// [`CliError::PasswordTooShort`] atau [`CliError::PasswordMismatch`].
pub fn check_new_password(first: &str, second: &str) -> Result<(), CliError> {
    if first.chars().count() < MIN_PASSWORD_CHARS {
        return Err(CliError::PasswordTooShort);
    }
    if first != second {
        return Err(CliError::PasswordMismatch);
    }
    Ok(())
}

/// Membaca password dari [`PASSWORD_ENV`], atau dari terminal tanpa menampilkannya.
///
/// # Errors
///
/// [`CliError::PasswordInput`] jika terminal tidak bisa dibaca.
pub fn terminal_password(prompt: PasswordPrompt) -> Result<Zeroizing<String>, CliError> {
    if let Ok(value) = std::env::var(PASSWORD_ENV) {
        return Ok(Zeroizing::new(value));
    }
    rpassword::prompt_password(prompt.text())
        .map(Zeroizing::new)
        .map_err(CliError::PasswordInput)
}

/// Berapa kali satu panggilan RPC dicoba lagi bila gerbang menjawab `429` (batas laju), sebelum galatnya dikembalikan
/// (TSK-M70).
pub const RATE_LIMIT_RETRIES: u32 = 8;

/// Jeda sebelum mencoba lagi panggilan yang ditolak batas laju, sama dengan `Retry-After: 1` gerbang RPC demo (TSK-M70).
pub const RATE_LIMIT_WAIT: Duration = Duration::from_secs(1);

/// Tembolok riwayat disimpan setiap kelipatan tinggi ini selama pemindaian, agar pemindaian yang terputus (galat jaringan,
/// batas laju, aplikasi ditutup) dilanjutkan dari blok terakhir yang tersimpan, bukan dari genesis (TSK-M70).
pub const HISTORY_SAVE_EVERY: u64 = 100;

/// Apakah keterangan galat klien RPC adalah jawaban HTTP `429` (batas laju gerbang RPC, MOD-18). Klien MOD-08 melaporkannya
/// sebagai galat transport "Request rejected `429`".
#[must_use]
pub fn is_rate_limited(message: &str) -> bool {
    message.contains("rejected `429`")
}

/// Galat RPC sebagai teks, agar tipe galat klien tidak ikut terekspos.
fn rpc_error(error: impl core::fmt::Display) -> CliError {
    CliError::Rpc {
        message: error.to_string(),
    }
}

/// Saldo wallet di ujung chain, beserta transaksi tertunda di mempool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balance {
    /// Tinggi ujung chain.
    pub tip_height: u64,
    /// Jumlah semua UTXO milik wallet di chain (`bisa dipakai` + `belum matang` + `keluar tertunda`).
    pub total: Amount,
    /// Bagian yang boleh dibelanjakan di blok berikutnya dan belum dibelanjakan transaksi di mempool.
    pub spendable: Amount,
    /// Bagian yang belum matang (coinbase di bawah 100 konfirmasi).
    pub immature: Amount,
    /// UTXO wallet yang sedang dibelanjakan transaksi di mempool.
    pub pending_spent: Amount,
    /// Output transaksi di mempool ke alamat wallet (termasuk kembalian).
    pub pending_received: Amount,
    /// Saldo setelah semua transaksi tertunda masuk blok: `total − pending_spent + pending_received`.
    pub after_pending: Amount,
    /// Jumlah UTXO milik wallet di chain.
    pub utxos: usize,
}

/// Hasil `send`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    /// Txid dari node.
    pub txid: Hex32,
    /// Jumlah yang diterima penerima.
    pub amount: Amount,
    /// Biaya untuk penambang.
    pub fee: Amount,
    /// Kembalian, jika ada.
    pub change: Option<Amount>,
    /// Jumlah input.
    pub inputs: usize,
    /// Ukuran transaksi dalam byte.
    pub size: usize,
}

/// Jumlah yang dikirim (RENCANA_T9 F4): jumlah tertentu, atau semua saldo yang bisa dipakai dikurangi biaya.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spend {
    /// Penerima menerima jumlah ini; kembalian ke alamat kembalian wallet berikutnya.
    Amount(Amount),
    /// Semua UTXO yang bisa dipakai (di luar yang sedang dibelanjakan transaksi di mempool) menjadi input; penerima
    /// menerima Σ input − biaya, tanpa kembalian (MOD-09 `plan_sweep`).
    All,
}

/// Jumlah dan biaya yang sudah dilihat pengguna di pratinjau. [`Cli::send_spend`] tidak menandatangani bila rencana baru
/// berbeda ([`CliError::PreviewChanged`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Expected {
    /// Jumlah yang diterima penerima.
    pub amount: Amount,
    /// Biaya untuk penambang.
    pub fee: Amount,
}

/// Pratinjau kirim (RENCANA_T9 F3): rencana yang sama dengan `send` (pemilihan koin, biaya, kembalian), tanpa password,
/// tanda tangan, dan pengiriman.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    /// Jumlah yang diterima penerima.
    pub amount: Amount,
    /// Biaya untuk penambang.
    pub fee: Amount,
    /// Kembalian, jika ada.
    pub change: Option<Amount>,
    /// Jumlah input.
    pub inputs: usize,
    /// Ukuran transaksi bertanda tangan dalam byte.
    pub size: usize,
    /// Tinggi ujung chain saat pratinjau dihitung.
    pub tip_height: u64,
}

impl Preview {
    /// Jumlah dan biaya untuk [`Cli::send_spend`].
    #[must_use]
    pub const fn expected(&self) -> Expected {
        Expected {
            amount: self.amount,
            fee: self.fee,
        }
    }
}

/// Rencana kirim dari node dan file wallet, belum ditandatangani.
struct Planned {
    wallet: OpenWallet,
    network_id: Hash32,
    chosen: Plan,
    change_index: Option<u32>,
    tip_height: u64,
}

impl Planned {
    fn recipient_amount(&self) -> Amount {
        self.chosen
            .outputs()
            .first()
            .map_or(Amount::ZERO, Output::amount)
    }

    fn preview(&self) -> Preview {
        Preview {
            amount: self.recipient_amount(),
            fee: self.chosen.fee(),
            change: self.chosen.change(),
            inputs: self.chosen.coins().len(),
            size: self.chosen.size(),
            tip_height: self.tip_height,
        }
    }
}

/// Transaksi tertunda di mempool node yang menyentuh wallet.
struct Pending {
    /// Outpoint UTXO wallet yang sedang dibelanjakan.
    spent: BTreeSet<(Hex32, u32)>,
    /// Output tertunda jenis 0 ke alamat wallet.
    outputs: Vec<MempoolOutput>,
}

/// Wallet yang sudah dibuka: jaringan, path file, isi file, dan kunci akun publik `m/44'/1'/0'`.
struct OpenWallet {
    network: Network,
    path: PathBuf,
    file: WalletFile,
    public: ExtendedPubKey,
}

/// Kunci akun rahasia dari frasa dan passphrase.
fn account_from(
    network: Network,
    mnemonic: &Mnemonic,
    passphrase: &str,
) -> Result<ExtendedPrivKey, CliError> {
    let seed = mnemonic.to_seed(passphrase)?;
    Ok(account_key(&seed, network)?)
}

/// Membuka wallet tanpa password: versi 2 memakai kunci akun publik di file (passphrase tidak dipakai), versi 1
/// menurunkannya dari frasa dan passphrase.
fn open_wallet(network: Network, datadir: &Path, passphrase: &str) -> Result<OpenWallet, CliError> {
    let path = wallet_file::wallet_path(network, datadir);
    let file = wallet_file::load_network(network, &path)?;
    let public = match file.account()? {
        Some(public) => public,
        None => account_from(network, &file.mnemonic()?, passphrase)?.to_public()?,
    };
    Ok(OpenWallet {
        network,
        path,
        file,
        public,
    })
}

/// Frasa pemulihan: versi 1 langsung, versi 2 dibuka dengan `password`.
fn unlock_mnemonic(file: &WalletFile, password: &str) -> Result<Mnemonic, CliError> {
    let Some(sealed) = file.sealed()? else {
        return Ok(file.mnemonic()?);
    };
    let bytes = unseal(&sealed, password.as_bytes())?;
    let phrase = core::str::from_utf8(&bytes).map_err(|_| CliError::WrongPassword)?;
    Mnemonic::parse(phrase).map_err(|_| CliError::WrongPassword)
}

/// Kunci akun rahasia untuk menandatangani. Untuk versi 2, kunci publiknya wajib sama dengan yang tersimpan.
fn private_account(
    wallet: &OpenWallet,
    passphrase: &str,
    password: &str,
) -> Result<ExtendedPrivKey, CliError> {
    let mnemonic = unlock_mnemonic(&wallet.file, password)?;
    let account = account_from(wallet.network, &mnemonic, passphrase)?;
    if account.to_public()? != wallet.public {
        return Err(CliError::PassphraseMismatch);
    }
    Ok(account)
}

/// File wallet versi 2 untuk frasa ini: kunci akun publik dari frasa dan passphrase, frasa dikunci `protection`.
fn encrypted_file(
    network: Network,
    mnemonic: &Mnemonic,
    passphrase: &str,
    protection: &Protection<'_>,
    next_index: NextIndex,
) -> Result<WalletFile, CliError> {
    let public = account_from(network, mnemonic, passphrase)?.to_public()?;
    let sealed = protection.seal(mnemonic.phrase())?;
    Ok(WalletFile::new_encrypted(
        network, &public, &sealed, next_index,
    ))
}

/// Menyimpan tembolok riwayat `state`, lalu indeks alamat terpakai tertinggi yang ditemukan `keys` ke file wallet jika lebih
/// tinggi dari yang tersimpan (dipakai di akhir `history` dan setiap [`HISTORY_SAVE_EVERY`] blok).
fn save_history(
    wallet: &mut OpenWallet,
    keys: &WalletKeys,
    path: &Path,
    identity: Hex32,
    state: &HistoryState,
) -> Result<(), CliError> {
    history::save(path, wallet.network, identity, state).map_err(|error| {
        CliError::HistoryFile {
            path: path.display().to_string(),
            message: error.to_string(),
        }
    })?;
    let found = keys.next_index();
    let next_index = NextIndex {
        receive: found.receive.max(wallet.file.next_index.receive),
        change: found.change.max(wallet.file.next_index.change),
    };
    if next_index != wallet.file.next_index {
        wallet.file.next_index = next_index;
        wallet_file::save(&wallet.path, &wallet.file)?;
    }
    Ok(())
}

/// Alamat pertama yang sah di `branch` mulai indeks `start`: `(indeks, x-only)`.
fn derive_unused(
    account: &ExtendedPubKey,
    branch: u32,
    start: u32,
) -> Result<(u32, [u8; 32]), CliError> {
    let branch_key = account.derive_child(branch)?;
    let mut index = start;
    while index < HARDENED {
        match branch_key.derive_child(index) {
            Ok(child) => return Ok((index, child.x_only())),
            // Indeks yang turunannya tidak sah dilewati, sesuai BIP32.
            Err(Bip32Error::InvalidChild { .. }) => {
                index = index.checked_add(1).ok_or(CliError::IndexExhausted)?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(CliError::IndexExhausted)
}

/// `wallet-create`: wallet baru jaringan `network` dari 32 byte entropi, frasa dikunci `protection`. Mengembalikan
/// frasa dan path file.
///
/// # Errors
///
/// [`CliError::File`] jika wallet sudah ada atau tidak bisa ditulis, [`CliError::Mnemonic`] jika passphrase tidak
/// sah, [`CliError::Vault`] jika enkripsi gagal.
pub fn wallet_create(
    network: Network,
    datadir: &Path,
    entropy: &[u8; 32],
    passphrase: &str,
    protection: &Protection<'_>,
) -> Result<(Mnemonic, PathBuf), CliError> {
    let mnemonic = Mnemonic::from_entropy(entropy)?;
    let path = wallet_file::wallet_path(network, datadir);
    let file = encrypted_file(
        network,
        &mnemonic,
        passphrase,
        protection,
        NextIndex::default(),
    )?;
    wallet_file::create_new(&path, &file)?;
    Ok((mnemonic, path))
}

/// `wallet-restore`: wallet jaringan `network` dari frasa 12 sampai 24 kata, dikunci `protection`. Mengembalikan path
/// file.
///
/// # Errors
///
/// [`CliError::Mnemonic`] jika frasa atau passphrase tidak sah, [`CliError::File`] jika wallet sudah ada atau tidak
/// bisa ditulis, [`CliError::Vault`] jika enkripsi gagal.
pub fn wallet_restore(
    network: Network,
    datadir: &Path,
    phrase: &str,
    passphrase: &str,
    protection: &Protection<'_>,
) -> Result<PathBuf, CliError> {
    let mnemonic = Mnemonic::parse(phrase)?;
    let path = wallet_file::wallet_path(network, datadir);
    let file = encrypted_file(
        network,
        &mnemonic,
        passphrase,
        protection,
        NextIndex::default(),
    )?;
    wallet_file::create_new(&path, &file)?;
    Ok(path)
}

/// `wallet-encrypt`: mengubah file wallet versi 1 (tanpa password) menjadi versi 2. Indeks berikutnya tetap.
///
/// # Errors
///
/// [`CliError::AlreadyEncrypted`] untuk wallet versi 2, galat file wallet, [`CliError::Mnemonic`] jika passphrase
/// tidak sah, atau [`CliError::Vault`].
pub fn wallet_encrypt(
    network: Network,
    datadir: &Path,
    passphrase: &str,
    protection: &Protection<'_>,
) -> Result<PathBuf, CliError> {
    let path = wallet_file::wallet_path(network, datadir);
    let file = wallet_file::load_network(network, &path)?;
    if file.is_encrypted() {
        return Err(CliError::AlreadyEncrypted);
    }
    let encrypted = encrypted_file(
        network,
        &file.mnemonic()?,
        passphrase,
        protection,
        file.next_index,
    )?;
    wallet_file::save(&path, &encrypted)?;
    Ok(path)
}

/// `wallet-show-phrase`: frasa pemulihan wallet. Versi 2 butuh `password`; versi 1 tidak.
///
/// # Errors
///
/// [`CliError::WrongPassword`] jika password salah, atau galat file wallet.
pub fn show_phrase(
    network: Network,
    datadir: &Path,
    password: &str,
) -> Result<Zeroizing<String>, CliError> {
    let file = wallet_file::load_network(network, &wallet_file::wallet_path(network, datadir))?;
    let mnemonic = unlock_mnemonic(&file, password)?;
    Ok(Zeroizing::new(mnemonic.phrase().to_owned()))
}

/// `address`: alamat penerimaan berikutnya, ber-HRP jaringan `network`. Indeks penerimaan di file wallet dinaikkan.
///
/// # Errors
///
/// [`CliError::File`], [`CliError::Mnemonic`], [`CliError::Key`], atau [`CliError::IndexExhausted`].
pub fn next_address(
    network: Network,
    datadir: &Path,
    passphrase: &str,
) -> Result<String, CliError> {
    let mut wallet = open_wallet(network, datadir, passphrase)?;
    let (index, payload) = derive_unused(
        &wallet.public,
        RECEIVE_BRANCH,
        wallet.file.next_index.receive,
    )?;
    wallet.file.next_index.receive = index.checked_add(1).ok_or(CliError::IndexExhausted)?;
    wallet_file::save(&wallet.path, &wallet.file)?;
    Ok(encode_address(network, OutputKind::SingleKey, &payload))
}

/// Klien ke satu node di jaringan CLI.
pub struct Cli {
    client: RpcClient,
    runtime: Runtime,
    network: Network,
    /// Jeda sebelum mencoba lagi panggilan yang ditolak batas laju ([`RATE_LIMIT_WAIT`]; lebih pendek di test).
    retry_wait: Duration,
}

impl Cli {
    /// Menyiapkan klien untuk node jaringan `network` di `rpc_url`. Belum ada koneksi sampai panggilan pertama.
    ///
    /// # Errors
    ///
    /// [`CliError::Runtime`] jika runtime gagal dibuat, [`CliError::Rpc`] jika `rpc_url` tidak sah.
    pub fn connect(rpc_url: &str, network: Network) -> Result<Self, CliError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(CliError::Runtime)?;
        let client = runtime
            .block_on(async { RpcClient::new(rpc_url) })
            .map_err(rpc_error)?;
        Ok(Self {
            client,
            runtime,
            network,
            retry_wait: RATE_LIMIT_WAIT,
        })
    }

    /// Menjalankan satu panggilan RPC. Bila gerbang menjawab `429` ([`is_rate_limited`]), panggilan diulang sesudah
    /// [`RATE_LIMIT_WAIT`], paling banyak [`RATE_LIMIT_RETRIES`] kali (TSK-M70). Galat lain langsung dikembalikan. Aman untuk
    /// `send_raw_transaction`: gerbang menolak `429` sebelum badan permintaan dibaca, jadi transaksi belum diteruskan.
    fn retry<T, E, F>(&self, request: impl Fn() -> F) -> Result<T, E>
    where
        E: core::fmt::Display,
        F: core::future::Future<Output = Result<T, E>>,
    {
        let mut retries = 0_u32;
        loop {
            match self.runtime.block_on(request()) {
                Err(error)
                    if retries < RATE_LIMIT_RETRIES && is_rate_limited(&error.to_string()) =>
                {
                    retries = retries.saturating_add(1);
                    std::thread::sleep(self.retry_wait);
                }
                result => return result,
            }
        }
    }

    /// `info`: ringkasan chain dari node.
    ///
    /// # Errors
    ///
    /// [`CliError::Rpc`].
    pub fn info(&self) -> Result<ChainInfo, CliError> {
        self.retry(|| self.client.get_chain_info())
            .map_err(rpc_error)
    }

    /// Ringkasan chain, dengan syarat node berjalan di jaringan CLI.
    fn checked_info(&self) -> Result<ChainInfo, CliError> {
        let info = self.info()?;
        if info.network.network() != self.network {
            return Err(CliError::WrongNetwork {
                expected: wallet_file::network_name(self.network).to_owned(),
                actual: String::from(info.network),
            });
        }
        Ok(info)
    }

    /// Memindai wallet, lalu menyimpan indeks berikutnya jika pemindaian menemukan indeks terpakai yang lebih tinggi.
    fn scan(&self, wallet: &mut OpenWallet) -> Result<ScanResult, CliError> {
        let result = scan_wallet(&wallet.public, wallet.file.next_index, |payloads| {
            self.retry(|| self.client.scan_utxos(payloads))
                .map_err(rpc_error)
        })?;
        if result.next_index != wallet.file.next_index {
            wallet.file.next_index = result.next_index;
            wallet_file::save(&wallet.path, &wallet.file)?;
        }
        Ok(result)
    }

    /// `scan_mempool` untuk semua `payloads`, paling banyak [`MAX_SCAN_PAYLOADS`] sekali panggil.
    fn mempool_scan(&self, payloads: &[Hex32]) -> Result<MempoolScan, CliError> {
        let mut scan = MempoolScan::default();
        for chunk in payloads.chunks(MAX_SCAN_PAYLOADS) {
            let part = self
                .retry(|| self.client.scan_mempool(chunk))
                .map_err(rpc_error)?;
            scan.spent.extend(part.spent);
            scan.outputs.extend(part.outputs);
        }
        Ok(scan)
    }

    /// ID blok chain aktif di `height`.
    fn block_hash(&self, height: u64) -> Result<Hex32, CliError> {
        self.retry(|| self.client.get_block_hash(height))
            .map_err(rpc_error)
    }

    /// Transaksi tertunda di mempool node yang menyentuh alamat wallet sampai batas pemindaian `scanned`: outpoint
    /// UTXO wallet yang sedang dibelanjakan, dan output tertunda jenis 0 ke alamat wallet.
    fn pending(&self, wallet: &OpenWallet, scanned: &ScanResult) -> Result<Pending, CliError> {
        let payloads = wallet_payloads(&wallet.public, scanned.next_index);
        let scan = self.mempool_scan(&payloads)?;
        let mine: BTreeSet<Hex32> = payloads.into_iter().collect();
        let spent = scan
            .spent
            .iter()
            .map(|spend| (spend.txid, spend.index))
            .collect();
        let outputs = scan
            .outputs
            .into_iter()
            .filter(|output| {
                output.kind.kind() == OutputKind::SingleKey && mine.contains(&output.payload)
            })
            .collect();
        Ok(Pending { spent, outputs })
    }

    /// `balance`: saldo wallet dari node, termasuk transaksi tertunda di mempool.
    ///
    /// # Errors
    ///
    /// [`CliError::Rpc`], [`CliError::WrongNetwork`], galat file wallet, atau [`CliError::Send`] jika jumlah meluap.
    pub fn balance(&self, datadir: &Path, passphrase: &str) -> Result<Balance, CliError> {
        let info = self.checked_info()?;
        let mut wallet = open_wallet(self.network, datadir, passphrase)?;
        let scanned = self.scan(&mut wallet)?;
        let pending = self.pending(&wallet, &scanned)?;
        // Sengaja saturating: pada tinggi u64::MAX semua UTXO dianggap sudah bisa dipakai.
        let limit = info.height.saturating_add(1);
        let overflow = || CliError::Send(SendError::AmountOverflow);
        let mut balance = Balance {
            tip_height: info.height,
            total: Amount::ZERO,
            spendable: Amount::ZERO,
            immature: Amount::ZERO,
            pending_spent: Amount::ZERO,
            pending_received: Amount::ZERO,
            after_pending: Amount::ZERO,
            utxos: scanned.found.len(),
        };
        for found in &scanned.found {
            let amount = found.utxo.amount.amount();
            balance.total = balance.total.checked_add(amount).ok_or_else(overflow)?;
            if pending.spent.contains(&(found.utxo.txid, found.utxo.index)) {
                balance.pending_spent = balance
                    .pending_spent
                    .checked_add(amount)
                    .ok_or_else(overflow)?;
            } else if found.utxo.spendable_at <= limit {
                balance.spendable = balance.spendable.checked_add(amount).ok_or_else(overflow)?;
            } else {
                balance.immature = balance.immature.checked_add(amount).ok_or_else(overflow)?;
            }
        }
        for output in &pending.outputs {
            balance.pending_received = balance
                .pending_received
                .checked_add(output.amount.amount())
                .ok_or_else(overflow)?;
        }
        balance.after_pending = balance
            .total
            .checked_sub(balance.pending_spent)
            .and_then(|rest| rest.checked_add(balance.pending_received))
            .ok_or_else(overflow)?;
        Ok(balance)
    }

    /// `history`: riwayat transaksi wallet, terbaru dulu (transaksi tertunda di mempool paling atas).
    ///
    /// Blok dibaca dari node mulai sesudah blok ujung tembolok `history.json`, atau dari genesis jika tembolok tidak ada,
    /// tidak sah, milik wallet lain, atau blok ujungnya tidak lagi di chain aktif (reorg). Tembolok disimpan sesudahnya, dan
    /// juga setiap [`HISTORY_SAVE_EVERY`] blok selama pemindaian (TSK-M70), sehingga pemindaian yang gagal di tengah
    /// dilanjutkan dari sana. Indeks alamat terpakai yang lebih tinggi yang ditemukan disimpan ke file wallet, seperti
    /// `balance`.
    ///
    /// # Errors
    ///
    /// [`CliError::Rpc`], [`CliError::WrongNetwork`], [`CliError::ChainChanged`], [`CliError::History`],
    /// [`CliError::HistoryFile`], atau galat file wallet.
    pub fn history(&self, datadir: &Path, passphrase: &str) -> Result<History, CliError> {
        let info = self.checked_info()?;
        let mut wallet = open_wallet(self.network, datadir, passphrase)?;
        let path = history::history_path(self.network, datadir);
        let identity = history::identity(&wallet.public);
        let mut state = history::load(&path, self.network, identity);
        if let Some(tip) = state.tip {
            let kept = tip.height <= info.height && self.block_hash(tip.height)? == tip.block_id;
            if !kept {
                state = HistoryState::new();
            }
        }
        let mut keys = WalletKeys::new(&wallet.public, wallet.file.next_index);
        let start = state.tip.map_or(0, |tip| tip.height.saturating_add(1));
        let mut previous = state.tip.map(|tip| tip.block_id);
        for height in start..=info.height {
            let block_id = self.block_hash(height)?;
            let found = self
                .retry(|| self.client.get_block(block_id))
                .map_err(rpc_error)?;
            if found.height != height || previous.is_some_and(|id| id != found.prev_block_id) {
                return Err(CliError::ChainChanged);
            }
            let block = Block::from_bytes(found.hex.as_slice()).map_err(rpc_error)?;
            state.apply_block(&mut keys, &block, &found.txids, height)?;
            state.tip = Some(HistoryTip { height, block_id });
            previous = Some(block_id);
            if height.is_multiple_of(HISTORY_SAVE_EVERY) && height < info.height {
                save_history(&mut wallet, &keys, &path, identity, &state)?;
            }
        }
        let scan = self.mempool_scan(&keys.payloads())?;
        let outputs: Vec<MempoolOutput> = scan
            .outputs
            .into_iter()
            .filter(|output| {
                output.kind.kind() == OutputKind::SingleKey && keys.contains(output.payload.bytes())
            })
            .collect();
        let pending = state.pending_entries(&scan.spent, &outputs)?;
        save_history(&mut wallet, &keys, &path, identity, &state)?;
        let mut entries = pending;
        entries.extend(state.entries().iter().rev().cloned());
        Ok(History {
            tip_height: info.height,
            entries,
        })
    }

    /// `send`: memilih koin, menandatangani dengan `aux_rand`, lalu mengirim ke mempool node. UTXO yang sedang
    /// dibelanjakan transaksi di mempool tidak dipilih. Indeks kembalian di file wallet dinaikkan jika transaksi
    /// memakai output kembalian. `password` membuka frasa wallet versi 2 (diabaikan untuk versi 1).
    ///
    /// # Errors
    ///
    /// [`CliError::Address`], [`CliError::WrongPassword`], [`CliError::PassphraseMismatch`], [`CliError::Send`],
    /// [`CliError::Rejected`], [`CliError::Rpc`], [`CliError::WrongNetwork`], atau galat file wallet.
    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &self,
        datadir: &Path,
        passphrase: &str,
        password: &str,
        to: &str,
        amount: Amount,
        fee_rate: u128,
        aux_rand: &[u8; 32],
    ) -> Result<Sent, CliError> {
        self.send_from(
            datadir, passphrase, password, None, to, amount, fee_rate, aux_rand,
        )
    }

    /// `send` dengan sumber pilihan (`send --dari <alamat>`, TSK-M63). Dengan `from`, hanya UTXO alamat itu yang
    /// dipilih dan kembalian kembali ke alamat itu, sehingga saldo alamat lain di wallet tidak tersentuh dan indeks
    /// kembalian di file wallet tidak berubah. Dipakai untuk kantong treasury demo. Tanpa `from` sama dengan [`Cli::send`].
    ///
    /// # Errors
    ///
    /// Seperti [`Cli::send`], ditambah [`CliError::SourceAddress`] jika `from` bukan alamat jaringan ini dan
    /// [`CliError::SourceNotOwned`] jika `from` bukan alamat jenis 0 wallet ini (sampai batas pemindaian).
    /// [`SendError::InsufficientFunds`] melaporkan saldo alamat `from` saja.
    #[allow(clippy::too_many_arguments)]
    pub fn send_from(
        &self,
        datadir: &Path,
        passphrase: &str,
        password: &str,
        from: Option<&str>,
        to: &str,
        amount: Amount,
        fee_rate: u128,
        aux_rand: &[u8; 32],
    ) -> Result<Sent, CliError> {
        let (planned, account) = self.plan_send(
            datadir,
            passphrase,
            from,
            to,
            Spend::Amount(amount),
            fee_rate,
            |wallet| private_account(wallet, passphrase, password),
        )?;
        self.sign_and_send(planned, &account, aux_rand)
    }

    /// Pratinjau kirim (RENCANA_T9 F3): koin, biaya, dan kembalian dipilih persis seperti [`Cli::send_spend`], tanpa
    /// password, tanpa tanda tangan, dan tanpa mengirim apa pun ke node. Seperti `balance`, file wallet hanya diperbarui
    /// dengan indeks alamat yang ditemukan pemindaian.
    ///
    /// # Errors
    ///
    /// [`CliError::Address`], [`CliError::Send`] (misalnya [`SendError::InsufficientFunds`]), [`CliError::Rpc`],
    /// [`CliError::WrongNetwork`], atau galat file wallet.
    pub fn preview(
        &self,
        datadir: &Path,
        passphrase: &str,
        to: &str,
        spend: Spend,
        fee_rate: u128,
    ) -> Result<Preview, CliError> {
        let (planned, ()) =
            self.plan_send(datadir, passphrase, None, to, spend, fee_rate, |_| Ok(()))?;
        Ok(planned.preview())
    }

    /// `send` untuk [`Spend`] (jumlah tertentu atau semua saldo, RENCANA_T9 F4), dengan pemeriksaan pratinjau: bila
    /// `expected` ada dan jumlah atau biaya rencana baru berbeda, tidak ada yang ditandatangani atau dikirim
    /// ([`CliError::PreviewChanged`]). [`Spend::Amount`] sama dengan [`Cli::send`].
    ///
    /// # Errors
    ///
    /// Seperti [`Cli::send`], ditambah [`CliError::PreviewChanged`].
    #[allow(clippy::too_many_arguments)]
    pub fn send_spend(
        &self,
        datadir: &Path,
        passphrase: &str,
        password: &str,
        to: &str,
        spend: Spend,
        fee_rate: u128,
        expected: Option<Expected>,
        aux_rand: &[u8; 32],
    ) -> Result<Sent, CliError> {
        let (planned, account) =
            self.plan_send(datadir, passphrase, None, to, spend, fee_rate, |wallet| {
                private_account(wallet, passphrase, password)
            })?;
        if let Some(expected) = expected {
            let now = planned.preview().expected();
            if now != expected {
                return Err(CliError::PreviewChanged {
                    amount: now.amount.as_quanta(),
                    fee: now.fee.as_quanta(),
                });
            }
        }
        self.sign_and_send(planned, &account, aux_rand)
    }

    /// Menyusun rencana kirim: alamat, node, file wallet, lalu `unlock` (kunci akun rahasia untuk kirim, atau tanpa kunci
    /// untuk pratinjau) sebelum pemindaian, sama urutannya dengan `send` sebelum TSK-M68. UTXO yang sedang dibelanjakan
    /// transaksi di mempool tidak dipilih; dengan `from` hanya UTXO alamat itu (TSK-M63).
    #[allow(clippy::too_many_arguments)]
    fn plan_send<K>(
        &self,
        datadir: &Path,
        passphrase: &str,
        from: Option<&str>,
        to: &str,
        spend: Spend,
        fee_rate: u128,
        unlock: impl FnOnce(&OpenWallet) -> Result<K, CliError>,
    ) -> Result<(Planned, K), CliError> {
        let (recipient_kind, recipient_payload) = decode_address(self.network, to)?;
        let source = match from {
            Some(address) => {
                let (kind, payload) =
                    decode_address(self.network, address).map_err(CliError::SourceAddress)?;
                Some((address, kind, payload))
            }
            None => None,
        };
        let info = self.checked_info()?;
        let network_id = self
            .retry(|| self.client.get_block_hash(0))
            .map_err(rpc_error)?
            .to_hash32();
        let mut wallet = open_wallet(self.network, datadir, passphrase)?;
        let key = unlock(&wallet)?;
        let scanned = self.scan(&mut wallet)?;
        let source = match source {
            Some((address, kind, payload)) => {
                let owned = kind == OutputKind::SingleKey
                    && wallet_payloads(&wallet.public, scanned.next_index)
                        .contains(&Hex32::new(payload));
                if !owned {
                    return Err(CliError::SourceNotOwned {
                        address: address.to_owned(),
                    });
                }
                Some(payload)
            }
            None => None,
        };
        let pending = self.pending(&wallet, &scanned)?;
        let coins: Vec<Coin> = scanned
            .found
            .iter()
            .filter(|found| !pending.spent.contains(&(found.utxo.txid, found.utxo.index)))
            .filter(|found| source.is_none_or(|payload| *found.utxo.payload.bytes() == payload))
            .map(|found| {
                Coin::new(
                    OutPoint::new(found.utxo.txid.to_hash32(), found.utxo.index),
                    found.utxo.amount.amount(),
                    *found.utxo.payload.bytes(),
                    found.utxo.spendable_at,
                    found.branch,
                    found.index,
                )
            })
            .collect();
        let (chosen, change_index) = match spend {
            Spend::Amount(amount) => {
                let (change_index, change_payload) = match source {
                    Some(payload) => (None, payload),
                    None => {
                        let (index, payload) = derive_unused(
                            &wallet.public,
                            CHANGE_BRANCH,
                            wallet.file.next_index.change,
                        )?;
                        (Some(index), payload)
                    }
                };
                let request = SendRequest {
                    recipient_kind,
                    recipient_payload,
                    amount,
                    fee_rate,
                    change_payload,
                    tip_height: info.height,
                };
                (plan(&coins, &request)?, change_index)
            }
            Spend::All => {
                let request = SweepRequest {
                    recipient_kind,
                    recipient_payload,
                    fee_rate,
                    tip_height: info.height,
                };
                (plan_sweep(&coins, &request)?, None)
            }
        };
        let planned = Planned {
            wallet,
            network_id,
            chosen,
            change_index,
            tip_height: info.height,
        };
        Ok((planned, key))
    }

    /// Menandatangani rencana dengan `account`, mengirimnya ke mempool node, lalu menaikkan indeks kembalian di file
    /// wallet jika transaksi memakai output kembalian.
    fn sign_and_send(
        &self,
        mut planned: Planned,
        account: &ExtendedPrivKey,
        aux_rand: &[u8; 32],
    ) -> Result<Sent, CliError> {
        let tx = sign_plan(&planned.chosen, account, &planned.network_id, aux_rand)?;
        let bytes = tx.to_bytes();
        let result = self
            .retry(|| self.client.send_raw_transaction(&bytes))
            .map_err(|error| match reject_reason(&error) {
                Some(reason) => CliError::Rejected { reason },
                None => rpc_error(error),
            })?;
        if planned.chosen.change().is_some()
            && let Some(index) = planned.change_index
        {
            planned.wallet.file.next_index.change =
                index.checked_add(1).ok_or(CliError::IndexExhausted)?;
            wallet_file::save(&planned.wallet.path, &planned.wallet.file)?;
        }
        Ok(Sent {
            txid: result.txid,
            amount: planned.recipient_amount(),
            fee: planned.chosen.fee(),
            change: planned.chosen.change(),
            inputs: planned.chosen.coins().len(),
            size: planned.chosen.size(),
        })
    }
}

/// Teks keluaran `info`.
#[must_use]
pub fn format_info(info: &ChainInfo) -> String {
    format!(
        "jaringan: {}\ntinggi: {}\nujung: {}\nhasil tambang: {}\ntotal pasokan: {}\nmempool: {} transaksi\n",
        String::from(info.network),
        info.height,
        String::from(info.best_block_id),
        info.mined_total.amount(),
        info.total_supply.amount(),
        info.mempool_count,
    )
}

/// Teks keluaran `balance`.
#[must_use]
pub fn format_balance(balance: &Balance) -> String {
    format!(
        "tinggi ujung: {}\nsaldo: {}\nbisa dipakai: {}\nbelum matang: {}\nkeluar tertunda: {}\nmasuk tertunda: {}\nsaldo setelah tertunda masuk blok: {}\nutxo: {}\n",
        balance.tip_height,
        balance.total,
        balance.spendable,
        balance.immature,
        balance.pending_spent,
        balance.pending_received,
        balance.after_pending,
        balance.utxos,
    )
}

/// Teks keluaran `send`.
#[must_use]
pub fn format_sent(sent: &Sent) -> String {
    let change = sent
        .change
        .map_or_else(|| "tidak ada".to_owned(), |amount| amount.to_string());
    format!(
        "txid: {}\ndikirim: {}\nbiaya: {}\nkembalian: {}\ninput: {}\nukuran: {} byte\n",
        String::from(sent.txid),
        sent.amount,
        sent.fee,
        change,
        sent.inputs,
        sent.size,
    )
}

/// Teks keluaran `history`: satu baris per transaksi, terbaru dulu.
#[must_use]
pub fn format_history(history: &History) -> String {
    let mut lines = vec![
        format!("tinggi ujung: {}", history.tip_height),
        format!("transaksi: {}", history.entries.len()),
    ];
    for entry in &history.entries {
        let place = entry
            .height
            .map_or_else(|| "tertunda".to_owned(), |height| format!("blok {height}"));
        let fee = entry
            .fee
            .map_or_else(String::new, |fee| format!(" biaya {fee}"));
        let kind = if entry.coinbase { " coinbase" } else { "" };
        lines.push(format!(
            "{place} {} masuk {} keluar {}{fee}{kind}",
            String::from(entry.txid),
            entry.received,
            entry.spent,
        ));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// Bahan acak untuk mengunci frasa: salt dari satu panggilan `random`, nonce dari panggilan berikutnya.
fn protection<'a>(
    password: &'a str,
    random: &mut impl FnMut() -> Result<[u8; 32], CliError>,
) -> Result<Protection<'a>, CliError> {
    let first = random()?;
    let second = random()?;
    let mut salt = [0_u8; SALT_LEN];
    salt.iter_mut()
        .zip(first.iter())
        .for_each(|(slot, byte)| *slot = *byte);
    let mut nonce = [0_u8; NONCE_LEN];
    nonce
        .iter_mut()
        .zip(second.iter())
        .for_each(|(slot, byte)| *slot = *byte);
    Ok(Protection {
        password,
        kdf: KdfParams::DEFAULT,
        salt,
        nonce,
    })
}

/// Password baru: ditanya dua kali, lalu diperiksa [`check_new_password`].
fn new_password(
    password: &mut impl FnMut(PasswordPrompt) -> Result<Zeroizing<String>, CliError>,
) -> Result<Zeroizing<String>, CliError> {
    let first = password(PasswordPrompt::New)?;
    let second = password(PasswordPrompt::Confirm)?;
    check_new_password(&first, &second)?;
    Ok(first)
}

/// Password wallet yang sudah ada, hanya ditanya jika file wallet terenkripsi.
fn unlock_password(
    network: Network,
    datadir: &Path,
    password: &mut impl FnMut(PasswordPrompt) -> Result<Zeroizing<String>, CliError>,
) -> Result<Zeroizing<String>, CliError> {
    let path = wallet_file::wallet_path(network, datadir);
    if wallet_file::load_network(network, &path)?.is_encrypted() {
        password(PasswordPrompt::Unlock)
    } else {
        Ok(Zeroizing::new(String::new()))
    }
}

/// Teks frasa baru di keluaran `wallet-create`.
fn phrase_notice(mnemonic: &Mnemonic) -> String {
    format!(
        "file wallet dikunci password (Argon2id + XChaCha20-Poly1305)\nfrasa pemulihan ({} kata; simpan baik-baik di luar komputer, password tidak bisa menggantikannya):\n{}\n",
        mnemonic.word_count(),
        mnemonic.phrase(),
    )
}

/// Menjalankan satu aksi dan mengembalikan teks keluarannya. `random` dipanggil untuk entropi frasa
/// (`wallet-create`), salt dan nonce enkripsi (`wallet-create`, `wallet-restore`, `wallet-encrypt`), dan `aux_rand`
/// tanda tangan (`send`). `password` dipanggil hanya saat password dibutuhkan (lihat [`PasswordPrompt`]).
///
/// # Errors
///
/// [`CliError`] dari aksi itu.
pub fn run(
    config: &Config,
    mut random: impl FnMut() -> Result<[u8; 32], CliError>,
    mut password: impl FnMut(PasswordPrompt) -> Result<Zeroizing<String>, CliError>,
) -> Result<String, CliError> {
    let datadir = || config.datadir.as_deref().ok_or(CliError::MissingDatadir);
    let network = config.network;
    match &config.action {
        Action::Info => Ok(format_info(
            &Cli::connect(&config.rpc_url, network)?.info()?,
        )),
        Action::WalletCreate => {
            let datadir = datadir()?;
            let secret = new_password(&mut password)?;
            let entropy = random()?;
            let lock = protection(&secret, &mut random)?;
            let (mnemonic, path) =
                wallet_create(network, datadir, &entropy, &config.passphrase, &lock)?;
            Ok(format!(
                "wallet dibuat: {}\n{}",
                path.display(),
                phrase_notice(&mnemonic)
            ))
        }
        Action::WalletRestore { mnemonic } => {
            let datadir = datadir()?;
            let secret = new_password(&mut password)?;
            let lock = protection(&secret, &mut random)?;
            let path = wallet_restore(network, datadir, mnemonic, &config.passphrase, &lock)?;
            Ok(format!(
                "wallet dipulihkan: {}\nfile wallet dikunci password; jalankan balance agar alamat yang sudah terpakai ikut terpindai\n",
                path.display(),
            ))
        }
        Action::WalletEncrypt => {
            let datadir = datadir()?;
            let path = wallet_file::wallet_path(network, datadir);
            if wallet_file::load_network(network, &path)?.is_encrypted() {
                return Err(CliError::AlreadyEncrypted);
            }
            let secret = new_password(&mut password)?;
            let lock = protection(&secret, &mut random)?;
            let path = wallet_encrypt(network, datadir, &config.passphrase, &lock)?;
            Ok(format!(
                "file wallet dikunci password: {}\n",
                path.display()
            ))
        }
        Action::WalletShowPhrase => {
            let datadir = datadir()?;
            let secret = unlock_password(network, datadir, &mut password)?;
            let phrase = show_phrase(network, datadir, &secret)?;
            let words = phrase.split(' ').count();
            Ok(format!(
                "frasa pemulihan ({words} kata):\n{}\n",
                phrase.as_str()
            ))
        }
        Action::Address => {
            let address = next_address(network, datadir()?, &config.passphrase)?;
            let (_kind, payload) = decode_address(network, &address)?;
            Ok(format!(
                "alamat: {address}\npayload: {}\n",
                String::from(Hex32::new(payload))
            ))
        }
        Action::Balance => {
            let cli = Cli::connect(&config.rpc_url, network)?;
            Ok(format_balance(
                &cli.balance(datadir()?, &config.passphrase)?,
            ))
        }
        Action::History => {
            let cli = Cli::connect(&config.rpc_url, network)?;
            Ok(format_history(
                &cli.history(datadir()?, &config.passphrase)?,
            ))
        }
        Action::Send {
            to,
            amount,
            fee_rate,
            from,
        } => {
            let datadir = datadir()?;
            let secret = unlock_password(network, datadir, &mut password)?;
            let cli = Cli::connect(&config.rpc_url, network)?;
            let aux_rand = random()?;
            let sent = cli.send_from(
                datadir,
                &config.passphrase,
                &secret,
                from.as_deref(),
                to,
                *amount,
                *fee_rate,
                &aux_rand,
            )?;
            Ok(format_sent(&sent))
        }
    }
}

/// Membaca 32 byte dari sumber acak sistem operasi lewat `getrandom` (Linux `getrandom(2)`, Windows `ProcessPrng`,
/// macOS `getentropy`), agar CLI dan wallet desktop berjalan di Unix maupun Windows (DEC-31).
///
/// # Errors
///
/// [`CliError::Random`] jika sumber acak tidak bisa dibaca.
pub fn os_random() -> Result<[u8; 32], CliError> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| CliError::Random(std::io::Error::other(error.to_string())))?;
    Ok(bytes)
}

#[cfg(test)]
mod tests;
