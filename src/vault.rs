//! Locked sections: pages are encrypted with AES-256-GCM.
//! - `v = 1` (current): Argon2id (19 MiB, t=2, p=1) + AES-256-GCM with AAD binding `section_id || salt || v`.
//! - `v = 0` (legacy): PBKDF2-HMAC-SHA256 (300,000 rounds) + AES-256-GCM with empty AAD (decrypted for backward
//!   compatibility and transparently upgraded to `v = 1` on unlock).
//!
//! Keys and plaintext buffers are zeroized on drop.
use crate::model::{Lock, Page};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine;
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use std::num::NonZeroU32;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

pub const V_PBKDF2: u8 = 0;
pub const V_ARGON2: u8 = 1;

const PBKDF2_ROUNDS: u32 = 300_000;
/// OWASP mobile-safe Argon2id parameters: 19 MiB, 2 iterations, 1 lane.
const ARGON2_M_KIB: u32 = 19 * 1024;
const ARGON2_T_COST: u32 = 2;
const ARGON2_P_COST: u32 = 1;

/// 256-bit symmetric key that scrubs itself from memory when dropped.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Key([u8; 32]);

impl Key {
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

fn b64() -> base64::engine::GeneralPurpose { base64::engine::general_purpose::STANDARD }

fn random<const N: usize>() -> Result<[u8; N], String> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn aad_v1(section_id: u64, salt: &str, v: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(14 + 8 + salt.len() + 1);
    out.extend_from_slice(b"leafnotes-v1\0");
    out.extend_from_slice(&section_id.to_le_bytes());
    out.extend_from_slice(salt.as_bytes());
    out.push(v);
    out
}

/// Derive a 256-bit key for lock version `v` (`0` = PBKDF2 legacy, `1` = Argon2id).
pub fn derive_key(password: &str, salt_b64: &str, v: u8) -> Result<Key, String> {
    let salt = b64().decode(salt_b64).map_err(|e| e.to_string())?;
    let mut k = [0u8; 32];
    match v {
        V_PBKDF2 => {
            ring::pbkdf2::derive(
                ring::pbkdf2::PBKDF2_HMAC_SHA256,
                NonZeroU32::new(PBKDF2_ROUNDS).expect("non-zero"),
                &salt,
                password.as_bytes(),
                &mut k,
            );
            Ok(Key(k))
        }
        V_ARGON2 => {
            let params = Params::new(ARGON2_M_KIB, ARGON2_T_COST, ARGON2_P_COST, Some(32))
                .map_err(|e| e.to_string())?;
            let a2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
            a2.hash_password_into(password.as_bytes(), &salt, &mut k)
                .map_err(|e| e.to_string())?;
            Ok(Key(k))
        }
        _ => Err("This locked section was encrypted by a newer version of Leaf Notes.".into()),
    }
}

/// Derive a current (`v = 1` Argon2id) key for a password and salt.
pub fn key(password: &str, salt_b64: &str) -> Result<Key, String> {
    derive_key(password, salt_b64, V_ARGON2)
}

/// A new 128-bit salt (once per locked section).
pub fn new_salt() -> Result<String, String> { Ok(b64().encode(random::<16>()?)) }

/// Encrypt pages under `key` (`v = 1` with fresh nonce and AAD binding `section_id + salt + v`).
pub fn seal(key: &Key, section_id: u64, salt: &str, pages: &[Page]) -> Result<Lock, String> {
    let mut data = Zeroizing::new(serde_json::to_vec(pages).map_err(|e| e.to_string())?);
    let nonce = random::<12>()?;
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key.as_bytes()).map_err(|_| "bad key".to_string())?);
    let aad = aad_v1(section_id, salt, V_ARGON2);
    k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(&aad), &mut *data)
        .map_err(|_| "encryption failed".to_string())?;
    Ok(Lock { v: V_ARGON2, salt: salt.to_string(), nonce: b64().encode(nonce), data: b64().encode(&*data) })
}

/// Legacy `v = 0` seal (used only in tests to verify migration from older vaults).
#[cfg(test)]
pub fn seal_v0_for_test(key: &Key, salt: &str, pages: &[Page]) -> Result<Lock, String> {
    let mut data = Zeroizing::new(serde_json::to_vec(pages).map_err(|e| e.to_string())?);
    let nonce = random::<12>()?;
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key.as_bytes()).map_err(|_| "bad key".to_string())?);
    k.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::empty(), &mut *data)
        .map_err(|_| "encryption failed".to_string())?;
    Ok(Lock { v: V_PBKDF2, salt: salt.to_string(), nonce: b64().encode(nonce), data: b64().encode(&*data) })
}

/// Decrypt a locked section (supports `v = 0` legacy and `v = 1` Argon2id+AAD).
pub fn open(key: &Key, section_id: u64, lock: &Lock) -> Result<Vec<Page>, String> {
    if lock.v > V_ARGON2 {
        return Err("This locked section was encrypted by a newer version of Leaf Notes.".into());
    }
    let nonce: [u8; 12] = b64().decode(&lock.nonce).ok().and_then(|n| n.try_into().ok()).ok_or("damaged lock")?;
    let mut data = Zeroizing::new(b64().decode(&lock.data).map_err(|_| "damaged lock")?);
    let k = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key.as_bytes()).map_err(|_| "bad key")?);
    let aad_buf = if lock.v == V_ARGON2 { aad_v1(section_id, &lock.salt, lock.v) } else { Vec::new() };
    let plain = k.open_in_place(Nonce::assume_unique_for_key(nonce), Aad::from(&aad_buf[..]), &mut data)
        .map_err(|_| "wrong password".to_string())?;
    serde_json::from_slice(plain).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seals_and_opens() {
        let salt = new_salt().unwrap();
        let k = key("correct horse", &salt).unwrap();
        let pages = vec![Page::new("Diary"), Page::new("Plans")];
        let lock = seal(&k, 42, &salt, &pages).unwrap();
        assert_eq!(lock.v, V_ARGON2);
        assert!(!lock.data.contains("Diary"), "nothing readable in the saved form");
        let back = open(&k, 42, &lock).unwrap();
        assert_eq!(back.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Diary", "Plans"]);
        assert!(open(&key("wrong", &salt).unwrap(), 42, &lock).is_err());
        // AAD binds section_id and salt: moving ciphertext to another section or altering salt fails authentication.
        assert!(open(&k, 99, &lock).is_err(), "section_id mismatch must fail AAD");
        let mut bad_salt = lock.clone();
        bad_salt.salt = new_salt().unwrap();
        assert!(open(&k, 42, &bad_salt).is_err(), "salt mismatch must fail AAD");
        let mut broken = lock.clone();
        broken.data.replace_range(4..8, "AAAA");
        assert!(open(&k, 42, &broken).is_err(), "tampering is detected");
        assert_ne!(seal(&k, 42, &salt, &pages).unwrap().nonce, lock.nonce, "a fresh nonce each time");
    }

    #[test]
    fn opens_legacy_v0_pbkdf2_vault() {
        let salt = new_salt().unwrap();
        let k0 = derive_key("legacy-pass", &salt, V_PBKDF2).unwrap();
        let pages = vec![Page::new("Old Secret")];
        let lock0 = seal_v0_for_test(&k0, &salt, &pages).unwrap();
        assert_eq!(lock0.v, V_PBKDF2);
        let back = open(&k0, 123, &lock0).unwrap();
        assert_eq!(back[0].title, "Old Secret");
    }
}
