//! Helpers communs aux tests d'intégration : prologue serveur (login → configuration)
//! et constructeurs de paquets play (login, chunk, position).
#![allow(dead_code)]

use librebot_rs::protocol::{write_string, write_varint, Body, Conn, ReadOutcome};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::time::timeout;

pub async fn free_listener() -> (TcpListener, std::net::SocketAddr) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let a = l.local_addr().unwrap();
    (l, a)
}

pub async fn expect(conn: &mut Conn, want_id: i32) -> Vec<u8> {
    match timeout(Duration::from_secs(5), conn.read_packet()).await {
        Ok(Ok(ReadOutcome::Packet(id, body))) => {
            assert_eq!(id, want_id, "paquet attendu 0x{:x}, reçu 0x{:x}", want_id, id);
            body
        }
        Ok(Ok(ReadOutcome::Eof)) => panic!("connexion fermée (attendu 0x{:x})", want_id),
        Ok(Err(e)) => panic!("erreur de lecture : {}", e),
        Err(_) => panic!("timeout en attendant 0x{:x}", want_id),
    }
}

/// Prologue serveur : handshake + login_start + compress + success + configuration
/// (ping, known packs, keep-alive, finish). Le bot est prêt pour l'état play.
pub async fn prologue(listener: TcpListener) -> Conn {
    let (stream, _) = listener.accept().await.unwrap();
    let mut conn = Conn::new(stream);

    let hs = expect(&mut conn, 0x00).await;
    let mut h = Body(&hs);
    assert_eq!(h.varint(), 767);
    let _host = h.string();

    let ls = expect(&mut conn, 0x00).await;
    let username = Body(&ls).string();
    assert_eq!(username, "LibreBot");

    let mut compress = Vec::new();
    write_varint(8, &mut compress);
    conn.send(0x03, &compress).await.unwrap();
    conn.set_compression(8);

    let mut success = Vec::new();
    write_string("00000000-0000-0000-0000-000000000000", &mut success);
    write_string("LibreBot", &mut success);
    conn.send(0x02, &success).await.unwrap();
    expect(&mut conn, 0x03).await;

    let mut ping = Vec::new();
    ping.extend_from_slice(&42i32.to_be_bytes());
    conn.send(0x05, &ping).await.unwrap();
    let pong = expect(&mut conn, 0x05).await;
    assert_eq!(Body(&pong).i32(), 42);

    conn.send(0x0e, &[0x00]).await.unwrap();
    let packs = expect(&mut conn, 0x07).await;
    {
        let mut p = Body(&packs);
        assert_eq!(p.varint(), 1);
        assert_eq!(p.string(), "minecraft");
        assert_eq!(p.string(), "core");
        assert_eq!(p.string(), librebot_rs::protocol::MC_VERSION);
    }
    expect(&mut conn, 0x00).await;

    let mut ka = Vec::new();
    ka.extend_from_slice(&999i64.to_be_bytes());
    conn.send(0x04, &ka).await.unwrap();
    let kar = expect(&mut conn, 0x04).await;
    assert_eq!(Body(&kar).i64(), 999);

    conn.send(0x03, &[]).await.unwrap();
    expect(&mut conn, 0x03).await;
    conn
}

/// Paquet `login` (0x2b) pour un monde overworld survie.
pub fn login_body(entity_id: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&entity_id.to_be_bytes());
    b.push(0);
    write_varint(0, &mut b);
    write_varint(20, &mut b);
    write_varint(8, &mut b);
    write_varint(8, &mut b);
    b.push(0);
    b.push(1);
    b.push(0);
    write_varint(0, &mut b);
    write_string("minecraft:overworld", &mut b);
    b.extend_from_slice(&0i64.to_be_bytes());
    b.push(0);
    b.push(0xff);
    b.push(0);
    b.push(0);
    b.push(0);
    write_varint(0, &mut b);
    b.push(0);
    b
}

/// Chunk avec un sol de stone à MIN_Y (couche unique) et, si `wall_x`, un muret
/// d'un bloc de haut à cette colonne X sur tout le Z du chunk.
pub fn floor_chunk_body(cx: i32, cz: i32, wall_x: Option<i32>) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(&cx.to_be_bytes());
    body.extend_from_slice(&cz.to_be_bytes());
    body.push(0x0a);
    body.push(0x00);

    let mut blob = Vec::new();
    let bits = 4u8;
    for s in 0..24u32 {
        blob.extend_from_slice(&0u16.to_be_bytes());
        if s == 0 {
            blob.push(bits);
            write_varint(2, &mut blob);
            write_varint(0, &mut blob);
            write_varint(1, &mut blob);
            write_varint(256, &mut blob); // longueur du tableau de longs
            for long in 0..256u32 {
                let v: u64 = if long < 16 {
                    0x1111111111111111
                } else if (16..32).contains(&long) {
                    match wall_x {
                        Some(wx) => 1u64 << (4 * wx as u32),
                        None => 0,
                    }
                } else {
                    0
                };
                blob.extend_from_slice(&v.to_be_bytes());
            }
        } else {
            blob.push(0x00);
            write_varint(0, &mut blob);
        }
        blob.push(0x00); // biome : bits = 0 (valeur unique)
        write_varint(0, &mut blob); // id
        blob.push(0x00); // hasData = false
    }
    write_varint(blob.len() as i32, &mut body);
    body.extend_from_slice(&blob);
    body
}

/// Paquet `position` (0x40) : téléport absolu + id à confirmer.
pub fn position_body(x: f64, y: f64, z: f64, teleport_id: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&x.to_be_bytes());
    b.extend_from_slice(&y.to_be_bytes());
    b.extend_from_slice(&z.to_be_bytes());
    b.extend_from_slice(&0f32.to_be_bytes());
    b.extend_from_slice(&0f32.to_be_bytes());
    b.push(0);
    write_varint(teleport_id, &mut b);
    b
}

pub fn nbt_text_compound(text: &str) -> Vec<u8> {
    let mut b = vec![0x0a];
    b.push(0x08);
    b.extend_from_slice(&4u16.to_be_bytes());
    b.extend_from_slice(b"text");
    b.extend_from_slice(&(text.len() as u16).to_be_bytes());
    b.extend_from_slice(text.as_bytes());
    b.push(0);
    b
}

/// Paquet `block_change` (0x09) : remplace l'état du bloc.
pub fn block_change_body(x: i32, y: i32, z: i32, state: i32) -> Vec<u8> {
    let mut b = Vec::new();
    librebot_rs::protocol::write_position(x, y, z, &mut b);
    librebot_rs::protocol::write_varint(state, &mut b);
    b
}
