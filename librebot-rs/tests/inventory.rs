//! Test d'intégration : inventaire — window_items, sélection hotbar, click window,
//! set_slot de retour. Le mock donne 64 stone au slot 0 puis déplace un stack.

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_varint, Body};
use std::time::{Duration, Instant};
use tokio::time::timeout;

fn slot_body(id: i32, count: i32) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(count, &mut b);
    if count > 0 {
        write_varint(id, &mut b);
        write_varint(0, &mut b);
        write_varint(0, &mut b);
    }
    b
}

fn window_items_body(state_id: i32, defined: &[(usize, i32, i32)]) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0u8);
    write_varint(state_id, &mut b);
    write_varint(46, &mut b);
    for slot in 0..46 {
        if let Some((_, id, count)) = defined.iter().find(|(s, _, _)| *s == slot) {
            b.extend_from_slice(&slot_body(*id, *count));
        } else {
            write_varint(0, &mut b);
        }
    }
    write_varint(0, &mut b);
    b
}

fn set_slot_body(state_id: i32, slot: i16, id: i32, count: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(0u8);
    write_varint(state_id, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.extend_from_slice(&slot_body(id, count));
    b
}

#[tokio::test]
async fn inventaire_hotbar_et_click() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();

        conn.send(0x13, &window_items_body(7, &[(36, 1, 64), (9, 5, 3)]))
            .await
            .unwrap();

        conn.send(0x40, &position_body(2.5, -50.0, 0.5, 100))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 100);

        // Sélection du slot 2 de la hotbar.
        let held = expect(&mut conn, 0x2f).await;
        assert_eq!(Body(&held).i16(), 2);

        // Clic sur le slot 9 : le bot doit réutiliser le state id 7.
        let click = expect(&mut conn, 0x0e).await;
        let mut b = Body(&click);
        assert_eq!(b.u8(), 0, "fenêtre 0");
        assert_eq!(b.varint(), 7, "state id");
        assert_eq!(b.i16(), 9, "slot");
        assert_eq!(b.u8(), 0, "bouton gauche");
        assert_eq!(b.varint(), 0, "mode pickup");
        assert_eq!(b.varint(), 0, "changedSlots vide");
        assert_eq!(b.varint(), 0, "cursorItem vide");

        // Le serveur accuse la transaction : le stack part vers le curseur.
        conn.send(0x15, &set_slot_body(8, 9, 0, 0)).await.unwrap();
        conn.send(0x15, &set_slot_body(8, -1, 5, 3)).await.unwrap();
        let _ = timeout(Duration::from_secs(2), conn.read_packet()).await;
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
    assert!(spawned, "jamais spawné");

    assert_eq!(bot.inventory.slots.len(), 46);
    assert_eq!(bot.inventory.state_id, 7);
    assert_eq!(bot.inventory.count(1), 64, "64 stone attendus");
    assert_eq!(bot.inventory.find(1), Some(36), "stone dans la hotbar");
    assert_eq!(
        bot.inventory.held(0).map(|s| (s.id, s.count)),
        Some((1, 64))
    );

    bot.select_hotbar(2).await.unwrap();
    bot.click_slot(9).await.unwrap();

    let mut updated = false;
    while Instant::now() < deadline && !updated {
        let disconnected = tokio::select! {
            ev = bot.play_turn() => matches!(ev.unwrap(), Event::Disconnected(_)),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {
                bot.tick(0.05).await.unwrap();
                false
            }
        };
        updated = bot.inventory.state_id == 8
            && bot.inventory.slots[9].is_none()
            && bot.inventory.carried.as_ref().map(|s| (s.id, s.count)) == Some((5, 3));
        if disconnected {
            break;
        }
    }
    assert!(updated, "set_slot jamais intégré");
    assert_eq!(
        bot.inventory.carried.as_ref().map(|s| (s.id, s.count)),
        Some((5, 3)),
        "le stack doit être sur le curseur"
    );

    server.await.unwrap();
}
