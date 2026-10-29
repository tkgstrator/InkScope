use std::fs;
use std::process::Command;

fn one_packet_pcap() -> Vec<u8> {
    packet_pcap(&[1, 2, 3])
}

fn packet_pcap(payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![0u8; 14 + 20 + 8 + payload.len()];
    frame[..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    frame[6..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&((28 + payload.len()) as u16).to_be_bytes());
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[192, 0, 2, 1]);
    frame[30..34].copy_from_slice(&[192, 0, 2, 2]);
    frame[34..36].copy_from_slice(&49152u16.to_be_bytes());
    frame[36..38].copy_from_slice(&49152u16.to_be_bytes());
    frame[38..40].copy_from_slice(&((8 + payload.len()) as u16).to_be_bytes());
    frame[42..].copy_from_slice(payload);

    let mut pcap = vec![];
    pcap.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    pcap.extend_from_slice(&[2, 0, 4, 0]);
    pcap.extend_from_slice(&[0; 8]);
    pcap.extend_from_slice(&65535u32.to_le_bytes());
    pcap.extend_from_slice(&1u32.to_le_bytes());
    for word in [
        1_792_937_154u32,
        105_000,
        frame.len() as u32,
        frame.len() as u32,
    ] {
        pcap.extend_from_slice(&word.to_le_bytes());
    }
    pcap.extend_from_slice(&frame);
    pcap
}

fn authenticated_equipment_setting() -> Vec<u8> {
    use aes::Aes128;
    use aes_gcm::{
        AesGcm, Nonce,
        aead::{
            AeadInOut, KeyInit,
            consts::{U8, U12},
        },
    };
    let mut payload = vec![0; 211];
    payload[0] = 7;
    payload[1..3].copy_from_slice(&203u16.to_be_bytes());
    payload[13..15].copy_from_slice(&194u16.to_be_bytes());
    payload[17..21].copy_from_slice(&1u32.to_le_bytes());
    payload[21..25].copy_from_slice(&0x9a1e036du32.to_le_bytes());
    payload[25..38].copy_from_slice(b"Cop_Shakedent");
    let mut set_bits = |start: usize, width: usize, value: u64| {
        for offset in 0..width {
            payload[(start + offset) / 8] |=
                (((value >> offset) & 1) as u8) << ((start + offset) % 8);
        }
    };
    for (start, width, value) in [
        (712, 3, 1),
        (715, 10, 5),
        (725, 3, 4),
        (1064, 11, 151),
        (1559, 32, 0xb0300),
        (1591, 1, 1),
        (1592, 3, 4),
        (1659, 23, 77),
    ] {
        set_bits(start, width, value);
    }
    for index in 0..4 {
        set_bits(728 + index * 16, 16, 22033);
        set_bits(1075 + index * 32, 32, index as u64 + 1);
        set_bits(1595 + index * 16, 16, 22033);
    }
    let mut body = vec![0x0e];
    body.extend_from_slice(&211u16.to_be_bytes());
    body.extend_from_slice(&[8, 100]);
    body.extend_from_slice(&payload);
    let padding = (16 - body.len() % 16) % 16;
    body.resize(body.len() + padding, 0xff);
    let mut wire = vec![0; 29];
    wire[..5].copy_from_slice(b"\x32\xab\x98\x64\x8f");
    wire[5] = (padding as u8) << 4;
    wire[13..21].copy_from_slice(&[9; 8]);
    let mut nonce = [0; 12];
    nonce[..4].copy_from_slice(&[192, 0, 2, 1]);
    nonce[4..].copy_from_slice(&[9; 8]);
    let tag = AesGcm::<Aes128, U12, U8>::new_from_slice(&[7; 16])
        .unwrap()
        .encrypt_inout_detached(&Nonce::from(nonce), b"", body.as_mut_slice().into())
        .unwrap();
    wire[21..29].copy_from_slice(&tag);
    wire.extend(body);
    packet_pcap(&wire)
}

#[test]
fn equipment_metadata_stays_in_details_and_jsonl_without_changing_normal_fields() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("equipment.pcap");
    fs::write(&pcap, authenticated_equipment_setting()).unwrap();
    let key = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    fs::write(key.path(), [7; 16]).unwrap();
    let run = |extra: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_inkscope"))
            .arg("inspect")
            .arg(&pcap)
            .args(["--game-key", "00000000000000000000000000000000"])
            .arg("--pia-key-file")
            .arg(key.path())
            .args(["--peer", "192.0.2.1"])
            .args(extra)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let normal = run(&[]);
    let details = run(&["--details"]);
    let json: serde_json::Value = serde_json::from_str(&run(&["--jsonl"])).unwrap();
    for name in [
        "UnlockedSeason",
        "WeaponIdCount",
        "WeaponId[0]",
        "WeaponId[1]",
        "WeaponId[2]",
        "WeaponId[3]",
        "ScenarioAppVersion",
        "ScenarioCompetitionType",
        "ScenarioOriginalWeaponIdCount",
        "ScenarioOriginalWeaponId[0]",
        "ScenarioOriginalWeaponId[1]",
        "ScenarioOriginalWeaponId[2]",
        "ScenarioOriginalWeaponId[3]",
    ] {
        assert!(
            !normal.contains(&format!("    {name}=")),
            "normal output includes {name}"
        );
        assert!(
            details.contains(&format!("    {name} bits ")),
            "details missing {name}"
        );
        assert!(
            json["fields"]
                .as_array()
                .unwrap()
                .iter()
                .any(|field| field["name"] == name),
            "JSONL missing {name}"
        );
    }
    for field in [
        "GameFrame=77",
        "StageResource=Cop_Shakedent",
        "StageName=Gone Fission Hydroplant",
        "JobType=1",
        "Difficulty=150",
        "GameSeed[0]=0x00000001",
        "EventResourceGroupId=1",
    ] {
        assert!(normal.contains(field), "normal output lost {field}");
    }
    for field in [
        "HeaderPacketId=",
        "HeaderCompressedFlag=",
        "PayloadBytes=",
        "TypeId=",
        "DifficultyWire=",
        "LifeNumber=0",
    ] {
        assert!(!normal.contains(field), "normal output gained {field}");
    }
    assert!(normal.contains("WAVE preview: conditional from OnlineCoopSetting.GameSeed"));
}

#[test]
fn terminal_uses_iso_utc_and_omits_endpoints_while_jsonl_retains_them() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();
    let run = |jsonl: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_inkscope"));
        command
            .arg("inspect")
            .arg(&pcap)
            .args(["--game-key", "00000000000000000000000000000000"])
            .args(["--target-mac", "11:22:33:44:55:66"]);
        if jsonl {
            command.arg("--jsonl");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };

    let terminal = run(false);
    assert!(
        terminal.contains("2026-10-25T14:05:54.105Z <- Pia"),
        "{terminal}"
    );
    assert!(!terminal.contains("192.0.2."), "{terminal}");

    let json: serde_json::Value = serde_json::from_str(&run(true)).unwrap();
    assert_eq!(json["timestamp_ns"], 1_792_937_154_105_000_000u64);
    assert_eq!(json["source"], "192.0.2.1:49152");
    assert_eq!(json["destination"], "192.0.2.2:49152");
}
