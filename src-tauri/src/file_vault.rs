//! A stand-in for the device keystore where there is none to ask (the
//! contract tests and the browser preview off Windows): ChaCha20-Poly1305
//! under a random key kept in the data folder. It keeps the key off the
//! database in plain text, not away from someone holding the folder.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use std::path::Path;

fn device_key(dir: &Path) -> Option<[u8; 32]> {
    let path = dir.join("fake-keystore.key");
    if let Ok(b) = std::fs::read(&path) {
        return b.try_into().ok();
    }
    let mut k = [0u8; 32];
    getrandom::getrandom(&mut k).ok()?;
    std::fs::create_dir_all(dir).ok()?;
    std::fs::write(&path, k).ok()?;
    Some(k)
}

pub fn seal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).ok()?;
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(Nonce::from_slice(&nonce), data).ok()?);
    Some(out)
}

pub fn unseal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
    if data.len() < 12 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
    cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()
}
