use super::*;

fn write_key(path: &Path, key: [u8; 16]) {
    use std::io::Write;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(&key).unwrap();
}

#[test]
fn verified_key_refresh_is_private_and_can_be_removed_if_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    write_key(&path, [1; 16]);
    let mut cache = ManagedPiaCache::loaded(path.clone(), &[1; 16]).unwrap();
    assert!(cache.save_verified(&[2; 16]).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), [2; 16]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(!cache.save_verified(&[2; 16]).unwrap());
    assert!(cache.remove_if_unchanged().unwrap());
    assert!(!path.exists());
}

#[test]
fn replacement_after_load_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    write_key(&path, [1; 16]);
    let mut cache = ManagedPiaCache::loaded(path.clone(), &[1; 16]).unwrap();
    std::fs::remove_file(&path).unwrap();
    write_key(&path, [3; 16]);
    assert!(!cache.remove_if_unchanged().unwrap());
    assert!(!cache.save_verified(&[2; 16]).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), [3; 16]);
}

#[test]
fn absent_cache_creates_but_does_not_replace_a_racing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pia_key.bin");
    let mut cache = ManagedPiaCache::absent(path.clone()).unwrap();
    write_key(&path, [3; 16]);
    assert!(!cache.save_verified(&[2; 16]).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), [3; 16]);
    std::fs::remove_file(&path).unwrap();
    assert!(cache.save_verified(&[2; 16]).unwrap());
    assert_eq!(std::fs::read(&path).unwrap(), [2; 16]);
}
