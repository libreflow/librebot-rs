//! Test d'intégration : marche + A* + saut d'obstacle.
//! Sol de stone à y=-64, muret d'un bloc (x=5, tout le Z du chunk), spawn en (0.5, -50, 0.5).
//! Le bot doit rejoindre (7.5, -63, 0.5) en franchissant le muret d'un saut.

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::Body;
use librebot_rs::world::MIN_Y;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn marche_saut_et_arrivee() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        expect(&mut conn, 0x0a).await; // client_information renvoyé en PLAY
        conn.send(0x27, &floor_chunk_body(0, 0, Some(5))).await.unwrap();
        conn.send(0x40, &position_body(0.5, -50.0, 0.5, 100))
            .await
            .unwrap();

        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        let floor_top = (MIN_Y + 1) as f64;
        let wall_top = (MIN_Y + 2) as f64;
        let mut max_y = -50.0f64;
        let mut arrived = false;
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            let body = match timeout(Duration::from_secs(2), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(0x1b, body))) => body,
                Ok(Ok(_)) => panic!("paquet inattendu pendant la marche"),
                _ => panic!("timeout : aucune position reçue"),
            };
            let mut b = Body(&body);
            let x = b.f64();
            let y = b.f64();
            let z = b.f64();
            let (_yaw, _pitch) = (b.f32(), b.f32());
            let on_ground = b.bool();
            max_y = max_y.max(y);
            if on_ground && (x - 7.5).abs() < 0.3 && (z - 0.5).abs() < 0.3 {
                assert!(
                    (y - floor_top).abs() < 0.05,
                    "y final = {} au lieu de {}",
                    y,
                    floor_top
                );
                arrived = true;
                break;
            }
        }
        assert!(arrived, "jamais arrivé (y max = {})", max_y);
        assert!(max_y >= wall_top - 0.05, "jamais sauté : y max = {}", max_y);
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    let mut spawned = false;
    let deadline = Instant::now() + Duration::from_secs(10);
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
    assert!(spawned, "jamais spawné");
    while Instant::now() < deadline && !bot.on_ground {
        tokio::time::sleep(Duration::from_millis(25)).await;
        bot.tick(0.05).await.unwrap();
    }
    assert!(bot.on_ground, "jamais au sol de départ");

    let steps = bot.goto(7, MIN_Y + 1, 0).await;
    assert!(steps.is_some(), "pas de chemin trouvé");
    assert!(steps.unwrap() > 1, "chemin trop court");

    while Instant::now() < deadline && bot.path_len() > 0 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        bot.tick(0.05).await.unwrap();
    }
    assert_eq!(bot.path_len(), 0, "chemin non terminé");
    let p = bot.pos.unwrap();
    assert!((p[0] - 7.5).abs() < 0.3, "x final = {}", p[0]);
    assert!((p[1] - (MIN_Y + 1) as f64).abs() < 0.05, "y final = {}", p[1]);

    server.await.unwrap();
}
