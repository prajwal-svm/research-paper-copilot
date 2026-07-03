//! The remote abstraction: three dumb verbs plus an atomic-ish manifest
//! swap. Anything needing a smarter server is out of scope by design.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("remote: {0}")]
    Io(#[from] std::io::Error),
    #[error("remote: {0}")]
    Backend(String),
    /// The conditional manifest write lost a race — re-pull and retry.
    #[error("remote: manifest generation conflict")]
    Conflict,
}

/// A dumb blob store. Keys are opaque flat strings (HMAC-derived names plus
/// a small fixed set of well-known keys like `key.salt` and manifests).
pub trait Remote: Send {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, RemoteError>;
    fn put(&self, key: &str, value: &[u8]) -> Result<(), RemoteError>;
    fn delete(&self, key: &str) -> Result<(), RemoteError>;
    fn list(&self, prefix: &str) -> Result<Vec<String>, RemoteError>;
    /// Create-only put: fails with [`RemoteError::Conflict`] if the key
    /// exists. The manifest-swap primitive (generation-numbered keys).
    fn put_if_absent(&self, key: &str, value: &[u8]) -> Result<(), RemoteError>;
    /// Human-readable destination for egress disclosure.
    fn describe(&self) -> String;
}

// ---------------------------------------------------------------------------
// Folder backend: iCloud Drive/Dropbox/Syncthing/USB/NAS as the transport.
// ---------------------------------------------------------------------------

/// Plain-directory remote. Atomicity via temp+rename; create-only via a
/// best-effort exclusive create (documented: eventually-consistent folder
/// transports prefer a single writer at a time — races degrade to a
/// redundant re-merge, never corruption, because journals union).
pub struct FolderRemote {
    root: PathBuf,
}

impl FolderRemote {
    pub fn new(root: PathBuf) -> std::io::Result<FolderRemote> {
        std::fs::create_dir_all(&root)?;
        Ok(FolderRemote { root })
    }

    fn path(&self, key: &str) -> PathBuf {
        // Keys are flat, opaque names; shard lightly to keep dirs sane.
        let shard = key.get(..2).unwrap_or("00");
        self.root.join(shard).join(key)
    }
}

impl Remote for FolderRemote {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, RemoteError> {
        match std::fs::read(self.path(key)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        let path = self.path(key);
        std::fs::create_dir_all(path.parent().expect("sharded"))?;
        let tmp = path.with_extension("tmp-write");
        std::fs::write(&tmp, value)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), RemoteError> {
        match std::fs::remove_file(self.path(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, RemoteError> {
        let mut keys = Vec::new();
        let Ok(shards) = std::fs::read_dir(&self.root) else {
            return Ok(keys);
        };
        for shard in shards.flatten() {
            if !shard.path().is_dir() {
                continue;
            }
            let Ok(files) = std::fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().to_string();
                if name.starts_with(prefix) && !name.ends_with(".tmp-write") {
                    keys.push(name);
                }
            }
        }
        keys.sort();
        Ok(keys)
    }

    fn put_if_absent(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        let path = self.path(key);
        std::fs::create_dir_all(path.parent().expect("sharded"))?;
        // Exclusive create is the strongest primitive a folder offers.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                use std::io::Write;
                file.write_all(value)?;
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(RemoteError::Conflict),
            Err(e) => Err(e.into()),
        }
    }

    fn describe(&self) -> String {
        format!("folder: {}", self.root.display())
    }
}

// ---------------------------------------------------------------------------
// S3 backend adapter (self-hosted MinIO / free-tier R2).
// ---------------------------------------------------------------------------

#[cfg(feature = "native")]
pub struct S3Remote {
    client: super::s3::S3Client,
}

#[cfg(feature = "native")]
impl S3Remote {
    pub fn new(config: super::s3::S3Config) -> S3Remote {
        S3Remote {
            client: super::s3::S3Client::new(config),
        }
    }

    pub fn ensure_bucket(&self) -> Result<(), RemoteError> {
        self.client
            .ensure_bucket()
            .map_err(|e| RemoteError::Backend(e.to_string()))
    }
}

#[cfg(feature = "native")]
impl Remote for S3Remote {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, RemoteError> {
        self.client
            .get(key)
            .map_err(|e| RemoteError::Backend(e.to_string()))
    }

    fn put(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        self.client
            .put(key, value)
            .map_err(|e| RemoteError::Backend(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<(), RemoteError> {
        self.client
            .delete(key)
            .map_err(|e| RemoteError::Backend(e.to_string()))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, RemoteError> {
        self.client
            .list(prefix)
            .map_err(|e| RemoteError::Backend(e.to_string()))
    }

    fn put_if_absent(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        match self.client.put_if_absent(key, value) {
            Ok(()) => Ok(()),
            Err(super::s3::S3Error::PreconditionFailed) => Err(RemoteError::Conflict),
            Err(e) => Err(RemoteError::Backend(e.to_string())),
        }
    }

    fn describe(&self) -> String {
        format!("s3: {}", self.client.host())
    }
}

// ---------------------------------------------------------------------------
// In-memory fake for engine tests.
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct MemoryRemote {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    /// Fail the next N puts (network-flake simulation).
    pub fail_next_puts: Mutex<u32>,
}

impl Remote for MemoryRemote {
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, RemoteError> {
        Ok(self.objects.lock().unwrap().get(key).cloned())
    }

    fn put(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        let mut failures = self.fail_next_puts.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            return Err(RemoteError::Backend("simulated network failure".into()));
        }
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<(), RemoteError> {
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, RemoteError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn put_if_absent(&self, key: &str, value: &[u8]) -> Result<(), RemoteError> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Err(RemoteError::Conflict);
        }
        objects.insert(key.to_string(), value.to_vec());
        Ok(())
    }

    fn describe(&self) -> String {
        "memory".to_string()
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_backend_verbs_and_exclusive_create() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = FolderRemote::new(tmp.path().join("icloud-like")).unwrap();
        assert!(remote.get("abc123").unwrap().is_none());
        remote.put("abc123", b"blob").unwrap();
        assert_eq!(remote.get("abc123").unwrap().unwrap(), b"blob");
        assert_eq!(remote.list("abc").unwrap(), vec!["abc123".to_string()]);

        remote.put_if_absent("manifest-000001", b"gen1").unwrap();
        assert!(matches!(
            remote.put_if_absent("manifest-000001", b"gen1-loser"),
            Err(RemoteError::Conflict)
        ));
        assert_eq!(remote.get("manifest-000001").unwrap().unwrap(), b"gen1");

        remote.delete("abc123").unwrap();
        assert!(remote.get("abc123").unwrap().is_none());
        remote.delete("abc123").unwrap(); // idempotent
    }
}
