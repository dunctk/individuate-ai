//! Cryptographic primitives used by the per-user privacy boundary.
//!
//! This module deliberately contains no database or HTTP code.  A DEK is
//! generated once for a user, while every credential gets an independent
//! authenticated sealed copy of it.  Content encryption uses the DEK
//! directly, with a fresh nonce for every value.

use anyhow::{anyhow, Result};
use chacha20poly1305::{aead::Aead, KeyInit, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::{rngs::OsRng, RngCore};
use sha2::{Digest, Sha256};

pub const DEK_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;

pub fn dek_verifier(dek: &[u8]) -> [u8; DEK_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(b"individuateai dek verifier\0");
    hasher.update(dek);
    hasher.finalize().into()
}

fn derive_key(secret: &[u8], context: &[u8]) -> [u8; DEK_LEN] {
    let hk = Hkdf::<Sha256>::new(Some(context), secret);
    let mut key = [0u8; DEK_LEN];
    hk.expand(b"individuateai key v1", &mut key)
        .expect("HKDF output length is valid");
    key
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut value = [0u8; N];
    OsRng.fill_bytes(&mut value);
    value
}

pub fn generate_dek() -> [u8; DEK_LEN] {
    random_bytes()
}

pub fn generate_salt() -> [u8; 32] {
    random_bytes()
}

pub fn seal(dek: &[u8], wrapping_secret: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if dek.len() != DEK_LEN {
        return Err(anyhow!("DEK must be exactly 32 bytes"));
    }
    let key = derive_key(wrapping_secret, b"individuateai dek wrap");
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = random_bytes::<NONCE_LEN>();
    let mut sealed = nonce.to_vec();
    sealed.extend(
        cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                chacha20poly1305::aead::Payload { msg: dek, aad },
            )
            .map_err(|_| anyhow!("DEK sealing failed"))?,
    );
    Ok(sealed)
}

pub fn open(sealed: &[u8], wrapping_secret: &[u8], aad: &[u8]) -> Result<[u8; DEK_LEN]> {
    if sealed.len() <= NONCE_LEN {
        return Err(anyhow!("sealed DEK is truncated"));
    }
    let key = derive_key(wrapping_secret, b"individuateai dek wrap");
    let cipher = XChaCha20Poly1305::new((&key).into());
    let plaintext = cipher
        .decrypt(
            XNonce::from_slice(&sealed[..NONCE_LEN]),
            chacha20poly1305::aead::Payload {
                msg: &sealed[NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| anyhow!("DEK unwrap failed"))?;
    plaintext
        .try_into()
        .map_err(|_| anyhow!("unwrapped DEK has the wrong length"))
}

pub fn encrypt(dek: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if dek.len() != DEK_LEN {
        return Err(anyhow!("DEK must be exactly 32 bytes"));
    }
    let cipher = XChaCha20Poly1305::new(dek.into());
    let nonce = random_bytes::<NONCE_LEN>();
    let mut encrypted = nonce.to_vec();
    encrypted.extend(
        cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                chacha20poly1305::aead::Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| anyhow!("content encryption failed"))?,
    );
    Ok(encrypted)
}

pub fn decrypt(dek: &[u8], encrypted: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if dek.len() != DEK_LEN || encrypted.len() <= NONCE_LEN {
        return Err(anyhow!("invalid encrypted content"));
    }
    let cipher = XChaCha20Poly1305::new(dek.into());
    cipher
        .decrypt(
            XNonce::from_slice(&encrypted[..NONCE_LEN]),
            chacha20poly1305::aead::Payload {
                msg: &encrypted[NONCE_LEN..],
                aad,
            },
        )
        .map_err(|_| anyhow!("content authentication failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_round_trip_and_tamper_detection() {
        let dek = generate_dek();
        let mut encrypted = encrypt(&dek, b"private therapy note", b"messages/content").unwrap();
        assert_eq!(
            decrypt(&dek, &encrypted, b"messages/content").unwrap(),
            b"private therapy note"
        );
        encrypted[NONCE_LEN] ^= 1;
        assert!(decrypt(&dek, &encrypted, b"messages/content").is_err());
    }

    #[test]
    fn wrapped_dek_round_trip() {
        let dek = generate_dek();
        let wrap_secret = random_bytes::<32>();
        let sealed = seal(&dek, &wrap_secret, b"user/credential").unwrap();
        assert_eq!(
            open(&sealed, &wrap_secret, b"user/credential").unwrap(),
            dek
        );
        assert!(open(&sealed, b"wrong", b"user/credential").is_err());
    }

    #[test]
    fn dek_verifier_is_stable_and_key_specific() {
        let dek = generate_dek();
        assert_eq!(dek_verifier(&dek), dek_verifier(&dek));
        assert_ne!(dek_verifier(&dek), dek_verifier(&generate_dek()));
    }
}
