//! Primitives du protocole Minecraft Java :
//! VarInt, chaînes, UUID offline, trames et compression zlib.

use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub const PROTOCOL_VERSION: i32 = 775;
pub const MC_VERSION: &str = "26.1.1";
const MAX_FRAME: usize = 8 * 1024 * 1024;

pub fn write_varint(v: i32, out: &mut Vec<u8>) {
    let mut u = v as u32;
    loop {
        if u & !0x7F == 0 {
            out.push(u as u8);
            return;
        }
        out.push(((u & 0x7F) | 0x80) as u8);
        u >>= 7;
    }
}

pub fn varint_len(v: i32) -> usize {
    let mut n = 1;
    let mut u = v as u32;
    while u & !0x7F != 0 {
        u >>= 7;
        n += 1;
    }
    n
}

pub fn write_position(x: i32, y: i32, z: i32, out: &mut Vec<u8>) {
    let v: i64 = ((x as i64 & 0x3FFFFFF) << 38)
        | ((z as i64 & 0x3FFFFFF) << 12)
        | (y as i64 & 0xFFF);
    out.extend_from_slice(&v.to_be_bytes());
}

pub fn write_string(s: &str, out: &mut Vec<u8>) {
    let b = s.as_bytes();
    write_varint(b.len() as i32, out);
    out.extend_from_slice(b);
}

/// UUID offline : identique à UUID.nameUUIDFromBytes(("OfflinePlayer:"+name).getBytes())
pub fn offline_uuid(name: &str) -> [u8; 16] {
    use md5::{Digest, Md5};
    let mut h = Md5::new();
    h.update(format!("OfflinePlayer:{}", name).as_bytes());
    let mut b: [u8; 16] = h.finalize().into();
    b[6] = (b[6] & 0x0f) | 0x30;
    b[8] = (b[8] & 0x3f) | 0x80;
    b
}

pub fn decode_varint(buf: &mut &[u8]) -> i32 {
    let mut num: i32 = 0;
    for i in 0..5 {
        if buf.is_empty() {
            return num;
        }
        let b = buf[0];
        *buf = &buf[1..];
        num |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            break;
        }
    }
    num
}

/// Curseur de lecture synchrone sur le corps d'un paquet.
pub struct Body<'a>(pub &'a [u8]);

impl<'a> Body<'a> {
    pub fn rest(&self) -> &[u8] {
        self.0
    }
    pub fn take(&mut self, n: usize) -> Body<'a> {
        let n = n.min(self.0.len());
        let (a, b) = self.0.split_at(n);
        let out = Body(a);
        self.0 = b;
        out
    }
    pub fn varint(&mut self) -> i32 {
        decode_varint(&mut self.0)
    }
    pub fn u8(&mut self) -> u8 {
        if self.0.is_empty() { return 0; }
        let b = self.0[0];
        self.0 = &self.0[1..];
        b
    }
    pub fn i16(&mut self) -> i16 {
        self.u16() as i16
    }
    pub fn u16(&mut self) -> u16 {
        if self.0.len() < 2 { let v = self.0.first().copied().unwrap_or(0) as u16; self.0 = &self.0[self.0.len().min(1)..]; return v; }
        let mut b = [0u8; 2];
        b.copy_from_slice(&self.0[..2]);
        self.0 = &self.0[2..];
        u16::from_be_bytes(b)
    }
    pub fn bool(&mut self) -> bool {
        self.u8() != 0
    }
    pub fn i32(&mut self) -> i32 {
        if self.0.len() < 4 { self.0 = &self.0[self.0.len()..]; return 0; }
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.0[..4]);
        self.0 = &self.0[4..];
        i32::from_be_bytes(b)
    }
    pub fn i64(&mut self) -> i64 {
        if self.0.len() < 8 { self.0 = &self.0[self.0.len()..]; return 0; }
        let mut b = [0u8; 8];
        b.copy_from_slice(&self.0[..8]);
        self.0 = &self.0[8..];
        i64::from_be_bytes(b)
    }
    pub fn f32(&mut self) -> f32 {
        f32::from_bits(self.i32() as u32)
    }
    pub fn f64(&mut self) -> f64 {
        f64::from_bits(self.i64() as u64)
    }
    /// Position Minecraft compacte (x: 26 bits, z: 26, y: 12) big endian.
    pub fn position(&mut self) -> (i32, i32, i32) {
        let v = self.i64();
        let x = (v >> 38) as i32;
        let z = ((v << 26) >> 38) as i32;
        let y = ((v << 52) >> 52) as i32;
        (x, y, z)
    }
    pub fn string(&mut self) -> String {
        let len = self.varint().max(0) as usize;
        let len = len.min(self.0.len());
        let s = String::from_utf8_lossy(&self.0[..len]).into_owned();
        self.0 = &self.0[len..];
        s
    }
}

/// Essaie d'extraire une trame complète du tampon. Renvoie (trame décompressée, octets consommés).
/// `None` = données insuffisantes pour l'instant.
fn try_parse_frame(buf: &[u8], compressed: bool) -> std::io::Result<Option<(Vec<u8>, usize)>> {
    let mut cur = buf;
    let len = match decode_varint_exact(&mut cur) {
        Some(v) => v as usize,
        None => return Ok(None),
    };
    if len == 0 || len > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("taille de trame invalide: {}", len),
        ));
    }
    if cur.len() < len {
        return Ok(None);
    }
    let (raw, consumed) = cur.split_at(len);
    let consumed = buf.len() - consumed.len();
    let data = if !compressed {
        raw.to_vec()
    } else {
        let mut c: &[u8] = raw;
        let data_len = match decode_varint_exact(&mut c) {
            Some(v) => v as usize,
            None => return Ok(None),
        };
        if data_len == 0 {
            c.to_vec()
        } else {
            let mut z = ZlibDecoder::new(c);
            let mut out = Vec::with_capacity(data_len);
            z.read_to_end(&mut out)?;
            if out.len() != data_len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "taille décompressée incohérente",
                ));
            }
            out
        }
    };
    Ok(Some((data, consumed)))
}

fn decode_varint_exact(buf: &mut &[u8]) -> Option<i32> {
    let mut num: i32 = 0;
    for i in 0..5 {
        if buf.is_empty() {
            return None;
        }
        let b = buf[0];
        *buf = &buf[1..];
        num |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Some(num);
        }
    }
    Some(num)
}

/// Connexion TCP Minecraft : lecture/écriture de trames avec compression optionnelle.
/// La lecture passe par un tampon interne : `read_packet` est *cancel-safe*
/// (utilisé dans `tokio::select!` sans risque de désynchronisation).
pub struct Conn {
    stream: TcpStream,
    compression: Option<i32>,
    rbuf: Vec<u8>,
}

pub enum ReadOutcome {
    Packet(i32, Vec<u8>),
    Eof,
}

impl Conn {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            compression: None,
            rbuf: Vec::new(),
        }
    }

    pub fn set_compression(&mut self, threshold: i32) {
        self.compression = Some(threshold);
    }

    pub fn stream(&self) -> &TcpStream {
        &self.stream
    }

    async fn fill(&mut self) -> std::io::Result<usize> {
        let mut tmp = [0u8; 8192];
        let n = self.stream.read(&mut tmp).await?;
        self.rbuf.extend_from_slice(&tmp[..n]);
        Ok(n)
    }

    pub async fn read_packet(&mut self) -> std::io::Result<ReadOutcome> {
        loop {
            if let Some((frame, consumed)) = try_parse_frame(&self.rbuf, self.compression.is_some())? {
                self.rbuf.drain(..consumed);
                let mut cur: &[u8] = &frame;
                let id = decode_varint(&mut cur);
                return Ok(ReadOutcome::Packet(id, cur.to_vec()));
            }
            if self.fill().await? == 0 {
                return Ok(ReadOutcome::Eof);
            }
        }
    }

    pub async fn send(&mut self, id: i32, body: &[u8]) -> std::io::Result<()> {
        let mut payload = Vec::with_capacity(varint_len(id) + body.len());
        write_varint(id, &mut payload);
        payload.extend_from_slice(body);
        let frame = match self.compression {
            None => {
                let mut f = Vec::with_capacity(varint_len(payload.len() as i32) + payload.len());
                write_varint(payload.len() as i32, &mut f);
                f.extend_from_slice(&payload);
                f
            }
            Some(th) => {
                let mut f = Vec::new();
                if (payload.len() as i32) < th {
                    write_varint(0, &mut f);
                    f.extend_from_slice(&payload);
                } else {
                    let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                    z.write_all(&payload)?;
                    let c = z.finish()?;
                    write_varint(payload.len() as i32, &mut f);
                    f.extend_from_slice(&c);
                }
                let mut full = Vec::with_capacity(varint_len(f.len() as i32) + f.len());
                write_varint(f.len() as i32, &mut full);
                full.extend_from_slice(&f);
                full
            }
        };
        self.stream.write_all(&frame).await?;
        self.stream.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_allonge() {
        for v in [0, 1, 127, 128, 255, 25565, 767, 2097151, i32::MAX, -1] {
            let mut b = Vec::new();
            write_varint(v, &mut b);
            assert_eq!(b.len(), varint_len(v));
            let mut s: &[u8] = &b;
            assert_eq!(decode_varint(&mut s), v, "v={}", v);
        }
    }

    #[test]
    fn uuid_offline_stable() {
        let a = offline_uuid("LibreBot");
        let b = offline_uuid("LibreBot");
        assert_eq!(a, b);
        assert_eq!(a[6] >> 4, 3);
        assert_eq!(a[8] >> 6, 0b10);
        let c = offline_uuid("autre");
        assert_ne!(a, c);
    }

    #[test]
    fn corps_lecture() {
        let mut b = vec![];
        write_varint(300, &mut b);
        write_string("salut", &mut b);
        b.push(1);
        let mut body = Body(&b);
        assert_eq!(body.varint(), 300);
        assert_eq!(body.string(), "salut");
        assert!(body.bool());
        assert!(body.0.is_empty());
    }
}
