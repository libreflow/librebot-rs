//! Test d'intégration speedrun (serveur mock 1.21.1 loopback) : détection d'un
//! portail ruiné dans les chunks + changement de dimension lu via respawn (0x47).

mod common;

use common::*;
use librebot_rs::blocks_gen::block_state_by_name;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_string, write_varint};
use librebot_rs::world::MIN_Y;
use std::time::Duration;
use tokio::time::timeout;

/// Corps du paquet respawn (0x47) 767 avec une dimension donnée (même tête que
/// le SpawnInfo du login : holder varint + nom + hashedSeed + ...).
fn respawn_body(dimension: &str) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(0, &mut b);
    write_string(dimension, &mut b);
    b.extend_from_slice(&0i64.to_be_bytes());
    b.push(0); // gamemode
    b.push(0xff); // previous gamemode
    b.push(0); // debug
    b.push(0); // flat
    b.push(0); // death location absent
    write_varint(0, &mut b); // portal cooldown
    b
}

#[tokio::test]
async fn portail_ruine_detecte_et_dimension_nether_lue() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None))
            .await
            .unwrap();
        // Portail du Nether posé en (3, MIN_Y+2, 3) via block_change (0x09).
        let portal = block_state_by_name("nether_portal").unwrap() as i32;
        conn.send(0x09, &block_change_body(3, MIN_Y + 2, 3, portal))
            .await
            .unwrap();
        conn.send(0x40, &position_body(1.5, (MIN_Y + 1) as f64, 1.5, 1))
            .await
            .unwrap();
        // Changement de dimension (entrée dans le Nether).
        conn.send(0x47, &respawn_body("minecraft:the_nether"))
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    // Consommer jusqu'au spawn.
    loop {
        match timeout(Duration::from_secs(5), bot.play_turn()).await {
            Ok(Ok(Event::Connected)) => {
                assert_eq!(bot.dimension.as_deref(), Some("minecraft:overworld"))
            }
            Ok(Ok(Event::Spawned)) => break,
            Ok(Ok(_)) => {}
            other => panic!("attendu Connected/Spawned, reçu {:?}", other.is_ok()),
        }
    }
    bot.drain_packets().await; // block_change + respawn
    assert_eq!(bot.dimension.as_deref(), Some("minecraft:the_nether"));
    let found = bot.find_block("nether_portal", 64);
    assert_eq!(found, Some((3, MIN_Y + 2, 3)), "portail ruiné non détecté");

    server.await.unwrap();
}
