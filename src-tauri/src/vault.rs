//! Device-bound sealing of the Gemini API key (P2). On Windows the key is
//! sealed with DPAPI for the signed-in user; elsewhere (the contract host on
//! Linux or macOS) with ChaCha20-Poly1305 under a random key kept in the
//! app-private folder. On Android the Kotlin plugin seals with the Android
//! Keystore through `Platform::seal`.

use std::path::Path;

#[cfg(windows)]
pub fn seal(data: &[u8], _dir: &Path) -> Option<Vec<u8>> {
    dpapi(data, true)
}

#[cfg(windows)]
pub fn unseal(data: &[u8], _dir: &Path) -> Option<Vec<u8>> {
    dpapi(data, false)
}

#[cfg(windows)]
fn dpapi(data: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: `input` points at `data` for the call; `out` is allocated by
    // DPAPI and freed with LocalFree after copying.
    unsafe {
        let ok = if protect {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        };
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(out.pbData as _);
        Some(bytes)
    }
}

#[cfg(not(windows))]
fn device_key(dir: &Path) -> Option<[u8; 32]> {
    let path = dir.join("device.key");
    if let Ok(b) = std::fs::read(&path) {
        return b.try_into().ok();
    }
    let mut k = [0u8; 32];
    getrandom::getrandom(&mut k).ok()?;
    std::fs::write(&path, k).ok()?;
    Some(k)
}

#[cfg(not(windows))]
pub fn seal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).ok()?;
    let mut out = nonce.to_vec();
    out.extend(cipher.encrypt(Nonce::from_slice(&nonce), data).ok()?);
    Some(out)
}

#[cfg(not(windows))]
pub fn unseal(data: &[u8], dir: &Path) -> Option<Vec<u8>> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
    if data.len() < 12 {
        return None;
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&device_key(dir)?));
    cipher.decrypt(Nonce::from_slice(&data[..12]), &data[12..]).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sealed_key_round_trips_and_hides_the_plain_text() {
        let dir = std::env::temp_dir().join(format!("rst-vault-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sealed = super::seal(b"AIzaSySecretKey1234", &dir).unwrap();
        assert!(!sealed.windows(10).any(|w| w == b"SecretKey1"));
        assert_eq!(super::unseal(&sealed, &dir).unwrap(), b"AIzaSySecretKey1234");
        let _ = std::fs::remove_dir_all(dir);
    }
}
