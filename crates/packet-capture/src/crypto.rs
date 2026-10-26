//! Key values are not logged; authentication precedes every plaintext parse.
//! The key-generation material is supplied externally, never embedded here.
use crate::Result;
use aes::Aes128;
use aes::cipher::{Array, BlockCipherEncrypt};
use aes_gcm::aead::{
    AeadInOut, KeyInit,
    consts::{U8, U12},
};
use aes_gcm::{AesGcm, Nonce, Tag};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::path::Path;
use zeroize::{Zeroize, Zeroizing};

const ELF_SHA: &str = "a641a94f74b832ffb6f28e216147fb5a867790246809b270507611d554294357";
const GAME_KEY_SHA: &str = "b4bf8780605e411f4b307b4d5b5f19765e96e581c34a035c8358bc0a715dfc5e";
const RANDOM_TABLE_BYTES: usize = 272;
const MAX_PLAINTEXT: usize = 0x2263;
const MAX_COMPRESSED: usize = 1440;

pub struct GameKey(pub [u8; 16]);
pub struct PiaKey(pub [u8; 16]);

/// Private v1 raw material: four little-endian state words followed by 64
/// little-endian lookup words. This value must come from an external source.
pub struct KeyMaterial {
    state: [u32; 4],
    table: [u32; 64],
}

impl Drop for KeyMaterial {
    fn drop(&mut self) {
        self.state.zeroize();
        self.table.zeroize();
    }
}

impl Drop for GameKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}
impl Drop for PiaKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub fn read_game_key(path: &Path) -> Result<GameKey> {
    let bytes = read_private_bytes(path, 16)?;
    Ok(GameKey(bytes.as_slice().try_into().unwrap()))
}
pub fn read_pia_key(path: &Path) -> Result<PiaKey> {
    let bytes = read_private_bytes(path, 16)?;
    Ok(PiaKey(bytes.as_slice().try_into().unwrap()))
}

fn read_private_bytes(path: &Path, expected_len: usize) -> Result<Zeroizing<Vec<u8>>> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "private input unavailable")?;
    let meta = file.metadata().map_err(|_| "private input unreadable")?;
    if !meta.file_type().is_file() {
        return Err("private input must be a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("private input must have mode 0600 or stricter".into());
        }
    }
    if meta.len() != expected_len as u64 {
        return Err(format!(
            "private input must contain exactly {expected_len} bytes"
        ));
    }
    let mut bytes = Zeroizing::new(vec![0u8; expected_len]);
    file.read_exact(&mut bytes)
        .map_err(|_| "private input changed while reading")?;
    let mut extra = [0u8; 1];
    if file
        .read(&mut extra)
        .map_err(|_| "private input unreadable")?
        != 0
    {
        return Err("private input changed while reading".into());
    }
    Ok(bytes)
}

pub fn read_game_key_hex(value: &str) -> Result<GameKey> {
    if value.len() != 32 {
        return Err("GameKey must be 32 hex digits".into());
    }
    let mut bytes = [0u8; 16];
    if hex::decode_to_slice(value, &mut bytes).is_err() {
        bytes.zeroize();
        return Err("GameKey must be 32 hex digits".into());
    }
    Ok(GameKey(bytes))
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset + 2)
            .ok_or("ELF header truncated")?
            .try_into()
            .unwrap(),
    ))
}
fn le_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("ELF header truncated")?
            .try_into()
            .unwrap(),
    ))
}
fn le_u64(bytes: &[u8], offset: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or("ELF header truncated")?
            .try_into()
            .unwrap(),
    ))
}

fn elf_va(elf: &[u8], address: u64, length: usize) -> Result<&[u8]> {
    let phoff = le_u64(elf, 32)?;
    let entsize = le_u16(elf, 54)? as u64;
    let count = le_u16(elf, 56)? as u64;
    if entsize < 56 || count > 256 {
        return Err("ELF program headers invalid".into());
    }
    for index in 0..count {
        let at = phoff
            .checked_add(index.checked_mul(entsize).ok_or("ELF offsets invalid")?)
            .ok_or("ELF offsets invalid")?;
        let at = usize::try_from(at).map_err(|_| "ELF offsets invalid")?;
        if le_u32(elf, at)? != 1 {
            continue;
        }
        let offset = le_u64(elf, at + 8)?;
        let va = le_u64(elf, at + 16)?;
        let filesz = le_u64(elf, at + 32)?;
        if address >= va
            && address
                .checked_add(length as u64)
                .is_some_and(|end| end <= va.saturating_add(filesz))
        {
            let start = offset
                .checked_add(address - va)
                .ok_or("ELF offsets invalid")?;
            let start = usize::try_from(start).map_err(|_| "ELF offsets invalid")?;
            let end = start.checked_add(length).ok_or("ELF offsets invalid")?;
            return elf
                .get(start..end)
                .ok_or("ELF address not backed by file".into());
        }
    }
    Err("ELF address not backed by file".into())
}

impl KeyMaterial {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != RANDOM_TABLE_BYTES {
            return Err("random table v1 must contain exactly 272 bytes".into());
        }
        let mut state = [0u32; 4];
        let mut table = [0u32; 64];
        for (word, bytes) in state.iter_mut().zip(bytes[..16].as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        for (word, bytes) in table.iter_mut().zip(bytes[16..].as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        if state.iter().all(|word| *word == 0) || table.iter().all(|word| *word == 0) {
            return Err("random table state and lookup must be nonzero".into());
        }
        Ok(Self { state, table })
    }

    fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(RANDOM_TABLE_BYTES));
        for word in self.state.iter().chain(self.table.iter()) {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

fn supported_material(material: &KeyMaterial) -> bool {
    let key = generate_game_key(material);
    hex::encode(Sha256::digest(key.0)) == GAME_KEY_SHA
}

pub fn read_random_table(path: &Path) -> Result<KeyMaterial> {
    let bytes = read_private_bytes(path, RANDOM_TABLE_BYTES)?;
    let material = KeyMaterial::from_bytes(&bytes)?;
    if !supported_material(&material) {
        return Err("random table is not for the pinned Splatoon 3 11.3.0 GameKey".into());
    }
    Ok(material)
}

fn material_from_elf(path: &Path) -> Result<KeyMaterial> {
    let elf = Zeroizing::new(fs::read(path).map_err(|_| "game ELF unavailable")?);
    if elf.get(..6) != Some(&b"\x7fELF\x02\x01"[..]) {
        return Err("expected little-endian ELF64".into());
    }
    if hex::encode(Sha256::digest(&elf)) != ELF_SHA {
        return Err("game ELF fingerprint mismatch (requires Splatoon 3 11.3.0)".into());
    }
    let mut x8 = 0u64;
    let mut x9 = 0u64;
    for address in (0x150836c..0x150838c).step_by(4) {
        let ins = u32::from_le_bytes(elf_va(&elf, address, 4)?.try_into().unwrap());
        let target = match ins & 31 {
            8 => &mut x8,
            9 => &mut x9,
            _ => return Err("ELF key instruction register changed".into()),
        };
        let shift = ((ins >> 21) & 3) * 16;
        let value = (((ins >> 5) & 0xffff) as u64) << shift;
        match ins & 0xff80_0000 {
            0xd280_0000 => *target = value,
            0xf280_0000 => *target = (*target & !(0xffffu64 << shift)) | value,
            _ => return Err("ELF key instruction opcode changed".into()),
        }
    }
    let state = [x8 as u32, (x8 >> 32) as u32, x9 as u32, (x9 >> 32) as u32];
    let table_bytes = elf_va(&elf, 0x50b8314, 256)?;
    let mut table = [0u32; 64];
    for (index, chunk) in table_bytes.as_chunks::<4>().0.iter().enumerate() {
        table[index] = u32::from_le_bytes(*chunk);
    }
    Ok(KeyMaterial { state, table })
}

pub fn derive_game_key(path: &Path) -> Result<GameKey> {
    let material = material_from_elf(path)?;
    Ok(generate_game_key(&material))
}

/// Reproduce the GameKey from external v1 derivation material.
pub fn generate_game_key(material: &KeyMaterial) -> GameKey {
    let mut state = material.state;
    let next = |state: &mut [u32; 4]| {
        let t = state[0] ^ state[0].wrapping_shl(11);
        let value = t ^ (t >> 8) ^ state[3] ^ (state[3] >> 19);
        *state = [state[1], state[2], state[3], value];
        value
    };
    let mut key = [0u8; 16];
    for output in key.as_chunks_mut::<4>().0 {
        let mut part = 0u32;
        for _ in 0..4 {
            let index = ((next(&mut state) as u64 * 64) >> 32) as usize;
            let shift = (next(&mut state) >> 27) & 0x18;
            part = (part << 8) | ((material.table[index] >> shift) & 0xff);
        }
        output.copy_from_slice(&part.to_le_bytes());
    }
    state.zeroize();
    GameKey(key)
}

pub fn export_random_table(elf_path: &Path, output: &Path) -> Result<()> {
    let material = material_from_elf(elf_path)?;
    if !supported_material(&material) {
        return Err("pinned ELF key material did not match reference".into());
    }
    let bytes = material.to_bytes();
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(output)
        .map_err(|_| "random table output must be a new file")?;
    if file
        .write_all(&bytes)
        .and_then(|_| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(output);
        return Err("random table export failed".into());
    }
    drop(file);
    if let Err(error) = read_random_table(output) {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    Ok(())
}

fn hmac16(key: &[u8; 16], data: &[u8]) -> [u8; 16] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC accepts 16 bytes");
    mac.update(data);
    let digest = mac.finalize().into_bytes();
    digest[..16].try_into().unwrap()
}

pub fn derive_pia_key(game: &GameKey, parameter: &[u8]) -> Result<PiaKey> {
    if parameter.len() != 32 {
        return Err("SessionParameter must be 32 bytes".into());
    }
    let mut changed = [0u8; 32];
    changed.copy_from_slice(parameter);
    changed[31] = changed[31].wrapping_add(1);
    let key = hmac16(&game.0, &changed);
    changed.fill(0);
    Ok(PiaKey(key))
}

pub fn decrypt_browse_request(game: &GameKey, wire: &[u8], broadcast: Ipv4Addr) -> Result<Vec<u8>> {
    if wire.len() != 325 || wire[0] != 0 {
        return Err("BrowseRequest layout unsupported".into());
    }
    let cipher = Aes128::new_from_slice(&game.0).map_err(|_| "invalid GameKey")?;
    let mut block = Array::from(<[u8; 16]>::try_from(&wire[37..53]).unwrap());
    cipher.encrypt_block(&mut block);
    let search = AesGcm::<Aes128, U12>::new_from_slice(&block).map_err(|_| "SearchKey invalid")?;
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&broadcast.octets());
    nonce[4..].copy_from_slice(&wire[29..37]);
    let mut plaintext = wire[69..325].to_vec();
    search
        .decrypt_inout_detached(
            &Nonce::from(nonce),
            b"",
            plaintext.as_mut_slice().into(),
            &Tag::from(<[u8; 16]>::try_from(&wire[53..69]).unwrap()),
        )
        .map_err(|_| "BrowseRequest authentication failed")?;
    Ok(plaintext)
}

pub fn verify_browse_reply(
    game: &GameKey,
    request: &[u8],
    request_plain: &[u8],
    reply: &[u8],
    broadcast: Ipv4Addr,
) -> Result<bool> {
    if request.len() != 325 || request_plain.len() != 256 || reply.len() != 620 || reply[0] != 1 {
        return Err("BrowseReply proof layout unsupported".into());
    }
    let mut material = [0u8; 32];
    material[..16].copy_from_slice(&reply[572..588]);
    material[16..].copy_from_slice(&request[37..53]);
    let proof_key = hmac16(&game.0, &material);
    material.fill(0);
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&broadcast.octets());
    nonce[4..].copy_from_slice(&reply[564..572]);
    let cipher =
        AesGcm::<Aes128, U12>::new_from_slice(&proof_key).map_err(|_| "ProofKey invalid")?;
    let mut decrypted = reply[604..620].to_vec();
    if cipher
        .decrypt_inout_detached(
            &Nonce::from(nonce),
            b"",
            decrypted.as_mut_slice().into(),
            &Tag::from(<[u8; 16]>::try_from(&reply[588..604]).unwrap()),
        )
        .is_err()
    {
        return Ok(false);
    }
    Ok(decrypted.as_slice() == hmac16(&game.0, request_plain))
}

pub struct AuthenticatedPia {
    pub flags: u8,
    pub packet_id: u16,
    pub destination_id: u16,
    pub source_id: u16,
    pub body: Vec<u8>,
}

pub fn validate_pia_header(wire: &[u8]) -> Result<()> {
    if !(45..=1472).contains(&wire.len())
        || &wire[..5] != b"\x32\xab\x98\x64\x8f"
        || !(wire.len() - 29).is_multiple_of(16)
    {
        return Err("Pia wire header invalid".into());
    }
    if wire[12] != 0 {
        return Err("Pia footer layout unsupported".into());
    }
    Ok(())
}

pub fn decrypt_pia(key: &PiaKey, wire: &[u8], source: Ipv4Addr) -> Result<AuthenticatedPia> {
    validate_pia_header(wire)?;
    let flags = wire[5];
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&source.octets());
    nonce[4..].copy_from_slice(&wire[13..21]);
    let cipher = AesGcm::<Aes128, U12, U8>::new_from_slice(&key.0).map_err(|_| "PiaKey invalid")?;
    let mut padded = wire[29..].to_vec();
    cipher
        .decrypt_inout_detached(
            &Nonce::from(nonce),
            b"",
            padded.as_mut_slice().into(),
            &Tag::<U8>::from(<[u8; 8]>::try_from(&wire[21..29]).unwrap()),
        )
        .map_err(|_| "Pia authentication failed")?;
    let padding = (flags >> 4) as usize;
    if padding > padded.len()
        || (padding > 0 && padded[padded.len() - padding..].iter().any(|v| *v != 0xff))
    {
        return Err("authenticated Pia padding invalid".into());
    }
    padded.truncate(padded.len() - padding);
    let body = if flags & 1 != 0 {
        if padded.is_empty()
            || padded.len() > MAX_COMPRESSED
            || !padded.starts_with(b"\x28\xb5\x2f\xfd")
            || zstd_safe::find_frame_compressed_size(&padded).ok() != Some(padded.len())
        {
            return Err("authenticated Zstd frame invalid".into());
        }
        zstd::bulk::decompress(&padded, MAX_PLAINTEXT)
            .map_err(|_| "authenticated Zstd frame invalid or oversized")?
    } else {
        padded
    };
    if body.is_empty() || body.len() > MAX_PLAINTEXT {
        return Err("authenticated Pia plaintext size invalid".into());
    }
    Ok(AuthenticatedPia {
        flags,
        packet_id: u16::from_be_bytes([wire[10], wire[11]]),
        destination_id: u16::from_be_bytes([wire[6], wire[7]]),
        source_id: u16::from_be_bytes([wire[8], wire[9]]),
        body,
    })
}

#[cfg(test)]
#[path = "../test_support/unit/crypto.rs"]
mod tests;
