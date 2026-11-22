//! Test d'intégration : serveur mock Minecraft Java 1.21.1 offline sur loopback.
//! Valide handshake, login offline, compression, configuration (ping, known packs,
//! keep-alive, finish), jeu (keep-alive, chat système, kick) et envoi de chat.

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{Body, ReadOutcome};
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn session_complete_offline() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        expect(&mut conn, 0x0a).await; // client_information renvoyé en PLAY

        let mut pk = Vec::new();
        pk.extend_from_slice(&123456789i64.to_be_bytes());
        conn.send(0x26, &pk).await.unwrap();

        let mut chat = nbt_text_compound("Bonjour LibreBot");
        chat.push(0);
        conn.send(0x6c, &chat).await.unwrap();

        let mut keepalive_ok = false;
        let mut chat_ok = false;
        for _ in 0..2 {
            let (id, body) = match timeout(Duration::from_secs(5), conn.read_packet()).await {
                Ok(Ok(ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout pendant l'échange de jeu"),
            };
            match id {
                0x18 => {
                    assert_eq!(Body(&body).i64(), 123456789);
                    keepalive_ok = true;
                }
                0x06 => {
                    let mut s = Body(&body);
                    assert_eq!(s.string(), "salut le serveur");
                    chat_ok = true;
                }
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        }
        assert!(keepalive_ok, "réponse keep-alive manquante");
        assert!(chat_ok, "message de chat manquant");

        conn.send(0x1d, &nbt_text_compound("test terminé"))
            .await
            .unwrap();
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    match bot.play_turn().await.unwrap() {
        Event::Connected => {
            assert_eq!(bot.entity_id, Some(7));
            assert_eq!(bot.dimension.as_deref(), Some("minecraft:overworld"));
        }
        other => panic!("attendu Connected, reçu {:?}", other),
    }
    bot.say("salut le serveur").await.unwrap();

    match bot.play_turn().await.unwrap() {
        Event::Chat(t) => assert_eq!(t, "Bonjour LibreBot"),
        other => panic!("attendu Chat, reçu {:?}", other),
    }
    match bot.play_turn().await.unwrap() {
        Event::Disconnected(r) => assert_eq!(r, "test terminé"),
        other => panic!("attendu Disconnected, reçu {:?}", other),
    }

    server.await.unwrap();
}
