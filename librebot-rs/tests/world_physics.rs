//! Test d'intégration : chunks + position/téléport + physique de chute.
//! Le mock place une stone à (0, -64, 0), téléporte le bot en (0.5, -50, 0.5) :
//! Le bot doit confirmer le téléport puis tomber jusqu'à y = -63 (sommet du bloc).

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::Body;
use librebot_rs::world::MIN_Y;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn chute_et_aterrissage() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        expect(&mut conn, 0x0a).await; // client_information renvoyé en PLAY
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        conn.send(0x40, &position_body(0.5, -50.0, 0.5, 100))
            .await
            .unwrap();

        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        let expected_top = (MIN_Y + 1) as f64;
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last_y = -50.0f64;
        let mut landed = false;
        while Instant::now() < deadline {
            let body = match timeout(Duration::from_secs(2), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(0x1b, body))) => body,
                Ok(Ok(_)) => panic!("paquet inattendu pendant la chute"),
                _ => panic!("timeout : pas de position reçue"),
            };
            let mut b = Body(&body);
            let _x = b.f64();
            let y = b.f64();
            let _z = b.f64();
            let (_yaw, _pitch) = (b.f32(), b.f32());
            let on_ground = b.bool();
            assert!(y <= last_y + 1e-9, "le bot remonte : {} > {}", y, last_y);
            last_y = y;
            if on_ground && (y - expected_top).abs() < 0.05 {
                landed = true;
                break;
            }
        }
        assert!(landed, "jamais atterri (dernier y = {})", last_y);
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
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
    assert!(spawned, "jamais spawné côté bot");

    while Instant::now() < deadline && !bot.on_ground {
        tokio::time::sleep(Duration::from_millis(25)).await;
        bot.tick(0.05).await.unwrap();
    }
    assert!(bot.on_ground, "jamais on_ground côté bot");
    let p = bot.pos.unwrap();
    assert!((p[1] - (MIN_Y + 1) as f64).abs() < 0.05, "y final = {}", p[1]);
    assert_eq!(bot.world.block_name(0, MIN_Y, 0), Some("stone"));
    assert!(bot.world.is_solid(0, MIN_Y, 0));

    server.await.unwrap();
}
