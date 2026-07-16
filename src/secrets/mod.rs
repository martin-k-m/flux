//! Flux Secrets — encrypted local variables.
//!
//! Secrets are declared in `.flux` (`secret DATABASE_URL`) and set with
//! `flux secret set NAME value`. Values are encrypted at rest with ChaCha20
//! (see [`chacha20`]) under a per-project key, and injected into steps that
//! list them in `env`.
//!
//! ## Threat model (be honest)
//!
//! The encryption key lives beside the ciphertext in `.flux-cache/secrets/.key`
//! (git-ignored). This protects secrets from *casual* exposure — they are never
//! stored in plaintext, won't show up in `grep`, and won't be committed. It is
//! **not** protection against an attacker who already has read access to the
//! project's `.flux-cache/` directory. For that, the key would need to come
//! from an OS keychain or an env-provided master key.

mod chacha20;

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// An encrypted secret store rooted at a project directory.
pub struct SecretStore {
    dir: PathBuf,
    key: [u8; 32],
}

impl SecretStore {
    /// Open a named environment's store (3.7 — environment separation). The
    /// same secret name can hold different values in `development` vs.
    /// `production`. Each environment has its own encryption key.
    pub fn open_env(project_root: &Path, environment: &str) -> io::Result<Self> {
        let safe_env: String = environment
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let dir = project_root
            .join(".flux-cache")
            .join("secrets")
            .join(safe_env);
        std::fs::create_dir_all(&dir)?;
        let key = load_or_create_key(&dir)?;
        Ok(SecretStore { dir, key })
    }

    /// Store (or overwrite) a secret.
    pub fn set(&self, name: &str, value: &str) -> io::Result<()> {
        let nonce = derive_nonce();
        let mut buf = value.as_bytes().to_vec();
        chacha20::xor(&self.key, &nonce, 0, &mut buf);

        // File layout: 12-byte nonce, then ciphertext.
        let mut out = Vec::with_capacity(12 + buf.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&buf);
        std::fs::write(self.secret_path(name), out)
    }

    /// Read and decrypt a secret, if present.
    pub fn get(&self, name: &str) -> io::Result<Option<String>> {
        let path = self.secret_path(name);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read(&path)?;
        if raw.len() < 12 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "corrupt secret file",
            ));
        }
        let (nonce_bytes, cipher) = raw.split_at(12);
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(nonce_bytes);
        let mut buf = cipher.to_vec();
        chacha20::xor(&self.key, &nonce, 0, &mut buf);
        Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
    }

    /// List stored secret names (not values).
    pub fn list(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let entry = entry?;
            let name = entry.file_name().into_string().unwrap_or_default();
            if let Some(base) = name.strip_suffix(".secret") {
                names.push(base.to_string());
            }
        }
        names.sort();
        Ok(names)
    }

    /// Resolve the values of `names` into an env map, skipping any that are
    /// unset. Used to inject secrets into a step's environment.
    pub fn resolve(&self, names: &[String]) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for name in names {
            if let Ok(Some(v)) = self.get(name) {
                map.insert(name.clone(), v);
            }
        }
        map
    }

    fn secret_path(&self, name: &str) -> PathBuf {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.secret"))
    }
}

/// Load the 32-byte key, generating one on first use.
fn load_or_create_key(dir: &Path) -> io::Result<[u8; 32]> {
    let key_path = dir.join(".key");
    if let Ok(bytes) = std::fs::read(&key_path) {
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    let key = generate_key();
    std::fs::write(&key_path, key)?;
    Ok(key)
}

/// Derive a 32-byte key from best-effort local entropy.
///
/// Note: without a `getrandom`-style crate (which won't link on this toolchain),
/// we mix several weak sources and hash them. Adequate for a casual-exposure
/// threat model; see the module docs.
fn generate_key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"flux-secret-key-v1");
    if let Ok(dur) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.update(dur.as_nanos().to_le_bytes());
    }
    hasher.update(std::process::id().to_le_bytes());
    // Address-space entropy (ASLR) as an extra source.
    let stack_marker = 0u8;
    hasher.update((&stack_marker as *const u8 as usize).to_le_bytes());
    let heap = Box::new(0u8);
    hasher.update((Box::into_raw(heap) as usize).to_le_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

/// Derive a fresh 12-byte nonce from local entropy.
fn derive_nonce() -> [u8; 12] {
    let mut hasher = Sha256::new();
    hasher.update(b"flux-nonce");
    if let Ok(dur) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.update(dur.as_nanos().to_le_bytes());
    }
    hasher.update(std::process::id().to_le_bytes());
    let marker = 0u8;
    hasher.update((&marker as *const u8 as usize).to_le_bytes());
    let digest = hasher.finalize();
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&digest[..12]);
    nonce
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut d = std::env::temp_dir();
        d.push(format!("flux-secret-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn set_get_roundtrip() {
        let root = temp_dir("rt");
        let store = SecretStore::open_env(&root, "default").unwrap();
        store
            .set("DATABASE_URL", "postgres://localhost/app")
            .unwrap();
        assert_eq!(
            store.get("DATABASE_URL").unwrap().as_deref(),
            Some("postgres://localhost/app")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn value_is_not_stored_in_plaintext() {
        let root = temp_dir("enc");
        let store = SecretStore::open_env(&root, "default").unwrap();
        store.set("API_KEY", "super-secret-token").unwrap();
        let raw = std::fs::read(root.join(".flux-cache/secrets/default/API_KEY.secret")).unwrap();
        assert!(
            !String::from_utf8_lossy(&raw).contains("super-secret-token"),
            "plaintext leaked into the secret file"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn list_reports_names_only() {
        let root = temp_dir("list");
        let store = SecretStore::open_env(&root, "default").unwrap();
        store.set("A", "1").unwrap();
        store.set("B", "2").unwrap();
        assert_eq!(store.list().unwrap(), vec!["A", "B"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
