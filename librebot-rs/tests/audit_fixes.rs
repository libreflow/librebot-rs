//! Tests de régression des corrections de l'audit :
//! 1) multi_block_change (0x49) met à jour le monde + air_confirmed
//! 2) goto() sans chemin vide l'ancien chemin (pas de chemin périmé)
//! 3) declare_recipes : showNotification des recettes shaped est consommé
//!    (sinon les recettes suivantes sont illisibles)

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_string, write_varint};
use librebot_rs::recipes::item_id;
use librebot_rs::world::MIN_Y;

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

/// Une recette crafting_shaped (type 0) complète, avec showNotification.
fn shaped_recipe_entry(name: &str, w: i32, h: i32, result_id: i32, result_count: i32) -> Vec<u8> {
    let mut b = Vec::new();
    write_string(name, &mut b);
    write_varint(0, &mut b); // type 0 = crafting_shaped
    write_string("", &mut b); // group
    write_varint(0, &mut b); // category
    write_varint(w, &mut b); // gridWidth
    write_varint(h, &mut b); // gridHeight
    for _ in 0..(w * h) {
        write_varint(0, &mut b); // ingrédient vide (count 0)
    }
    b.extend_from_slice(&slot_body(result_id, result_count)); // result
    b.push(1); // showNotification = true
    b
}

/// Une recette crafting_shapeless (type 1) : group, category, n ingrédients, result.
fn shapeless_recipe_entry(name: &str) -> Vec<u8> {
    let mut b = Vec::new();
    write_string(name, &mut b);
    write_varint(1, &mut b); // type 1 = crafting_shapeless
    write_string("", &mut b); // group
    write_varint(0, &mut b); // category
    write_varint(0, &mut b); // 0 ingrédient
    b.extend_from_slice(&slot_body(0, 0)); // result vide
    b
}

fn declare_recipes_mixed_body() -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(2, &mut b);
    b.extend_from_slice(&shaped_recipe_entry(
        "minecraft:stick",
        1,
        2,
        item_id("stick"),
        4,
    ));
    b.extend_from_slice(&shapeless_recipe_entry("minecraft:oak_planks"));
    b
}

/// Corps d'un multi_block_change (0x49) pour le chunk (0,0), section y=0 :
/// record = (stateId << 12) | index, index = y*256 + z*16 + x (dans la section).
fn multi_block_change_body(records: &[(i32, i32, i32, i32)]) -> Vec<u8> {
    // (x, z, y, state) -> record ; chunkCoordinates = x<<42 | z<<20 | y(section)
    let mut b = Vec::new();
    // chunk (0,0), section y = MIN_Y/16 = -4 (codé sur 20 bits signés)
    let v: i64 = (0i64 << 42) | (0i64 << 20) | ((-4i64) & 0xF_FFFF);
    b.extend_from_slice(&v.to_be_bytes());
    write_varint(records.len() as i32, &mut b);
    for (x, z, y, state) in records {
        let idx = y * 256 + z * 16 + x;
        write_varint((state << 12) | idx, &mut b);
    }
    b
}

#[tokio::test]
async fn multi_block_change_met_a_jour_le_monde() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        // section (0,0) : (3,5,y=0) -> air (0), (2,7,y=0) -> stone (1)
        conn.send(0x49, &multi_block_change_body(&[(3, 5, 0, 0), (2, 7, 0, 1)]))
            .await
            .unwrap();
        conn.send(0x40, &position_body(0.5, -50.0, 0.5, 100)).await.unwrap();
        let _ = expect(&mut conn, 0x00).await; // teleport_confirm
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    bot.drain_packets().await; // traite le 0x49
    assert_eq!(
        bot.world.block_name(2, MIN_Y, 7),
        Some("stone"),
        "multi_block_change doit poser la stone en (2, MIN_Y, 7)"
    );
    assert!(
        bot.air_confirmed.contains(&(3, MIN_Y, 5)),
        "multi_block_change à air doit alimenter air_confirmed"
    );
    // le paquet position a déjà été consommé par drain_packets
    assert!(bot.pos.is_some(), "le téléport doit avoir été appliqué");
    server.await.unwrap();
}

#[tokio::test]
async fn goto_sans_chemin_ne_garde_pas_un_chemin_perime() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        conn.send(0x40, &position_body(0.5, -63.0, 0.5, 100)).await.unwrap();
        let _ = expect(&mut conn, 0x00).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Spawned));

    // Cible atteignable : chemin rempli
    assert!(bot.goto(5, MIN_Y + 1, 0).await.is_some());
    assert!(bot.path_len() > 0);
    // Cible hors des chunks chargés : aucun chemin -> l'ancien chemin doit être vidé
    assert!(bot.goto(10_000, 64, 10_000).await.is_none());
    assert_eq!(bot.path_len(), 0, "un goto sans chemin doit vider le chemin");
    server.await.unwrap();
}

#[tokio::test]
async fn declare_recipes_shaped_avec_show_notification() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x77, &declare_recipes_mixed_body()).await.unwrap();
        conn.send(0x40, &position_body(0.5, -63.0, 0.5, 100)).await.unwrap();
        let _ = expect(&mut conn, 0x00).await;
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    bot.drain_packets().await; // traite le declare_recipes
    assert_eq!(
        bot.recipes,
        vec!["minecraft:stick".to_string(), "minecraft:oak_planks".to_string()],
        "les 2 recettes doivent être lues (showNotification consommé après le shaped)"
    );
    server.await.unwrap();
}
