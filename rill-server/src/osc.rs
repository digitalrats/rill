use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeTag {
    pub seconds: u64,
    pub fractional: u64,
}

impl TimeTag {
    pub fn immediate() -> Self {
        TimeTag {
            seconds: 0,
            fractional: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum OscType {
    Int(i32),
    Float(f32),
    String(String),
    Blob(Vec<u8>),
    Timetag(TimeTag),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OscMessage {
    pub addr: String,
    pub args: Vec<OscType>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OscBundle {
    pub timetag: TimeTag,
    pub packets: Vec<OscPacket>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OscPacket {
    Message(OscMessage),
    Bundle(OscBundle),
}

pub fn decode(buf: &[u8]) -> Result<OscPacket, Error> {
    if buf.starts_with(b"/") {
        decode_message(buf).map(OscPacket::Message)
    } else if buf.starts_with(b"#bundle\0") {
        decode_bundle(buf).map(OscPacket::Bundle)
    } else {
        Err(Error::InvalidPacket)
    }
}

fn decode_message(buf: &[u8]) -> Result<OscMessage, Error> {
    let (addr, rest) = read_string(buf)?;

    if rest.is_empty() {
        return Ok(OscMessage {
            addr,
            args: Vec::new(),
        });
    }

    let (type_tags, rest) = read_string(rest)?;

    if !type_tags.starts_with(',') {
        return Err(Error::Parse("missing type tag prefix ','".into()));
    }

    let type_chars: Vec<char> = type_tags[1..].chars().collect();
    let mut args = Vec::with_capacity(type_chars.len());
    let mut ptr = rest;

    for ch in type_chars {
        let (arg, remaining) = match ch {
            'i' => {
                let (v, r) = read_i32(ptr)?;
                (OscType::Int(v), r)
            }
            'f' => {
                let (v, r) = read_f32(ptr)?;
                (OscType::Float(v), r)
            }
            's' => {
                let (v, r) = read_string(ptr)?;
                (OscType::String(v), r)
            }
            'b' => {
                let (v, r) = read_blob(ptr)?;
                (OscType::Blob(v), r)
            }
            't' => {
                let (v, r) = read_timetag(ptr)?;
                (OscType::Timetag(v), r)
            }
            'T' => (OscType::Int(1), ptr),
            'F' => (OscType::Int(0), ptr),
            _ => return Err(Error::Parse(format!("unknown type tag: {}", ch))),
        };
        args.push(arg);
        ptr = remaining;
    }

    Ok(OscMessage { addr, args })
}

fn decode_bundle(buf: &[u8]) -> Result<OscBundle, Error> {
    let buf = &buf[8..];

    let (timetag, mut rest) = read_timetag(buf)?;

    let mut packets = Vec::new();

    while !rest.is_empty() {
        let (size, rest_after_size) = read_i32(rest)?;
        let size = size as usize;

        if size > rest_after_size.len() {
            return Err(Error::Parse(
                "bundle element size exceeds remaining data length".into(),
            ));
        }

        let packet_data = &rest_after_size[..size];
        let packet = decode(packet_data)?;
        packets.push(packet);

        rest = &rest_after_size[size..];
    }

    Ok(OscBundle { timetag, packets })
}

pub fn encode(packet: &OscPacket) -> Result<Vec<u8>, Error> {
    match packet {
        OscPacket::Message(msg) => encode_message(msg),
        OscPacket::Bundle(bundle) => encode_bundle(bundle),
    }
}

fn encode_message(msg: &OscMessage) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();

    write_string(&mut buf, &msg.addr);

    let mut type_str = String::from(",");
    for arg in &msg.args {
        match arg {
            OscType::Int(_) => type_str.push('i'),
            OscType::Float(_) => type_str.push('f'),
            OscType::String(_) => type_str.push('s'),
            OscType::Blob(_) => type_str.push('b'),
            OscType::Timetag(_) => type_str.push('t'),
        }
    }

    write_string(&mut buf, &type_str);

    for arg in &msg.args {
        match arg {
            OscType::Int(v) => buf.extend_from_slice(&v.to_be_bytes()),
            OscType::Float(v) => buf.extend_from_slice(&v.to_be_bytes()),
            OscType::String(v) => write_string(&mut buf, v),
            OscType::Blob(v) => write_blob(&mut buf, v),
            OscType::Timetag(v) => write_timetag(&mut buf, *v),
        }
    }

    Ok(buf)
}

fn encode_bundle(bundle: &OscBundle) -> Result<Vec<u8>, Error> {
    let mut buf = Vec::new();

    buf.extend_from_slice(b"#bundle\0");
    write_timetag(&mut buf, bundle.timetag);

    for packet in &bundle.packets {
        let packet_bytes = encode(packet)?;
        let size = packet_bytes.len() as i32;
        buf.extend_from_slice(&size.to_be_bytes());
        buf.extend_from_slice(&packet_bytes);
    }

    Ok(buf)
}

fn read_i32(buf: &[u8]) -> Result<(i32, &[u8]), Error> {
    if buf.len() < 4 {
        return Err(Error::Parse("buffer too short for int32".into()));
    }
    let bytes: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    Ok((i32::from_be_bytes(bytes), &buf[4..]))
}

fn read_f32(buf: &[u8]) -> Result<(f32, &[u8]), Error> {
    if buf.len() < 4 {
        return Err(Error::Parse("buffer too short for float32".into()));
    }
    let bytes: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    Ok((f32::from_be_bytes(bytes), &buf[4..]))
}

fn read_string(buf: &[u8]) -> Result<(String, &[u8]), Error> {
    let null_pos = buf
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| Error::Parse("string without null terminator".into()))?;

    let s = std::str::from_utf8(&buf[..null_pos])
        .map_err(|e| Error::Parse(format!("invalid UTF-8: {}", e)))?
        .to_string();

    let padded_len = ((null_pos + 4) / 4) * 4;

    if padded_len > buf.len() {
        return Err(Error::Parse("buffer too short for padded string".into()));
    }

    Ok((s, &buf[padded_len..]))
}

fn read_blob(buf: &[u8]) -> Result<(Vec<u8>, &[u8]), Error> {
    let (size, rest) = read_i32(buf)?;
    let size = size as usize;

    if size > rest.len() {
        return Err(Error::Parse("blob size exceeds data length".into()));
    }

    let data = rest[..size].to_vec();

    let padded_len = size.div_ceil(4) * 4;

    if padded_len > rest.len() {
        return Err(Error::Parse("buffer too short for padded blob".into()));
    }

    Ok((data, &rest[padded_len..]))
}

fn read_timetag(buf: &[u8]) -> Result<(TimeTag, &[u8]), Error> {
    if buf.len() < 8 {
        return Err(Error::Parse("buffer too short for timetag".into()));
    }

    let sec_bytes: [u8; 4] = [buf[0], buf[1], buf[2], buf[3]];
    let frac_bytes: [u8; 4] = [buf[4], buf[5], buf[6], buf[7]];

    let seconds = u32::from_be_bytes(sec_bytes) as u64;
    let fractional = u32::from_be_bytes(frac_bytes) as u64;

    Ok((
        TimeTag {
            seconds,
            fractional,
        },
        &buf[8..],
    ))
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(bytes);
    buf.push(0);

    let padded_len = (bytes.len() + 1).div_ceil(4) * 4;
    let pad_len = padded_len - (bytes.len() + 1);
    buf.extend(std::iter::repeat_n(0u8, pad_len));
}

fn write_blob(buf: &mut Vec<u8>, data: &[u8]) {
    let size = data.len() as i32;
    buf.extend_from_slice(&size.to_be_bytes());
    buf.extend_from_slice(data);

    let padded_len = data.len().div_ceil(4) * 4;
    let pad_len = padded_len - data.len();
    buf.extend(std::iter::repeat_n(0u8, pad_len));
}

fn write_timetag(buf: &mut Vec<u8>, tag: TimeTag) {
    buf.extend_from_slice(&(tag.seconds as u32).to_be_bytes());
    buf.extend_from_slice(&(tag.fractional as u32).to_be_bytes());
}

pub fn pattern_match(pattern: &str, addr: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let pat_parts: Vec<&str> = pattern.split('/').collect();
    let addr_parts: Vec<&str> = addr.split('/').collect();

    if pat_parts.len() != addr_parts.len() {
        return false;
    }

    for (p, a) in pat_parts.iter().zip(addr_parts.iter()) {
        if *p == "*" {
            continue;
        }
        if p != a {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_int_float() {
        let msg = OscMessage {
            addr: "/test/foo".into(),
            args: vec![OscType::Int(42), OscType::Float(3.14)],
        };
        let packet = OscPacket::Message(msg.clone());
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_message_string() {
        let msg = OscMessage {
            addr: "/hello".into(),
            args: vec![OscType::String("world".into())],
        };
        let packet = OscPacket::Message(msg);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_message_blob() {
        let msg = OscMessage {
            addr: "/data".into(),
            args: vec![OscType::Blob(vec![0x00, 0x01, 0x02, 0x03, 0x04])],
        };
        let packet = OscPacket::Message(msg);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_message_timetag() {
        let msg = OscMessage {
            addr: "/sync".into(),
            args: vec![OscType::Timetag(TimeTag {
                seconds: 100,
                fractional: 500,
            })],
        };
        let packet = OscPacket::Message(msg);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_message_mixed_args() {
        let msg = OscMessage {
            addr: "/mix".into(),
            args: vec![
                OscType::Int(-1),
                OscType::Float(2.5),
                OscType::String("test".into()),
                OscType::Blob(vec![0xFF]),
            ],
        };
        let packet = OscPacket::Message(msg);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_empty_message() {
        let msg = OscMessage {
            addr: "/test".into(),
            args: vec![],
        };
        let bytes = encode(&OscPacket::Message(msg.clone())).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(OscPacket::Message(msg), decoded);
    }

    #[test]
    fn test_bundle() {
        let msg1 = OscMessage {
            addr: "/a".into(),
            args: vec![OscType::Int(1)],
        };
        let msg2 = OscMessage {
            addr: "/b".into(),
            args: vec![OscType::Float(2.0)],
        };
        let bundle = OscBundle {
            timetag: TimeTag::immediate(),
            packets: vec![OscPacket::Message(msg1), OscPacket::Message(msg2)],
        };
        let packet = OscPacket::Bundle(bundle);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_nested_bundle() {
        let inner = OscBundle {
            timetag: TimeTag::immediate(),
            packets: vec![OscPacket::Message(OscMessage {
                addr: "/inner".into(),
                args: vec![OscType::Int(99)],
            })],
        };
        let outer = OscBundle {
            timetag: TimeTag {
                seconds: 1,
                fractional: 0,
            },
            packets: vec![OscPacket::Bundle(inner)],
        };
        let packet = OscPacket::Bundle(outer);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_decode_invalid() {
        assert!(decode(b"garbage").is_err());
        assert!(decode(b"").is_err());
    }

    #[test]
    fn test_pattern_match_exact() {
        assert!(pattern_match("/audio/volume", "/audio/volume"));
        assert!(!pattern_match("/audio/volume", "/audio/pan"));
    }

    #[test]
    fn test_pattern_match_wildcard() {
        assert!(pattern_match("/audio/*", "/audio/volume"));
        assert!(pattern_match("/audio/*", "/audio/pan"));
        assert!(!pattern_match("/audio/*", "/mixer/volume"));
        assert!(pattern_match("*", "/anything/works"));
    }

    #[test]
    fn test_pattern_match_multi_segment() {
        assert!(pattern_match("/*/volume", "/audio/volume"));
        assert!(!pattern_match("/*/volume", "/audio/pan"));
    }

    #[test]
    fn test_time_tag_immediate() {
        let t = TimeTag::immediate();
        assert_eq!(t.seconds, 0);
        assert_eq!(t.fractional, 0);
    }

    #[test]
    fn test_encode_empty_string() {
        let msg = OscMessage {
            addr: "/".into(),
            args: vec![OscType::String(String::new())],
        };
        let packet = OscPacket::Message(msg);
        let bytes = encode(&packet).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(packet, decoded);
    }

    #[test]
    fn test_known_osc_packet() {
        let bytes = b"/test\0\0\0,i\0\0\0\0\0\x2a";
        let decoded = decode(bytes).unwrap();
        match decoded {
            OscPacket::Message(msg) => {
                assert_eq!(msg.addr, "/test");
                assert_eq!(msg.args.len(), 1);
                assert_eq!(msg.args[0], OscType::Int(42));
            }
            _ => panic!("expected message"),
        }
    }

    #[test]
    fn test_empty_addr_no_args() {
        let bytes = b"/\0\0\0";
        let decoded = decode(bytes).unwrap();
        match decoded {
            OscPacket::Message(msg) => {
                assert_eq!(msg.addr, "/");
                assert!(msg.args.is_empty());
            }
            _ => panic!("expected message"),
        }
    }
}
