//! Bounded classic-PCAP reader for complete Ethernet/IPv4/UDP frames.
use crate::Result;
use std::io::{self, Read};
use std::net::Ipv4Addr;

const MAX_FRAME: usize = 65_535;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UdpPacket {
    pub record: u64,
    pub timestamp_ns: u64,
    pub source: Ipv4Addr,
    pub destination: Ipv4Addr,
    pub source_port: u16,
    pub destination_port: u16,
    pub source_mac: [u8; 6],
    pub destination_mac: [u8; 6],
    pub payload: Vec<u8>,
}

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn u32(self, bytes: &[u8]) -> u32 {
        let a: [u8; 4] = bytes.try_into().expect("four bytes");
        match self {
            Self::Little => u32::from_le_bytes(a),
            Self::Big => u32::from_be_bytes(a),
        }
    }
}

pub struct Reader<R: Read> {
    input: R,
    endian: Endian,
    nanoseconds: bool,
    record: u64,
}

fn exact_or_eof<R: Read>(input: &mut R, buffer: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buffer.len() {
        match input.read(&mut buffer[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated PCAP record",
                ));
            },
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

impl<R: Read> Reader<R> {
    pub fn new(mut input: R) -> Result<Self> {
        let mut header = [0; 24];
        if !exact_or_eof(&mut input, &mut header).map_err(|e| e.to_string())? {
            return Err("empty PCAP".into());
        }
        let (endian, nanoseconds) = match &header[..4] {
            [0xd4, 0xc3, 0xb2, 0xa1] => (Endian::Little, false),
            [0xa1, 0xb2, 0xc3, 0xd4] => (Endian::Big, false),
            [0x4d, 0x3c, 0xb2, 0xa1] => (Endian::Little, true),
            [0xa1, 0xb2, 0x3c, 0x4d] => (Endian::Big, true),
            [0x0a, 0x0d, 0x0d, 0x0a] => {
                return Err("pcapng is unsupported; use classic PCAP".into());
            },
            _ => return Err("invalid classic PCAP magic".into()),
        };
        let version = match endian {
            Endian::Little => u16::from_le_bytes([header[4], header[5]]),
            Endian::Big => u16::from_be_bytes([header[4], header[5]]),
        };
        let minor = match endian {
            Endian::Little => u16::from_le_bytes([header[6], header[7]]),
            Endian::Big => u16::from_be_bytes([header[6], header[7]]),
        };
        if (version, minor) != (2, 4) {
            return Err("unsupported PCAP version".into());
        }
        let link_type = endian.u32(&header[20..24]);
        if link_type != 1 {
            return Err(format!(
                "unsupported PCAP link type {link_type}; Ethernet (1) required"
            ));
        }
        let snaplen = endian.u32(&header[16..20]);
        // tcpdump on macOS commonly advertises 524288 even for small Ethernet frames.
        // The actual per-record allocation remains capped independently below.
        if snaplen == 0 || snaplen > 16 * 1024 * 1024 {
            return Err("PCAP snaplen outside supported bounds".into());
        }
        Ok(Self {
            input,
            endian,
            nanoseconds,
            record: 0,
        })
    }

    pub fn next_record(&mut self) -> Result<Option<std::result::Result<UdpPacket, String>>> {
        loop {
            let mut header = [0; 16];
            if !exact_or_eof(&mut self.input, &mut header).map_err(|e| e.to_string())? {
                return Ok(None);
            }
            self.record += 1;
            let seconds = self.endian.u32(&header[..4]) as u64;
            let fraction = self.endian.u32(&header[4..8]) as u64;
            let captured = self.endian.u32(&header[8..12]) as usize;
            let original = self.endian.u32(&header[12..16]) as usize;
            if captured > MAX_FRAME || captured > original {
                return Err(format!("record {} length invalid", self.record));
            }
            let scale = if self.nanoseconds { 1 } else { 1000 };
            if fraction
                >= if self.nanoseconds {
                    1_000_000_000
                } else {
                    1_000_000
                }
            {
                return Err(format!("record {} timestamp invalid", self.record));
            }
            let mut frame = vec![0; captured];
            if !exact_or_eof(&mut self.input, &mut frame)
                .map_err(|e| format!("record {}: {e}", self.record))?
            {
                return Err(format!(
                    "record {}: truncated PCAP record body",
                    self.record
                ));
            }
            if captured != original {
                return Ok(Some(Err(format!(
                    "record {}: truncated frame (caplen < wirelen)",
                    self.record
                ))));
            }
            let timestamp_ns = seconds * 1_000_000_000 + fraction * scale;
            match parse_udp(&frame, self.record, timestamp_ns) {
                Ok(Some(packet)) => return Ok(Some(Ok(packet))),
                Ok(None) => continue,
                Err(error) => return Ok(Some(Err(format!("record {}: {error}", self.record)))),
            }
        }
    }
}

fn be16(bytes: &[u8]) -> u16 {
    u16::from_be_bytes([bytes[0], bytes[1]])
}

fn parse_udp(frame: &[u8], record: u64, timestamp_ns: u64) -> Result<Option<UdpPacket>> {
    if frame.len() < 14 {
        return Err("Ethernet frame truncated".into());
    }
    let destination_mac = frame[..6].try_into().unwrap();
    let source_mac = frame[6..12].try_into().unwrap();
    let mut offset = 14;
    let mut ethertype = be16(&frame[12..14]);
    for _ in 0..2 {
        if ethertype != 0x8100 && ethertype != 0x88a8 {
            break;
        }
        if frame.len() < offset + 4 {
            return Err("VLAN header truncated".into());
        }
        ethertype = be16(&frame[offset + 2..offset + 4]);
        offset += 4;
    }
    if ethertype != 0x0800 {
        return Ok(None);
    }
    if frame.len() < offset + 20 {
        return Err("IPv4 header truncated".into());
    }
    let ip = &frame[offset..];
    let ihl = ((ip[0] & 15) as usize) * 4;
    if ip[0] >> 4 != 4 || ihl < 20 || ip.len() < ihl {
        return Err("invalid IPv4 header".into());
    }
    let total = be16(&ip[2..4]) as usize;
    if total < ihl || total > ip.len() {
        return Err("IPv4 length invalid or frame truncated".into());
    }
    if ip[9] != 17 {
        return Ok(None);
    }
    if be16(&ip[6..8]) & 0x3fff != 0 {
        return Err("fragmented IPv4 UDP datagram unsupported".into());
    }
    if total < ihl + 8 {
        return Err("UDP header truncated".into());
    }
    let udp = &ip[ihl..total];
    let udp_len = be16(&udp[4..6]) as usize;
    if udp_len < 8 || udp_len != udp.len() {
        return Err("UDP length invalid".into());
    }
    let source_port = be16(&udp[..2]);
    let destination_port = be16(&udp[2..4]);
    if source_port != 35000
        && destination_port != 35000
        && source_port != 49152
        && destination_port != 49152
    {
        return Ok(None);
    }
    Ok(Some(UdpPacket {
        record,
        timestamp_ns,
        source: Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]),
        destination: Ipv4Addr::new(ip[16], ip[17], ip[18], ip[19]),
        source_port,
        destination_port,
        source_mac,
        destination_mac,
        payload: udp[8..].to_vec(),
    }))
}

#[cfg(test)]
mod tests {
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
}
