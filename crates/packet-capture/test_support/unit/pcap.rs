use super::*;
use std::io::Cursor;

struct OneByte(Cursor<Vec<u8>>);
impl Read for OneByte {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let count = buf.len().min(1);
        self.0.read(&mut buf[..count])
    }
}

fn capture(frame: &[u8], nanoseconds: bool, big: bool) -> Vec<u8> {
    let mut out = vec![];
    out.extend(if big {
        if nanoseconds {
            [0xa1, 0xb2, 0x3c, 0x4d]
        } else {
            [0xa1, 0xb2, 0xc3, 0xd4]
        }
    } else if nanoseconds {
        [0x4d, 0x3c, 0xb2, 0xa1]
    } else {
        [0xd4, 0xc3, 0xb2, 0xa1]
    });
    let word = |n: u32| {
        if big {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    };
    out.extend(if big { [0, 2, 0, 4] } else { [2, 0, 4, 0] });
    out.extend([0; 8]);
    out.extend(word(65535));
    out.extend(word(1));
    for n in [1, 25, frame.len() as u32, frame.len() as u32] {
        out.extend(word(n));
    }
    out.extend(frame);
    out
}

fn frame() -> Vec<u8> {
    let mut f = vec![0; 14 + 20 + 8 + 3];
    f[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    f[14] = 0x45;
    f[16..18].copy_from_slice(&31u16.to_be_bytes());
    f[23] = 17;
    f[26..30].copy_from_slice(&[192, 168, 1, 1]);
    f[30..34].copy_from_slice(&[192, 168, 1, 2]);
    f[34..36].copy_from_slice(&49152u16.to_be_bytes());
    f[36..38].copy_from_slice(&49152u16.to_be_bytes());
    f[38..40].copy_from_slice(&11u16.to_be_bytes());
    f[42..].copy_from_slice(&[1, 2, 3]);
    f
}

#[test]
fn endian_and_timestamp_variants() {
    for big in [false, true] {
        for nano in [false, true] {
            let mut reader = Reader::new(Cursor::new(capture(&frame(), nano, big))).unwrap();
            let packet = reader.next_record().unwrap().unwrap().unwrap();
            assert_eq!(packet.payload, [1, 2, 3]);
            assert_eq!(
                packet.timestamp_ns,
                1_000_000_000 + if nano { 25 } else { 25_000 }
            );
            assert!(reader.next_record().unwrap().is_none());
        }
    }
}

#[test]
fn truncated_record_and_fragment_are_distinct() {
    let mut bytes = capture(&frame(), false, false);
    bytes.pop();
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert!(reader.next_record().unwrap_err().contains("truncated PCAP"));
    let mut no_body = capture(&frame(), false, false);
    no_body.truncate(24 + 16);
    let mut reader = Reader::new(Cursor::new(no_body)).unwrap();
    assert!(
        reader
            .next_record()
            .unwrap_err()
            .contains("truncated PCAP record body")
    );
    let mut f = frame();
    f[20] = 0x20;
    let mut reader = Reader::new(Cursor::new(capture(&f, false, false))).unwrap();
    assert!(
        reader
            .next_record()
            .unwrap()
            .unwrap()
            .unwrap_err()
            .contains("fragmented")
    );
}

#[test]
fn accepts_large_advertised_snaplen_but_caps_actual_records() {
    let mut bytes = capture(&frame(), false, false);
    bytes[16..20].copy_from_slice(&524288u32.to_le_bytes());
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert!(reader.next_record().unwrap().unwrap().is_ok());
}

#[test]
fn split_input_vlan_and_unsupported_formats() {
    let mut vlan = frame();
    vlan[12..14].copy_from_slice(&0x8100u16.to_be_bytes());
    vlan.splice(14..14, [0, 1, 0x08, 0x00]);
    let mut reader = Reader::new(OneByte(Cursor::new(capture(&vlan, false, false)))).unwrap();
    assert_eq!(
        reader.next_record().unwrap().unwrap().unwrap().payload,
        [1, 2, 3]
    );
    let mut wrong_link = capture(&frame(), false, false);
    wrong_link[20..24].copy_from_slice(&101u32.to_le_bytes());
    assert!(
        Reader::new(Cursor::new(wrong_link))
            .err()
            .unwrap()
            .contains("link type")
    );
    assert!(
        Reader::new(Cursor::new(vec![
            0x0a, 0x0d, 0x0d, 0x0a, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
        ]))
        .err()
        .unwrap()
        .contains("pcapng")
    );
}

#[test]
fn captured_shorter_than_wire_is_reported() {
    let mut bytes = capture(&frame(), false, false);
    let actual = frame().len() as u32;
    bytes[36..40].copy_from_slice(&(actual + 1).to_le_bytes());
    let mut reader = Reader::new(Cursor::new(bytes)).unwrap();
    assert!(
        reader
            .next_record()
            .unwrap()
            .unwrap()
            .unwrap_err()
            .contains("caplen < wirelen")
    );
}
