//! Private on-disk cache for a proof-verified room PiaKey.
//!
//! The caller decides whether the key is current. This module only ensures
//! that a cache changed by another process is not intentionally replaced.

use crate::Result;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Eq, PartialEq)]
struct Snapshot {
    digest: [u8; 32],
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

fn digest(key: &[u8; 16]) -> [u8; 32] {
    Sha256::digest(key).into()
}

fn snapshot(path: &Path) -> Result<Option<Snapshot>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("PiaKey cache cannot be opened: {error}")),
    };
    let metadata = file
        .metadata()
        .map_err(|error| format!("PiaKey cache metadata unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() != 16 {
        return Err("PiaKey cache must be a regular 16-byte file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("PiaKey cache must have mode 0600 or stricter".into());
        }
    }
    let mut bytes = Zeroizing::new([0u8; 16]);
    file.read_exact(&mut *bytes)
        .map_err(|error| format!("PiaKey cache changed while reading: {error}"))?;
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .map_err(|error| format!("PiaKey cache unreadable: {error}"))?
        != 0
    {
        return Err("PiaKey cache changed while reading".into());
    }
    Ok(Some(Snapshot {
        digest: digest(&bytes),
        #[cfg(unix)]
        device: {
            use std::os::unix::fs::MetadataExt;
            metadata.dev()
        },
        #[cfg(unix)]
        inode: {
            use std::os::unix::fs::MetadataExt;
            metadata.ino()
        },
    }))
}

struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub struct ManagedPiaCache {
    path: PathBuf,
    expected: Option<Snapshot>,
}

impl ManagedPiaCache {
    /// Track the exact private file whose bytes were loaded into the decoder.
    pub fn loaded(path: PathBuf, loaded: &[u8; 16]) -> Result<Self> {
        let expected = snapshot(&path)?.ok_or("PiaKey cache disappeared before tracking")?;
        if expected.digest != digest(loaded) {
            return Err("PiaKey cache changed after loading".into());
        }
        Ok(Self {
            path,
            expected: Some(expected),
        })
    }

    /// Track a missing cache so a later proof-verified key can be saved.
    pub fn absent(path: PathBuf) -> Result<Self> {
        if snapshot(&path)?.is_some() {
            return Err("PiaKey cache already exists".into());
        }
        Ok(Self {
            path,
            expected: None,
        })
    }

    /// Refresh only if the file is still the one this instance previously saw.
    /// Returns false for an unchanged key or a concurrently replaced cache.
    pub fn save_verified(&mut self, key: &[u8; 16]) -> Result<bool> {
        let current = snapshot(&self.path)?;
        if current != self.expected {
            return Ok(false);
        }
        if current
            .as_ref()
            .is_some_and(|value| value.digest == digest(key))
        {
            return Ok(false);
        }
        let parent = self
            .path
            .parent()
            .ok_or("PiaKey cache path has no parent directory")?;
        let temporary = TempFile(parent.join(format!(".pia_key.{}.tmp", Uuid::new_v4())));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let mut file = options
            .open(&temporary.0)
            .map_err(|error| format!("PiaKey temporary file unavailable: {error}"))?;
        file.write_all(key)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("PiaKey temporary write failed: {error}"))?;
        drop(file);
        if snapshot(&self.path)? != self.expected {
            return Ok(false);
        }
        if self.expected.is_none() {
            match fs::hard_link(&temporary.0, &self.path) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    return Ok(false);
                },
                Err(error) => return Err(format!("PiaKey cache creation failed: {error}")),
            }
        } else {
            fs::rename(&temporary.0, &self.path)
                .map_err(|error| format!("PiaKey cache replacement failed: {error}"))?;
        }
        self.expected = snapshot(&self.path)?;
        Ok(true)
    }

    /// Remove only the file this instance most recently loaded or saved.
    pub fn remove_if_unchanged(&mut self) -> Result<bool> {
        if self.expected.is_none() || snapshot(&self.path)? != self.expected {
            return Ok(false);
        }
        fs::remove_file(&self.path)
            .map_err(|error| format!("stale PiaKey cache removal failed: {error}"))?;
        self.expected = None;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "../test_support/unit/pia_cache.rs"]
mod tests;
