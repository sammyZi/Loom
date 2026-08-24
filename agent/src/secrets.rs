//! API keys at rest.
//!
//! Keys live in their own file, never in `config.json`, so nothing that
//! rewrites settings — or clears sessions — can touch them. On Windows the
//! file is sealed with DPAPI: the ciphertext is bound to the logged-in user
//! account, so another account (or a copy of the file lifted off the disk)
//! cannot read it. Elsewhere it is a 0600 file, which is what the OS gives us
//! without dragging in a keyring daemon.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// provider id -> api key.
pub type Keys = BTreeMap<String, String>;

/// Next to `config.json`, so one directory holds the whole user setup.
pub fn path() -> PathBuf {
    crate::settings::Settings::path().with_file_name("keys.dat")
}

/// A missing or unreadable file means "no keys yet" — a first run, a restored
/// profile, or a file sealed for a different user. Never an error the caller
/// has to handle, because the answer is the same in every case: ask again.
pub fn load() -> Keys {
    let Ok(raw) = std::fs::read(path()) else {
        return Keys::new();
    };
    match unseal(&raw) {
        Ok(plain) => serde_json::from_slice(&plain).unwrap_or_default(),
        Err(e) => {
            tracing::warn!("could not read stored keys ({e:#}); treating as empty");
            Keys::new()
        }
    }
}

pub fn store(keys: &Keys) -> Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    if keys.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let sealed = seal(&serde_json::to_vec(keys)?)?;
    // Write-then-rename, same as settings: a crash mid-write cannot shred keys.
    let tmp = path.with_extension("dat.tmp");
    std::fs::write(&tmp, sealed)?;
    restrict(&tmp);
    std::fs::rename(&tmp, &path).with_context(|| format!("save {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn restrict(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &std::path::Path) {}

#[cfg(windows)]
fn seal(plain: &[u8]) -> Result<Vec<u8>> {
    dpapi(plain, true)
}

#[cfg(windows)]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>> {
    dpapi(sealed, false)
}

/// CryptProtectData / CryptUnprotectData with the user's own login secret —
/// the same primitive browsers use for saved passwords.
#[cfg(windows)]
fn dpapi(input: &[u8], protect: bool) -> Result<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData,
    };

    let mut input_blob = CRYPT_INTEGER_BLOB {
        cbData: input.len() as u32,
        pbData: input.as_ptr() as *mut u8,
    };
    let mut out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        if protect {
            CryptProtectData(
                &mut input_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .context("DPAPI encrypt")?;
        } else {
            CryptUnprotectData(
                &mut input_blob,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .context("DPAPI decrypt")?;
        }
        let bytes = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        let _ = LocalFree(Some(HLOCAL(out.pbData as *mut std::ffi::c_void)));
        Ok(bytes)
    }
}

// ponytail: no OS keystore off Windows yet — file permissions only. Wire up
// Secret Service / Keychain when the app actually ships there.
#[cfg(not(windows))]
fn seal(plain: &[u8]) -> Result<Vec<u8>> {
    Ok(plain.to_vec())
}

#[cfg(not(windows))]
fn unseal(sealed: &[u8]) -> Result<Vec<u8>> {
    Ok(sealed.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_bytes_round_trip_and_hide_the_key() {
        let mut keys = Keys::new();
        keys.insert("openrouter".into(), "sk-secret-value".into());
        let plain = serde_json::to_vec(&keys).unwrap();

        let sealed = seal(&plain).unwrap();
        if cfg!(windows) {
            let text = String::from_utf8_lossy(&sealed);
            assert!(!text.contains("sk-secret-value"), "key must not survive in the clear");
        }
        let back: Keys = serde_json::from_slice(&unseal(&sealed).unwrap()).unwrap();
        assert_eq!(back.get("openrouter").map(String::as_str), Some("sk-secret-value"));
    }
}
