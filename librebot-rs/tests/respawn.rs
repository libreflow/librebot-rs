//! Test d'intégration : mort du bot et respawn automatique.
//! Le serveur mock envoie set_health=0 (0x5d) puis player_combat_kill (0x3c) ;
//! le bot doit répondre client_command (0x09) action 0 = PERFORM_RESPAWN,
//! une seule fois, puis reprendre après le respawn (0x47) + position (0x40).

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_varint, Body};
use std::time::Duration;
use tokio::time::timeout;

fn set_health_body(health: f32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&health.to_be_bytes());
    write_varint(20, &mut b);
    b.extend_from_slice(&5.0f32.to_be_bytes());
    b
}

fn kill_body(player_id: i32) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(player_id, &mut b);
    b.extend_from_slice(&player_id.to_be_bytes());
    b.extend_from_slice(&[0x0a, 0x00, 0x00]);
    b
}

fn position_body(x: f64, y: f64, z: f64, teleport_id: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&x.to_be_bytes());
    b.extend_from_slice(&y.to_be_bytes());
    b.extend_from_slice(&z.to_be_bytes());
    b.extend_from_slice(&0.0f32.to_be_bytes());
    b.extend_from_slice(&0.0f32.to_be_bytes());
    b.push(0);
    write_varint(teleport_id, &mut b);
    b
}

#[tokio::test]
async fn respawn_automatique_apres_mort() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();

        // 1) mort via set_health = 0
        conn.send(0x5d, &set_health_body(0.0)).await.unwrap();
        // le bot doit envoyer client_command (0x09) action 0 = PERFORM_RESPAWN
        let body = expect(&mut conn, 0x09).await;
        assert_eq!(Body(&body).varint(), 0, "action PERFORM_RESPAWN attendue");

        // 2) kill packet aussi : pas de second respawn (deja mort)
        conn.send(0x3c, &kill_body(7)).await.unwrap();

        // 3) le serveur renvoie le joueur : respawn (0x47) puis position (0x40)
        conn.send(0x47, &[]).await.unwrap();
        conn.send(0x40, &position_body(0.5, 65.0, 0.5, 55)).await.unwrap();
        let body = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&body).varint(), 55, "teleport_confirm attendu");
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));

    // mort via set_health=0
    match bot.play_turn().await.unwrap() {
        Event::Died => {
            assert_eq!(bot.health, 0.0);
            assert!(bot.dead);
        }
        other => panic!("attendu Died, reçu {:?}", other),
    }

    // kill packet : deuxième Died, mais send_respawn est idempotent (dead=true)
    match bot.play_turn().await.unwrap() {
        Event::Died => assert!(bot.dead),
        other => panic!("attendu Died (kill), reçu {:?}", other),
    }

    // après respawn (0x47) + position (0x40) : le bot est de retour
    match timeout(Duration::from_secs(5), bot.play_turn()).await {
        Ok(Ok(Event::Spawned)) => {
            assert!(!bot.dead, "dead doit etre remis à false par respawn");
            assert_eq!(bot.health, 20.0);
            assert_eq!(bot.pos, Some([0.5, 65.0, 0.5]));
        }
        other => panic!("attendu Spawned, reçu {:?}", other),
    }

    server.await.unwrap();
}

#[tokio::test]
async fn set_health_positif_ne_declenche_pas_de_respawn() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x5d, &set_health_body(12.5)).await.unwrap();
        // le bot doit continuer : envoyer un chat, aucun client_command attendu
        let body = expect(&mut conn, 0x06).await;
        assert_eq!(Body(&body).string(), "toujours vivant");
        conn.send(0x1d, &nbt_text_compound("fin")).await.unwrap();
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    // set_health=12.5 : pas d'événement, la boucle continue jusqu'au chat suivant
    bot.say("toujours vivant").await.unwrap();
    match bot.play_turn().await.unwrap() {
        Event::Disconnected(r) => assert_eq!(r, "fin"),
        other => panic!("attendu Disconnected, reçu {:?}", other),
    }
    assert_eq!(bot.health, 12.5);
    assert!(!bot.dead);
    server.await.unwrap();
}
