//! Device-bound sealing of the Gemini API key (P2).
//!
//! Windows seals with DPAPI for the signed-in user. Elsewhere this module
//! has no secure store and refuses (`None`), so a key is never kept under a
//! key stored next to it: on Android sealing belongs to the Android Keystore
//! behind `Platform::seal`, which the Kotlin plugin provides.

#[cfg(windows)]
pub fn seal(data: &[u8]) -> Option<Vec<u8>> {
    dpapi(data, true)
}

#[cfg(windows)]
pub fn unseal(data: &[u8]) -> Option<Vec<u8>> {
    dpapi(data, false)
}

#[cfg(not(windows))]
pub fn seal(_data: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
pub fn unseal(_data: &[u8]) -> Option<Vec<u8>> {
    None
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

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn sealed_key_round_trips_and_hides_the_plain_text() {
        let sealed = super::seal(b"AIzaSySecretKey1234").unwrap();
        assert!(!sealed.windows(10).any(|w| w == b"SecretKey1"));
        assert_eq!(super::unseal(&sealed).unwrap(), b"AIzaSySecretKey1234");
    }
}
