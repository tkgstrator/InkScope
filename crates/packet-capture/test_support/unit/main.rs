use super::*;
use std::io::Cursor;

#[test]
fn normal_stream_keeps_recognized_private_setting() {
    assert!(visible_in_stream(
        DisplayMode::Normal,
        false,
        Some(11),
        "PrivateSetting",
        Some("confirmed_fields"),
        false
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        false,
        Some(11),
        "BroadcastReliableProtocol",
        Some("type_only"),
        false
    ));
    assert!(visible_in_stream(
        DisplayMode::Debug,
        false,
        Some(11),
        "BroadcastReliableProtocol",
        Some("type_only"),
        false
    ));
    assert!(!visible_in_stream(
        DisplayMode::Normal,
        true,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        false
    ));
    assert!(visible_in_stream(
        DisplayMode::Normal,
        true,
        Some(3),
        "RTTProtocol",
        Some("confirmed_fields"),
        true
    ));
}

#[test]
fn arrows_are_relative_to_the_explicit_target() {
    let target_mac = parse_mac("02:00:00:00:00:01").unwrap();
    assert_eq!(target_mac, [0x02, 0, 0, 0, 0, 1]);
    assert!(parse_mac("02:00:00:00:00").is_err());
    assert!(parse_mac("02:00:00:00:00:gg").is_err());
    let mut packet = UdpPacket {
        record: 1,
        timestamp_ns: 0,
        source: Ipv4Addr::new(192, 0, 2, 1),
        destination: Ipv4Addr::new(192, 0, 2, 2),
        source_port: 49152,
        destination_port: 49152,
        source_mac: target_mac,
        destination_mac: [1; 6],
        payload: vec![],
    };
    assert_eq!(direction(&packet, &Target::Mac(target_mac)), "->");
    assert_eq!(direction(&packet, &Target::Ip(packet.destination)), "<-");
    packet.source_mac = [1; 6];
    packet.destination_mac = target_mac;
    assert_eq!(direction(&packet, &Target::Mac(target_mac)), "<-");
    packet.destination_mac = [2; 6];
    assert_eq!(direction(&packet, &Target::Mac(target_mac)), "--");
}

struct OneByte(Cursor<Vec<u8>>);
impl Read for OneByte {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = buf.len().min(1);
        self.0.read(&mut buf[..count])
    }
}

#[test]
fn tee_saves_exact_stream_despite_partial_reads() {
    let payload = b"pcap stream in small chunks".to_vec();
    let mut tee = Tee {
        source: OneByte(Cursor::new(payload.clone())),
        sink: Vec::new(),
    };
    let mut decoded = vec![];
    tee.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, payload);
    assert_eq!(tee.sink, payload);
}

#[test]
fn capture_output_is_private_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("capture.pcap");
    private_output(&path).unwrap();
    assert!(private_output(&path).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn only_opt_in_capture_can_start_without_a_key() {
    let controlled = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "capture.pcap",
        "--control-port",
        "10253",
    ])
    .unwrap();
    assert!(decoder(&controlled).is_ok());

    let ordinary = Cli::try_parse_from([
        "inkscope",
        "capture",
        "--interface",
        "test0",
        "--output",
        "capture.pcap",
    ])
    .unwrap();
    assert!(decoder(&ordinary).is_err());

    let inspect = Cli::try_parse_from(["inkscope", "inspect", "capture.pcap"]).unwrap();
    assert!(decoder(&inspect).is_err());
    assert!(
        Cli::try_parse_from([
            "inkscope",
            "inspect",
            "capture.pcap",
            "--control-port",
            "10253",
        ])
        .is_err()
    );
}
