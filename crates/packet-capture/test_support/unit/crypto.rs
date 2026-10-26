use super::*;
use aes_gcm::aead::AeadInOut;

#[test]
fn external_material_roundtrip_and_key_source_validation() {
    let material = KeyMaterial {
        state: [1, 2, 3, 4],
        table: [0x0102_0304; 64],
    };
    let bytes = material.to_bytes();
    let parsed = KeyMaterial::from_bytes(&bytes).unwrap();
    assert_eq!(generate_game_key(&material).0, generate_game_key(&parsed).0);
    assert!(KeyMaterial::from_bytes(&bytes[..271]).is_err());
    assert!(!supported_material(&parsed));
    assert!(read_game_key_hex("x").is_err());
    assert_eq!(
        read_game_key_hex("0123456789abcdef0123456789abcdef")
            .unwrap()
            .0[0],
        1
    );
}

fn sealed_pia(body: &[u8], compressed: bool, padding_byte: u8) -> Vec<u8> {
    let mut wire = vec![0u8; 29];
    wire[..5].copy_from_slice(b"\x32\xab\x98\x64\x8f");
    let padding = (16 - body.len() % 16) % 16;
    wire[5] = ((padding as u8) << 4) | u8::from(compressed);
    wire[10..12].copy_from_slice(&7u16.to_be_bytes());
    wire[13..21].copy_from_slice(&[9; 8]);
    let mut plaintext = body.to_vec();
    plaintext.resize(body.len() + padding, padding_byte);
    let mut nonce = [0u8; 12];
    nonce[..4].copy_from_slice(&[192, 0, 2, 1]);
    nonce[4..].copy_from_slice(&[9; 8]);
    let cipher = AesGcm::<Aes128, U12, U8>::new_from_slice(&[7; 16]).unwrap();
    let tag = cipher
        .encrypt_inout_detached(&Nonce::from(nonce), b"", plaintext.as_mut_slice().into())
        .unwrap();
    wire[21..29].copy_from_slice(&tag);
    wire.extend(plaintext);
    wire
}

#[test]
fn nist_zero_gcm_tag_truncated_to_eight_bytes() {
    // NIST SP 800-38D test case 2: zero key, zero 96-bit IV, empty plaintext.
    let key = PiaKey([0; 16]);
    let cipher = AesGcm::<Aes128, U12, U8>::new_from_slice(&key.0).unwrap();
    let mut empty = vec![];
    let tag = cipher
        .encrypt_inout_detached(&Nonce::from([0; 12]), b"", empty.as_mut_slice().into())
        .unwrap();
    assert_eq!(hex::encode(tag), "58e2fccefa7e3061");
    cipher
        .decrypt_inout_detached(
            &Nonce::from([0; 12]),
            b"",
            empty.as_mut_slice().into(),
            &tag,
        )
        .unwrap();
    let mut bad = tag;
    bad[0] ^= 1;
    assert!(
        cipher
            .decrypt_inout_detached(
                &Nonce::from([0; 12]),
                b"",
                empty.as_mut_slice().into(),
                &bad
            )
            .is_err()
    );
}

#[test]
fn authenticated_wire_tamper_and_padding_rejection() {
    let source = Ipv4Addr::new(192, 0, 2, 1);
    let key = PiaKey([7; 16]);
    let wire = sealed_pia(&[4, 0], false, 0xff);
    let plain = decrypt_pia(&key, &wire, source).unwrap();
    assert_eq!(plain.body, [4, 0]);
    assert_eq!(plain.packet_id, 7);
    let mut tampered = wire.clone();
    tampered[21] ^= 1;
    assert_eq!(
        decrypt_pia(&key, &tampered, source).err().unwrap(),
        "Pia authentication failed"
    );
    assert_eq!(
        decrypt_pia(&key, &wire, Ipv4Addr::new(192, 0, 2, 2))
            .err()
            .unwrap(),
        "Pia authentication failed"
    );
    let malformed_padding = sealed_pia(&[4, 0], false, 0);
    assert_eq!(
        decrypt_pia(&key, &malformed_padding, source).err().unwrap(),
        "authenticated Pia padding invalid"
    );
}

#[test]
fn one_bounded_zstd_frame() {
    let source = Ipv4Addr::new(192, 0, 2, 1);
    let key = PiaKey([7; 16]);
    let original = [6, 0, 1, 1, 0x32];
    let frame = zstd::bulk::compress(&original, 3).unwrap();
    let wire = sealed_pia(&frame, true, 0xff);
    assert_eq!(decrypt_pia(&key, &wire, source).unwrap().body, original);
    let mut trailing = frame.clone();
    trailing.push(0);
    assert!(decrypt_pia(&key, &sealed_pia(&trailing, true, 0xff), source).is_err());
    let bomb = zstd::bulk::compress(vec![0; MAX_PLAINTEXT + 1].as_slice(), 3).unwrap();
    assert!(decrypt_pia(&key, &sealed_pia(&bomb, true, 0xff), source).is_err());
}
