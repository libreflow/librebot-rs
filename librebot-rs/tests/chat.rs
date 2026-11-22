//! Test d'intégration : chat des joueurs (player_chat 0x39) et chat système (0x6c).

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_string, write_varint, Body};
use std::time::{Duration, Instant};
use tokio::time::timeout;

/// player_chat signé minimal de Steve : "coucou".
fn player_chat_body(name: &str, message: &str) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[0u8; 16]); // UUID brut
    write_varint(0, &mut b);
    b.push(0); // pas de signature
    write_string(message, &mut b);
    b.extend_from_slice(&0i64.to_be_bytes());
    b.extend_from_slice(&0i64.to_be_bytes());
    write_varint(0, &mut b); // aucun message précédent
    b.push(0); // pas de contenu non signé
    write_varint(0, &mut b); // pas de filtrage
    write_varint(1, &mut b); // chat type : id
    let name_nbt = nbt_name_compound(name);
    b.extend_from_slice(&name_nbt);
    b.push(0); // pas de cible
    b
}

fn nbt_name_compound(text: &str) -> Vec<u8> {
    let mut b = vec![0x0a];
    b.push(0x08);
    b.extend_from_slice(&4u16.to_be_bytes());
    b.extend_from_slice(b"text");
    b.extend_from_slice(&(text.len() as u16).to_be_bytes());
    b.extend_from_slice(text.as_bytes());
    b.push(0);
    b
}

#[tokio::test]
async fn chat_joueur_et_systeme() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        // Le client renvoie client_information (0x0a) en PLAY juste après login
        expect(&mut conn, 0x0a).await;
        conn.send(0x40, &position_body(2.5, -50.0, 0.5, 100))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        conn.send(0x39, &player_chat_body("Steve", "coucou"))
            .await
            .unwrap();

        let mut sys = nbt_text_compound("[Joueur] Steve a rejoint le jeu");
        sys.push(0);
        conn.send(0x6c, &sys).await.unwrap();

        let _ = timeout(Duration::from_secs(2), conn.read_packet()).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut messages = Vec::new();
    while Instant::now() < deadline && messages.len() < 2 {
        tokio::select! {
            ev = bot.play_turn() => match ev.unwrap() {
                Event::Chat(t) => messages.push(t),
                Event::Disconnected(_) => break,
                _ => {}
            },
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                bot.tick(0.05).await.unwrap();
            }
        }
    }
    assert_eq!(
        messages,
        vec![
            "<Steve> coucou".to_string(),
            "[Joueur] Steve a rejoint le jeu".to_string(),
        ]
    );

    server.await.unwrap();
}
