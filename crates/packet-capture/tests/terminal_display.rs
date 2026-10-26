use std::fs;
use std::process::Command;

fn one_packet_pcap() -> Vec<u8> {
    let mut frame = vec![0u8; 14 + 20 + 8 + 3];
    frame[..6].copy_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66]);
    frame[6..12].copy_from_slice(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff]);
    frame[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    frame[14] = 0x45;
    frame[16..18].copy_from_slice(&31u16.to_be_bytes());
    frame[23] = 17;
    frame[26..30].copy_from_slice(&[192, 0, 2, 1]);
    frame[30..34].copy_from_slice(&[192, 0, 2, 2]);
    frame[34..36].copy_from_slice(&49152u16.to_be_bytes());
    frame[36..38].copy_from_slice(&49152u16.to_be_bytes());
    frame[38..40].copy_from_slice(&11u16.to_be_bytes());
    frame[42..].copy_from_slice(&[1, 2, 3]);

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

#[test]
fn terminal_uses_iso_utc_and_omits_endpoints_while_jsonl_retains_them() {
    let dir = tempfile::tempdir().unwrap();
    let pcap = dir.path().join("one.pcap");
    fs::write(&pcap, one_packet_pcap()).unwrap();
    let run = |jsonl: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_s3-packet-capture"));
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
