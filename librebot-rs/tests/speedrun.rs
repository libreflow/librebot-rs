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

/// Barter : un piglin spawné est détecté (kind 78) et `interact_entity` envoie
/// bien Use Entity (0x16, action interact) puis Swing (0x36).
#[tokio::test]
async fn barter_piglin_use_entity_016() {
    use librebot_rs::entities::PIGLIN_ENTITY_TYPE;

    let (listener, addr) = free_listener().await;
    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None))
            .await
            .unwrap();
        conn.send(0x40, &position_body(1.5, (MIN_Y + 1) as f64, 1.5, 1))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await; // teleport_confirm
        assert_eq!(librebot_rs::protocol::Body(&confirm).varint(), 1);
        // Piglin (kind 78) à 3 blocs.
        conn.send(
            0x01,
            &spawn_entity_body(42, PIGLIN_ENTITY_TYPE, 4.5, (MIN_Y + 1) as f64, 1.5),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        // Le bot doit envoyer use_entity (0x16) puis swing (0x36).
        let body = expect(&mut conn, 0x16).await;
        let mut b = librebot_rs::protocol::Body(&body);
        assert_eq!(b.varint(), 42, "target entity");
        assert_eq!(b.varint(), 0, "action interact (barter)");
        assert_eq!(b.varint(), 0, "main hand");
        assert_eq!(b.u8(), 0, "sneaking");
        let _swing = expect(&mut conn, 0x36).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    loop {
        match timeout(Duration::from_secs(5), bot.play_turn()).await {
            Ok(Ok(Event::Connected)) => {}
            Ok(Ok(Event::Spawned)) => break,
            Ok(Ok(_)) => {}
            _ => panic!("attendu Connected/Spawned"),
        }
    }
    bot.drain_packets().await; // spawn_entity du piglin
    let (pid, _) = bot
        .entities
        .nearest_kind(PIGLIN_ENTITY_TYPE, bot.pos.unwrap_or([0.0; 3]), 24.0)
        .expect("piglin non détecté");
    assert_eq!(pid, 42);
    bot.interact_entity(pid, false).await.unwrap();

    server.await.unwrap();
}

/// Combat : un blaze spawné (kind 8) est détecté et `interact_entity(.., true)`
/// envoie Use Entity (0x16) avec action attack = 1.
#[tokio::test]
async fn combat_blaze_use_entity_attack() {
    use librebot_rs::entities::BLAZE_ENTITY_TYPE;

    let (listener, addr) = free_listener().await;
    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None))
            .await
            .unwrap();
        conn.send(0x40, &position_body(1.5, (MIN_Y + 1) as f64, 1.5, 1))
            .await
            .unwrap();
        let _ = expect(&mut conn, 0x00).await; // teleport_confirm
        conn.send(
            0x01,
            &spawn_entity_body(77, BLAZE_ENTITY_TYPE, 3.5, (MIN_Y + 1) as f64, 1.5),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let body = expect(&mut conn, 0x16).await;
        let mut b = librebot_rs::protocol::Body(&body);
        assert_eq!(b.varint(), 77, "target blaze");
        assert_eq!(b.varint(), 1, "action attack");
        let _swing = expect(&mut conn, 0x36).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    loop {
        match timeout(Duration::from_secs(5), bot.play_turn()).await {
            Ok(Ok(Event::Connected)) => {}
            Ok(Ok(Event::Spawned)) => break,
            Ok(Ok(_)) => {}
            _ => panic!("attendu Connected/Spawned"),
        }
    }
    bot.drain_packets().await;
    let (bid, _) = bot
        .entities
        .nearest_kind(BLAZE_ENTITY_TYPE, bot.pos.unwrap_or([0.0; 3]), 24.0)
        .expect("blaze non détecté");
    assert_eq!(bid, 77);
    bot.interact_entity(bid, true).await.unwrap();

    server.await.unwrap();
}

/// End : le dragon (kind 31) est détecté et attaqué (use_entity 0x16 action=1).
#[tokio::test]
async fn combat_dragon_use_entity_attack() {
    use librebot_rs::entities::ENDER_DRAGON_ENTITY_TYPE;

    let (listener, addr) = free_listener().await;
    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None))
            .await
            .unwrap();
        conn.send(0x40, &position_body(1.5, (MIN_Y + 1) as f64, 1.5, 1))
            .await
            .unwrap();
        let _ = expect(&mut conn, 0x00).await;
        conn.send(
            0x01,
            &spawn_entity_body(99, ENDER_DRAGON_ENTITY_TYPE, 10.5, (MIN_Y + 5) as f64, 1.5),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let body = expect(&mut conn, 0x16).await;
        let mut b = librebot_rs::protocol::Body(&body);
        assert_eq!(b.varint(), 99, "target dragon");
        assert_eq!(b.varint(), 1, "action attack");
        let _ = expect(&mut conn, 0x36).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    loop {
        match timeout(Duration::from_secs(5), bot.play_turn()).await {
            Ok(Ok(Event::Connected)) => {}
            Ok(Ok(Event::Spawned)) => break,
            Ok(Ok(_)) => {}
            _ => panic!("attendu Connected/Spawned"),
        }
    }
    bot.drain_packets().await;
    let (did, _) = bot
        .entities
        .nearest_kind(ENDER_DRAGON_ENTITY_TYPE, bot.pos.unwrap_or([0.0; 3]), 64.0)
        .expect("dragon non détecté");
    assert_eq!(did, 99);
    bot.interact_entity(did, true).await.unwrap();

    server.await.unwrap();
}
