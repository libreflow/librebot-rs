//! Test d'intégration : suivi des entités + ramassage d'un drop d'item.
//! Le serveur mock fait apparaître une entité `item` près du bot ; le bot doit
//! la repérer, s'en approcher, et le serveur confirme le ramassage (remove_entities).

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::entities::ITEM_ENTITY_TYPE;
use librebot_rs::protocol::{Body, ReadOutcome};
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn ramassage_drop_item() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        conn.send(0x40, &position_body(2.5, -50.0, 0.5, 100))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        // Drop d'item à 3 blocs du spawn du bot (sur le sol en -63)
        let drop_id = 4242;
        conn.send(0x01, &spawn_entity_body(drop_id, ITEM_ENTITY_TYPE, 5.5, -63.0, 0.5))
            .await
            .unwrap();

        // Lire les mouvements du bot ; quand il est assez proche du drop,
        // le serveur confirme le ramassage.
        loop {
            let (id, body) = match timeout(Duration::from_secs(15), conn.read_packet()).await {
                Ok(Ok(ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant les mouvements du bot"),
            };
            if id == 0x1a || id == 0x1b {
                let mut b = Body(&body);
                let x = b.f64();
                let _y = b.f64();
                let z = b.f64();
                if (x - 5.5).powi(2) + (z - 0.5).powi(2) < 4.0 {
                    conn.send(0x42, &remove_entities_body(&[drop_id]))
                        .await
                        .unwrap();
                    break;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
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

    assert!(bot.entities.contains(4242), "entité item non suivie");
    assert_eq!(bot.entities.count_items(), 1);

    let n = bot.collect_drops(8.0).await;
    assert!(n >= 1, "aucun drop ramassé");
    assert!(
        !bot.entities.contains(4242),
        "entité toujours présente après ramassage"
    );

    server.await.unwrap();
}
