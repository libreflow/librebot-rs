//! Test d'intégration : minage (block_dig start/finish) + block_change + pose.
//! Le bot miné la stone sous ses pieds (2,25 s à mains nues), le serveur
//! confirme via block_change (air) ; puis le bot pose une stone sur le sol.

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::Body;
use librebot_rs::world::MIN_Y;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn mine_et_pose() {
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

        // Le bot atterrit puis commence à miner : on attend le start du block_dig.
        let (dig, seq) = loop {
            let (id, body) = match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant un paquet"),
            };
            match id {
                0x1b => {}
                0x24 => {
                    let mut b = Body(&body);
                    let status = b.varint();
                    let loc = b.position();
                    let _face = b.u8();
                    let seq = b.varint();
                    assert_eq!(status, 0, "premier block_dig doit être un start");
                    assert_eq!(loc, (2, MIN_Y, 0));
                    break (loc, seq);
                }
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        };
        let _ = dig;

        // Le serveur casse le bloc.
        conn.send(0x09, &block_change_body(2, MIN_Y, 0, 0)).await.unwrap();

        // Puis le bot doit envoyer le finish (avec la même séquence) ;
        // entre-temps il tombe dans le trou qu'il vient de creuser : ignorer les 0x1b.
        let (finish, seq_finish, loc_finish) = loop {
            let (id, body) = match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant le finish"),
            };
            match id {
                0x1b => continue,
                0x24 => {
                    let mut b = Body(&body);
                    let status = b.varint();
                    let loc = b.position();
                    let _face = b.u8();
                    let seq = b.varint();
                    break (status, seq, loc);
                }
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        };
        assert_eq!(finish, 2, "status finish attendu");
        assert_eq!(loc_finish, (2, MIN_Y, 0));
        assert_eq!(seq_finish, seq, "séquence incohérente");

        // Enfin la pose : block_place sur le sol devant.
        let place = expect(&mut conn, 0x38).await;
        let mut b = Body(&place);
        assert_eq!(b.varint(), 0, "main");
        let loc = b.position();
        let direction = b.varint();
        let _ = b.f32();
        let _ = b.f32();
        let _ = b.f32();
        let _inside = b.bool();
        let place_seq = b.varint();
        assert_eq!(loc, (2, MIN_Y, 1));
        assert_eq!(direction, 3, "face +Z");
        assert!(place_seq > seq);
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
        tokio::time::sleep(Duration::from_millis(25)).await;
        bot.tick(0.05).await.unwrap();
    }
    assert!(bot.on_ground);
    assert_eq!(bot.world.block_name(2, MIN_Y, 0), Some("stone"));

    let secs = bot.dig(2, MIN_Y, 0).await;
    assert!(secs.unwrap() > 2.0, "le minage de stone doit prendre > 2 s");

    while Instant::now() < deadline && !bot.mining_done() {
        tokio::select! {
            ev = bot.play_turn() => {
                if let Event::Disconnected(r) = ev.unwrap() { panic!("déconnecté : {}", r) }
            },
            _ = tokio::time::sleep(Duration::from_millis(25)) => {
                bot.tick(0.05).await.unwrap();
            }
        }
    }
    assert!(bot.mining_done(), "minage jamais terminé");
    assert_eq!(bot.world.block_name(2, MIN_Y, 0), Some("air"), "le monde n'est pas à jour");

    bot.place(2, MIN_Y, 1, 3).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    server.await.unwrap();
}
