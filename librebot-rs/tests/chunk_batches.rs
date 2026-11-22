//! Test de conformité : le flux Paper complet des lots de chunks en 767.
//! Le serveur envoie chunk_batch_start → 2 map_chunk → chunk_batch_finished ;
//! le client doit acquitter avec chunk_batch_received (f32 > 0) et charger
//! les deux chunks. Sans cet acquittement, un serveur vanilla/Paper gèle totalement la suite de l'envoi des chunks.

mod common;

use common::*;
use librebot_rs::bot::Bot;
use librebot_rs::protocol::Body;
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::test]
async fn lots_de_chunks_acquittes() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        expect(&mut conn, 0x0a).await; // client_information en PLAY

        // Position initiale + confirmation de téléport attendue.
        conn.send(0x40, &position_body(2.5, -50.0, 0.5, 100))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        // Lot de chunks façon Paper : start, 2 chunks, finished.
        conn.send(0x0d, &[]).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(1, 0, None)).await.unwrap();
        let mut fin = Vec::new();
        fin.extend_from_slice(&1.5f32.to_be_bytes()); // size on wire : chunks per tick
        conn.send(0x0c, &fin).await.unwrap();

        // Le client DOIT acquitter le lot avec un f32 strictement positif.
        // (tolérer les position_look 0x1b intercalés pendant les chutes)
        let ack = loop {
            let (id, body) = match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant l'acquittement du lot"),
            };
            match id {
                0x1b | 0x1a => continue,
                0x08 => break body,
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        };
        let mut b = Body(&ack);
        let rythme = b.f32();
        assert!(
            rythme > 0.0,
            "chunk_batch_received doit porter un f32 > 0, reçu {}",
            rythme
        );

        // Le lot suivant est envoyé : le client doit à nouveau acquitter.
        conn.send(0x0d, &[]).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 1, None)).await.unwrap();
        conn.send(0x0c, &fin).await.unwrap();
        loop {
            let (id, _body) = match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant le 2e acquittement"),
            };
            match id {
                0x1b | 0x1a => continue,
                0x08 => break,
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        }

        // Keep-alive : le client doit bien renvoyer l'identifiant.
        let mut ka = Vec::new();
        ka.extend_from_slice(&123456789i64.to_be_bytes());
        conn.send(0x26, &ka).await.unwrap();
        loop {
            let (id, body) = match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(librebot_rs::protocol::ReadOutcome::Packet(id, body))) => (id, body),
                _ => panic!("timeout en attendant le keep-alive"),
            };
            match id {
                0x1b | 0x1a => continue,
                0x18 => {
                    assert_eq!(Body(&body).i64(), 123456789);
                    break;
                }
                other => panic!("paquet inattendu 0x{:x}", other),
            }
        }
    });

    let mut bot = timeout(
        Duration::from_secs(20),
        Bot::connect_with_view("127.0.0.1", addr.port(), "LibreBot", 3),
    )
    .await
    .expect("connexion")
    .expect("connexion ok");
    let debut = Instant::now();
    while bot.world.chunk_count() < 3 && debut.elapsed() < Duration::from_secs(15) {
        let _ = timeout(Duration::from_millis(200), bot.play_turn()).await;
        bot.tick(0.05).await.ok();
    }
    assert_eq!(
        bot.world.chunk_count(),
        3,
        "les 3 chunks des lots doivent être chargés"
    );
    assert_eq!(bot.stats_batches, 2, "les 2 lots doivent être acquittés");
    server.abort();
    let _ = addr;
}
