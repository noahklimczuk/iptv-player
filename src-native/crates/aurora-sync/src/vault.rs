//! End-to-end encryption of synced records (`docs/APPLE_PORT_AND_SYNC.md` §3.5).
//!
//! Google stores ciphertext, keyed ids and timestamps, and nothing it could read: not
//! the provider passwords, not the viewing history, not even which films are on a list.
//!
//! - **The vault key** is 256 random bits, made by the household's first device. It
//!   reaches the server only *wrapped*: encrypted under a key derived from the **sync
//!   passphrase** with Argon2id ([`WrappedKey`]). The passphrase never leaves a device;
//!   each device asks for it once, unwraps the key, and keeps the key in the platform
//!   credential store.
//! - **Records** are XChaCha20-Poly1305 under a subkey of the vault key, with the
//!   document id as associated data, so a ciphertext cannot be moved to another record.
//! - **Document ids** are `HMAC-SHA256(subkey, sync key)`, so the server cannot tell a
//!   favourite from a film's progress, or one film from another.
//! - **The recovery code** is the vault key itself, written for a person: Crockford
//!   base32 with a checksum, in groups of five. A forgotten passphrase cannot be
//!   recovered -- that is what end-to-end means -- but the code restores the key.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::codec::Codec;
use crate::error::{Result, SyncError};
use crate::record::Record;

type HmacSha256 = Hmac<Sha256>;

const WRAP_AAD: &[u8] = b"aurora-sync/vault-key/v1";
const ID_LABEL: &[u8] = b"aurora-sync/doc-id/v1";
const RECORD_LABEL: &[u8] = b"aurora-sync/record/v1";
/// The first byte of every record ciphertext, so the format can change later without a
/// newer record being mistaken for a damaged one by an older app.
const RECORD_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VaultError {
    /// The passphrase does not open this vault. Wrapping is authenticated, so this is
    /// reported as what it is and never as "corrupt data" (§3.8).
    #[error("that passphrase does not match the one this household's sync was set up with")]
    WrongPassphrase,
    #[error("that recovery code is not valid; check it for typos")]
    BadRecoveryCode,
    #[error("the stored vault is damaged: {0}")]
    Damaged(String),
    #[error("could not get randomness from the system: {0}")]
    Entropy(String),
}

fn random<const N: usize>() -> std::result::Result<[u8; N], VaultError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|e| VaultError::Entropy(e.to_string()))?;
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() & 1 == 1 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The household's vault key.
#[derive(Clone, PartialEq, Eq)]
pub struct VaultKey([u8; 32]);

impl std::fmt::Debug for VaultKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VaultKey(..)")
    }
}

impl VaultKey {
    pub fn generate() -> std::result::Result<Self, VaultError> {
        Ok(Self(random()?))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// For the platform credential store.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    pub fn from_hex(s: &str) -> std::result::Result<Self, VaultError> {
        let bytes = unhex(s.trim()).ok_or_else(|| VaultError::Damaged("key".into()))?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| VaultError::Damaged("key length".into()))?;
        Ok(Self(arr))
    }

    fn subkey(&self, label: &[u8]) -> [u8; 32] {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.0).expect("any key length");
        mac.update(label);
        mac.finalize().into_bytes().into()
    }

    /// The recovery code: the key and a 16-bit checksum in Crockford base32, grouped in
    /// fives. 55 characters; the checksum catches a mistyped one.
    pub fn recovery_code(&self) -> String {
        let check = Sha256::digest(self.0);
        let mut data = self.0.to_vec();
        data.extend_from_slice(&check[..2]);
        let chars = base32_encode(&data);
        chars
            .as_bytes()
            .chunks(5)
            .map(|c| std::str::from_utf8(c).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("-")
    }

    /// Read a recovery code back. Forgiving about case, separators, and the letters
    /// people confuse with digits (I, L for 1; O for 0).
    pub fn from_recovery_code(code: &str) -> std::result::Result<Self, VaultError> {
        let data = base32_decode(code).ok_or(VaultError::BadRecoveryCode)?;
        if data.len() < 34 {
            return Err(VaultError::BadRecoveryCode);
        }
        let key: [u8; 32] = data[..32]
            .try_into()
            .map_err(|_| VaultError::BadRecoveryCode)?;
        if Sha256::digest(key)[..2] != data[32..34] {
            return Err(VaultError::BadRecoveryCode);
        }
        Ok(Self(key))
    }
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn base32_decode(code: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in code.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' | '\t' | '\n' | '\r' => continue,
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        };
        let value = CROCKFORD.iter().position(|&x| x as char == c)? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// Argon2id cost. The defaults are what ships; tests use a cheap set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m: u32,
    /// Iterations.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl Default for KdfParams {
    /// 64 MiB, three passes: about a second on a 2020 PC, and within what an Apple TV
    /// can allocate. Run once per device, at sign-in.
    fn default() -> Self {
        Self {
            m: 64 * 1024,
            t: 3,
            p: 1,
        }
    }
}

/// The vault key as the server stores it (`users/{uid}/meta/vault`): encrypted under the
/// passphrase, with what is needed to derive that key again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WrappedKey {
    pub kdf: KdfParams,
    /// Hex.
    pub salt: String,
    /// Hex.
    pub nonce: String,
    /// Hex: the vault key and its authentication tag.
    pub sealed: String,
}

fn derive(
    passphrase: &str,
    salt: &[u8],
    kdf: KdfParams,
) -> std::result::Result<[u8; 32], VaultError> {
    let params = Params::new(kdf.m, kdf.t, kdf.p, Some(32))
        .map_err(|e| VaultError::Damaged(format!("kdf parameters: {e}")))?;
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| VaultError::Damaged(format!("kdf: {e}")))?;
    Ok(out)
}

impl WrappedKey {
    pub fn wrap(key: &VaultKey, passphrase: &str) -> std::result::Result<Self, VaultError> {
        Self::wrap_with(key, passphrase, KdfParams::default())
    }

    pub fn wrap_with(
        key: &VaultKey,
        passphrase: &str,
        kdf: KdfParams,
    ) -> std::result::Result<Self, VaultError> {
        let salt: [u8; 16] = random()?;
        let nonce: [u8; NONCE_LEN] = random()?;
        let kek = derive(passphrase, &salt, kdf)?;
        let sealed = XChaCha20Poly1305::new((&kek).into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: key.as_bytes(),
                    aad: WRAP_AAD,
                },
            )
            .map_err(|_| VaultError::Damaged("seal".into()))?;
        Ok(Self {
            kdf,
            salt: hex(&salt),
            nonce: hex(&nonce),
            sealed: hex(&sealed),
        })
    }

    pub fn unwrap_key(&self, passphrase: &str) -> std::result::Result<VaultKey, VaultError> {
        let damaged = || VaultError::Damaged("wrapped key".into());
        let salt = unhex(&self.salt).ok_or_else(damaged)?;
        let nonce = unhex(&self.nonce).ok_or_else(damaged)?;
        let sealed = unhex(&self.sealed).ok_or_else(damaged)?;
        if nonce.len() != NONCE_LEN {
            return Err(damaged());
        }
        let kek = derive(passphrase, &salt, self.kdf)?;
        let opened = XChaCha20Poly1305::new((&kek).into())
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &sealed,
                    aad: WRAP_AAD,
                },
            )
            .map_err(|_| VaultError::WrongPassphrase)?;
        let key: [u8; 32] = opened.try_into().map_err(|_| damaged())?;
        Ok(VaultKey(key))
    }
}

/// The production [`Codec`]: keyed ids and authenticated encryption.
pub struct VaultCodec {
    id_key: [u8; 32],
    cipher: XChaCha20Poly1305,
}

impl VaultCodec {
    pub fn new(key: &VaultKey) -> Self {
        let record_key = key.subkey(RECORD_LABEL);
        Self {
            id_key: key.subkey(ID_LABEL),
            cipher: XChaCha20Poly1305::new((&record_key).into()),
        }
    }
}

impl Codec for VaultCodec {
    fn doc_id(&self, key: &str) -> String {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.id_key).expect("any key length");
        mac.update(key.as_bytes());
        hex(&mac.finalize().into_bytes())
    }

    fn encode(&self, record: &Record) -> Result<Vec<u8>> {
        let plain = serde_json::to_vec(record)?;
        let nonce: [u8; NONCE_LEN] = random().map_err(|e| SyncError::Protocol(e.to_string()))?;
        let doc_id = self.doc_id(&record.key);
        let sealed = self
            .cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plain,
                    aad: doc_id.as_bytes(),
                },
            )
            .map_err(|_| SyncError::Protocol("encryption failed".into()))?;
        let mut out = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
        out.push(RECORD_VERSION);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    fn decode(&self, doc_id: &str, bytes: &[u8]) -> Result<Record> {
        if bytes.len() < 1 + NONCE_LEN || bytes[0] != RECORD_VERSION {
            return Err(SyncError::Undecodable);
        }
        let (nonce, sealed) = bytes[1..].split_at(NONCE_LEN);
        let plain = self
            .cipher
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: sealed,
                    aad: doc_id.as_bytes(),
                },
            )
            .map_err(|_| SyncError::Undecodable)?;
        let record: Record = serde_json::from_slice(&plain).map_err(|_| SyncError::Undecodable)?;
        if self.doc_id(&record.key) != doc_id {
            return Err(SyncError::Undecodable);
        }
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::record::{Clocked, Record};
    use serde_json::json;

    const CHEAP: KdfParams = KdfParams { m: 64, t: 1, p: 1 };

    fn record(key: &str) -> Record {
        Record {
            key: key.into(),
            kind: "favorite".into(),
            ident: json!({"item": "mv:abc:9001"}),
            alive: Clocked {
                value: true,
                hlc: 7,
            },
            fields: Default::default(),
            writer: "pc".into(),
        }
    }

    #[test]
    fn the_right_passphrase_unwraps_and_a_wrong_one_says_so() {
        let key = VaultKey::generate().unwrap();
        let wrapped = WrappedKey::wrap_with(&key, "correct horse", CHEAP).unwrap();
        assert_eq!(wrapped.unwrap_key("correct horse").unwrap(), key);
        assert_eq!(
            wrapped.unwrap_key("wrong horse").unwrap_err(),
            VaultError::WrongPassphrase
        );
        // And the wrapped form survives the trip through Firestore as JSON.
        let back: WrappedKey =
            serde_json::from_str(&serde_json::to_string(&wrapped).unwrap()).unwrap();
        assert_eq!(back.unwrap_key("correct horse").unwrap(), key);
    }

    #[test]
    fn the_wrapped_key_does_not_contain_the_key() {
        let key = VaultKey::generate().unwrap();
        let wrapped =
            serde_json::to_string(&WrappedKey::wrap_with(&key, "p", CHEAP).unwrap()).unwrap();
        assert!(!wrapped.contains(&key.to_hex()));
    }

    #[test]
    fn a_recovery_code_round_trips_and_forgives_how_people_type() {
        let key = VaultKey::generate().unwrap();
        let code = key.recovery_code();
        assert_eq!(code.replace('-', "").len(), 55);
        assert_eq!(VaultKey::from_recovery_code(&code).unwrap(), key);
        let sloppy = code.to_lowercase().replace('-', " ").replace('0', "o");
        assert_eq!(VaultKey::from_recovery_code(&sloppy).unwrap(), key);
    }

    #[test]
    fn a_mistyped_recovery_code_is_caught() {
        let key = VaultKey::generate().unwrap();
        let code = key.recovery_code();
        let first = code.chars().next().unwrap();
        let swapped = if first == 'A' { 'B' } else { 'A' };
        let typo = format!("{swapped}{}", &code[1..]);
        assert_eq!(
            VaultKey::from_recovery_code(&typo).unwrap_err(),
            VaultError::BadRecoveryCode
        );
        assert!(VaultKey::from_recovery_code("short").is_err());
    }

    #[test]
    fn records_round_trip_and_reveal_nothing() {
        let codec = VaultCodec::new(&VaultKey::generate().unwrap());
        let r = record("favorite:{\"item\":\"mv:abc:9001\"}");
        let id = codec.doc_id(&r.key);
        let bytes = codec.encode(&r).unwrap();
        assert_eq!(codec.decode(&id, &bytes).unwrap(), r);

        assert_eq!(id.len(), 64);
        assert!(!id.contains("favorite"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("9001") && !text.contains("favorite"));
        // Fresh nonce every time: the same record does not encrypt the same way twice.
        assert_ne!(codec.encode(&r).unwrap(), bytes);
    }

    #[test]
    fn a_ciphertext_moved_to_another_document_does_not_decode() {
        let codec = VaultCodec::new(&VaultKey::generate().unwrap());
        let a = record("a");
        let b_id = codec.doc_id("b");
        let bytes = codec.encode(&a).unwrap();
        assert!(matches!(
            codec.decode(&b_id, &bytes),
            Err(SyncError::Undecodable)
        ));
    }

    #[test]
    fn another_households_key_cannot_read_it() {
        let ours = VaultCodec::new(&VaultKey::generate().unwrap());
        let theirs = VaultCodec::new(&VaultKey::generate().unwrap());
        let r = record("a");
        let bytes = ours.encode(&r).unwrap();
        assert_ne!(ours.doc_id("a"), theirs.doc_id("a"));
        assert!(theirs.decode(&ours.doc_id("a"), &bytes).is_err());
    }

    #[test]
    fn a_tampered_or_future_record_is_undecodable_not_a_panic() {
        let codec = VaultCodec::new(&VaultKey::generate().unwrap());
        let r = record("a");
        let id = codec.doc_id("a");
        let mut bytes = codec.encode(&r).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert!(codec.decode(&id, &bytes).is_err());
        let mut future = codec.encode(&r).unwrap();
        future[0] = 2;
        assert!(codec.decode(&id, &future).is_err());
        assert!(codec.decode(&id, &[]).is_err());
    }

    #[test]
    fn the_shipping_kdf_parameters_are_accepted() {
        // Not run at full cost here; only checks Argon2 accepts them.
        let d = KdfParams::default();
        assert!(Params::new(d.m, d.t, d.p, Some(32)).is_ok());
    }
}
