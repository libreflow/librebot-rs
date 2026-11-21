use std::io::{Read, Write};
use std::net::TcpStream;

fn varint(mut n: i32, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n != 0 { out.push(b | 0x80); } else { out.push(b); break; }
    }
}

fn main() {
    let mut s = TcpStream::connect("127.0.0.1:58654").expect("connect");
    s.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();

    let host = b"127.0.0.1";
    let mut hs = Vec::new();
    varint(767, &mut hs);
    varint(host.len() as i32, &mut hs);
    hs.extend_from_slice(host);
    hs.extend_from_slice(&58654u16.to_be_bytes());
    varint(1, &mut hs); // next state = 1 (status)
    let mut pkt = Vec::new();
    varint(0, &mut pkt);
    pkt.extend_from_slice(&hs);
    let mut framed = Vec::new();
    varint(pkt.len() as i32, &mut framed);
    framed.extend_from_slice(&pkt);
    s.write_all(&framed).unwrap();

    let req = vec![0x01, 0x00];
    s.write_all(&req).unwrap();

    let mut buf = [0u8; 4096];
    match s.read(&mut buf) {
        Ok(n) => {
            println!("STATUS RESPONSE ({} octets):", n);
            println!("Hex: {}", buf[..n.min(500)].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" "));
            let data = &buf[..n];
            let mut i = 0;
            loop { let b = data[i]; i += 1; if b & 0x80 == 0 { break; } }
            i += 1; // packet id
            let mut len = 0i32; let mut shift = 0;
            loop { let b = data[i]; i += 1; len |= ((b & 0x7F) as i32) << shift; shift += 7; if b & 0x80 == 0 { break; } }
            let json = String::from_utf8_lossy(&data[i..i + len as usize]);
            println!("JSON: {}", json);
        }
        Err(e) => println!("Erreur lecture: {}", e),
    }
}
