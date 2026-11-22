//! Test de bout en bout : minage confirmé par le serveur (bloc -> air), spawn d'un
//! drop d'item, ramassage par le bot (remove_entities + set_slot), inventaire à jour.

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::entities::ITEM_ENTITY_TYPE;
use librebot_rs::protocol::{write_varint, Body, ReadOutcome};
use librebot_rs::recipes::item_id;
use librebot_rs::world::MIN_Y;
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// Paquet `set_slot` (0x15) fenêtre -1 (inventaire brut), slot hotbar 0.
fn set_slot_player_body(item: i32, count: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0xff); // window -1
    write_varint(1, &mut b); // state id
    b.extend_from_slice(&0i16.to_be_bytes()); // slot 0 (hotbar brute)
    write_varint(count, &mut b); // slot: count
    write_varint(item, &mut b); // id
    write_varint(0, &mut b); // composants ajoutés
    write_varint(0, &mut b); // composants retirés
    b
}

#[tokio::test]
async fn minage_confirmed_puis_ramassage_drop() {
    let (listener, addr) = free_listener().await;
    let drop_id = 9001;
    let log_item = item_id("birch_log");

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, Some(5))).await.unwrap();
        conn.send(0x40, &position_body(2.5, -50.0, 0.5, 100))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        // Drop d'item posé à 3 blocs du spawn du bot
        conn.send(0x01, &spawn_entity_body(drop_id, ITEM_ENTITY_TYPE, 5.5, -63.0, 0.5))
            .await
            .unwrap();

        // Boucle : attendre le finish du minage -> casser le bloc côté serveur ;
        // puis attendre que le bot soit proche du drop -> confirmer le ramassage.
        let mut broke = false;
        let mut picked = false;
        loop {
            let (id, body) = match timeout(Duration::from_secs(20), conn.read_packet()).await {
                Ok(Ok(ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant un paquet du bot"),
            };
            match id {
                0x24 if !broke => {
                    let mut b = Body(&body);
                    let status = b.varint();
                    let loc = b.position();
                    if status == 2 {
                        // fin du minage : le serveur casse le bloc et spawn le drop
                        conn.send(0x09, &block_change_body(loc.0, loc.1, loc.2, 0))
                            .await
                            .unwrap();
                        broke = true;
                    }
                }
                0x1a | 0x1b if broke && !picked => {
                    let mut b = Body(&body);
                    let x = b.f64();
                    let _y = b.f64();
                    let z = b.f64();
                    if (x - 5.5).powi(2) + (z - 0.5).powi(2) < 2.25 {
                        conn.send(0x42, &remove_entities_body(&[drop_id])).await.unwrap();
                        conn.send(0x15, &set_slot_player_body(log_item, 1)).await.unwrap();
                        picked = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut spawned = false;
    while Instant::now() < deadline && !spawned {
        tokio::select! {
            ev = bot.play_turn() => match ev.unwrap() {
                Event::Spawned => spawned = true,
                Event::Disconnected(r) => panic!("déconnecté : {}", r),
                _ => {}
            },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                bot.tick(0.05).await.unwrap();
            }
        }
    }
    assert!(spawned);
    while Instant::now() < deadline && !bot.on_ground {
        tokio::select! {
            ev = bot.play_turn() => {
                if let Event::Disconnected(r) = ev.unwrap() { panic!("déconnecté : {}", r); }
            }
            _ = tokio::time::sleep(Duration::from_millis(25)) => {
                bot.tick(0.05).await.unwrap();
            }
        }
    }
    assert!(bot.on_ground);
    bot.drain_packets().await;
    assert!(bot.entities.contains(drop_id), "drop non suivi");

    // Miner le bloc sous les pieds : le serveur confirme, puis le bot ramasse le drop.
    let ok = bot.goto_and_dig(5, MIN_Y + 1, 0).await;
    assert!(ok, "minage non confirmé par le serveur");
    assert_eq!(
        bot.world.block_name(5, MIN_Y + 1, 0),
        Some("air"),
        "le bloc devrait être de l'air côté client"
    );
    assert!(!bot.entities.contains(drop_id), "drop non ramassé");
    assert!(
        bot.count_item("birch_log") >= 1,
        "inventaire sans birch_log après ramassage"
    );

    server.await.unwrap();
}
