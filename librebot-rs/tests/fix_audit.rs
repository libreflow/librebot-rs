//! Tests de régression des 4 bugs de l'audit :
//! 1) craft : les ingrédients sont placés dans la grille par clics avant place_recipe
//! 2) anti-tunneling : chute de haut ne traverse pas le sol
//! 3) minage : FINISH_DIGGING renvoyé jusqu'à confirmation serveur ; temps h*5 sans outil
//! 4) inventaire : composants d'items usuels (varint/bool/nbt) lus sans désaligner

mod common;

use common::*;
use librebot_rs::bot::{Bot, Event};
use librebot_rs::protocol::{write_string, write_varint, Body, ReadOutcome};
use librebot_rs::recipes::item_id;
use librebot_rs::world::MIN_Y;
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

fn set_slot_body(window_id: u8, state_id: i32, slot: i16, id: i32, count: i32) -> Vec<u8> {
    let mut b = Vec::new();
    b.push(window_id);
    write_varint(state_id, &mut b);
    b.extend_from_slice(&slot.to_be_bytes());
    b.extend_from_slice(&slot_body(id, count));
    b
}

fn declare_recipes_body(names: &[&str]) -> Vec<u8> {
    let mut b = Vec::new();
    write_varint(names.len() as i32, &mut b);
    for name in names {
        write_varint(1, &mut b); // type shapeless (skip)
        b.extend_from_slice(&{
            let mut s = Vec::new();
            write_string(name, &mut s);
            s
        });
        // attende group + category + count + ingr + result selon skip_recipe_data type 1
        write_string("", &mut b);
        write_varint(0, &mut b);
        write_varint(0, &mut b); // 0 ingrédient
        b.extend_from_slice(&slot_body(0, 0)); // résultat vide
    }
    // declare_recipes lit d'abord le nom PUIS le type ; reconstruire correctement :
    let mut b = Vec::new();
    write_varint(names.len() as i32, &mut b);
    for name in names {
        write_string(name, &mut b);
        write_varint(1, &mut b);
        write_string("", &mut b);
        write_varint(0, &mut b);
        write_varint(0, &mut b);
        b.extend_from_slice(&slot_body(0, 0));
    }
    b
}

/// Paquet container_click lu côté serveur : (slot, button, mode)
fn parse_click(body: &[u8]) -> (i16, u8, i32) {
    let mut b = Body(body);
    let _wid = b.u8();
    let _state = b.varint();
    let slot = b.i16();
    let button = b.u8();
    let mode = b.varint();
    (slot, button, mode)
}

#[tokio::test]
async fn craft_place_les_ingredients_dans_la_grille() {
    let (listener, addr) = free_listener().await;
    let log = item_id("oak_log");
    let planks = item_id("oak_planks");

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x77, &declare_recipes_body(&["minecraft:oak_planks"]))
            .await
            .unwrap();
        conn.send(0x13, &window_items_body(1, &[(36, log, 1)]))
            .await
            .unwrap();

        // 1) clic gauche sur la source (slot 36) : ramasser le log
        let b = expect(&mut conn, 0x0e).await;
        let (slot, button, _) = parse_click(&b);
        assert_eq!((slot, button), (36, 0), "pickup attendu sur le slot du log");
        // 2) clic droit sur la cellule de grille 1 : poser 1 log
        let b = expect(&mut conn, 0x0e).await;
        let (slot, button, _) = parse_click(&b);
        assert_eq!((slot, button), (1, 1), "place-1 attendu sur la grille (slot 1, bouton droit)");
        // 3) place_recipe avec la bonne recette
        let b = expect(&mut conn, 0x22).await;
        let mut bb = Body(&b);
        let _wid = bb.u8();
        assert_eq!(bb.string(), "minecraft:oak_planks");
        // le serveur met le resultat sur le curseur
        conn.send(0x15, &set_slot_body(0, 2, -1, planks, 4))
            .await
            .unwrap();
        // 4) le bot prend le curseur dans un slot vide
        let b = expect(&mut conn, 0x0e).await;
        let (slot, button, _) = parse_click(&b);
        assert!(slot >= 9, "le curseur doit etre pose dans le stockage, slot={slot}");
        assert_eq!(button, 0);
        let placed = slot;
        conn.send(0x15, &set_slot_body(0, 3, placed, planks, 4))
            .await
            .unwrap();
        conn.send(0x15, &set_slot_body(0, 3, -1, 0, 0)).await.unwrap();
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    bot.drain_packets().await; // declare_recipes + window_items
    assert!(bot.inventory.count(log) == 1, "le mock doit donner 1 log");
    let ok = bot.craft("oak_planks").await;
    assert!(ok, "le craft doit reussir");
    assert_eq!(bot.inventory.count(planks), 4, "4 planks attendus");
    server.await.unwrap();
}

#[tokio::test]
async fn chute_de_haut_ne_traverse_pas_le_sol() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        // Chute de 164 blocs : à vitesse terminale le bot parcourt ~3,9 blocs/tick.
        conn.send(0x40, &position_body(0.5, 100.0, 0.5, 42))
            .await
            .unwrap();
        let confirm = expect(&mut conn, 0x00).await;
        assert_eq!(Body(&confirm).varint(), 42);
        // Lire les positions jusqu'au deadline ; le dernier y envoyé doit être le sommet du sol.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut last_y = 100.0f64;
        while Instant::now() < deadline {
            match timeout(Duration::from_secs(2), conn.read_packet()).await {
                Ok(Ok(ReadOutcome::Packet(0x1b, body))) => {
                    let mut b = Body(&body);
                    let _x = b.f64();
                    last_y = b.f64();
                    let _z = b.f64();
                }
                Ok(Ok(_)) => continue,
                _ => break,
            }
            if (last_y - ((MIN_Y + 1) as f64)).abs() < 0.01 {
                break;
            }
        }
        assert!(
            (last_y - ((MIN_Y + 1) as f64)).abs() < 0.01,
            "le bot a traverse le sol : dernier y={last_y}, attendu {}",
            MIN_Y + 1
        );
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Spawned));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !bot.on_ground && Instant::now() < deadline {
        bot.drain_packets().await;
        bot.tick(0.05).await.unwrap();
    }
    assert!(bot.on_ground, "le bot doit avoir atterri");
    assert!(
        (bot.pos.unwrap()[1] - ((MIN_Y + 1) as f64)).abs() < 0.01,
        "pos y = {:?}, attendu {}",
        bot.pos.unwrap()[1],
        MIN_Y + 1
    );
    server.await.unwrap();
}

#[tokio::test]
async fn minage_renvoie_finish_jusqua_confirmation() {
    let (listener, addr) = free_listener().await;

    let server = tokio::spawn(async move {
        let mut conn = prologue(listener).await;
        conn.send(0x2b, &login_body(7)).await.unwrap();
        conn.send(0x27, &floor_chunk_body(0, 0, None)).await.unwrap();
        conn.send(0x40, &position_body(0.5, (MIN_Y + 2) as f64, 0.5, 9))
            .await
            .unwrap();
        expect(&mut conn, 0x00).await;

        let mut finishes = 0u32;
        let deadline = Instant::now() + Duration::from_secs(15);
        while finishes < 2 && Instant::now() < deadline {
            match timeout(Duration::from_secs(10), conn.read_packet()).await {
                Ok(Ok(ReadOutcome::Packet(0x24, body))) => {
                    let mut b = Body(&body);
                    let status = b.varint();
                    if status == 2 {
                        finishes += 1;
                        if finishes >= 2 {
                            // confirmer la casse seulement au 2e FINISH
                            let mut bc = Vec::new();
                            librebot_rs::protocol::write_position(0, MIN_Y, 0, &mut bc);
                            write_varint(0, &mut bc); // air
                            conn.send(0x09, &bc).await.unwrap();
                            // garder la socket ouverte pendant que le bot traite la confirmation
                            while let Ok(Ok(ReadOutcome::Packet(..))) =
                                timeout(Duration::from_secs(2), conn.read_packet()).await
                            {}
                        }
                    }
                }
                Ok(Ok(_)) => continue,
                _ => break,
            }
        }
        assert!(finishes >= 2, "le bot doit renvoyer FINISH (reçu {finishes})");
    });

    let mut bot = Bot::connect(&addr.ip().to_string(), addr.port(), "LibreBot")
        .await
        .unwrap();
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Connected));
    assert!(matches!(bot.play_turn().await.unwrap(), Event::Spawned));
    bot.drain_packets().await;
    let t = bot.dig(0, MIN_Y, 0).await.expect("dig stone");
    // à mains nues sur la stone (outil requis) : h*5 = 7.5 s, pas h*1.5 = 2.25 s
    assert!(t > 5.0, "temps de minage mains nues attendu ~7.5s, obtenu {t}");
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut n = 0u32;
    while !bot.mining_done() && Instant::now() < deadline {
        // drain_packets bloque 50 ms quand rien n'est en attente : ne l'appeler
        // qu'à intervalles réguliers pour garder une cadence de ticks rapide
        if n % 10 == 0 {
            bot.drain_packets().await;
        }
        bot.tick(0.05).await.unwrap();
        n += 1;
    }
    bot.drain_packets().await;
    assert!(bot.mining_done(), "minage jamais terminé");
    assert!(bot.air_confirmed.contains(&(0, MIN_Y, 0)), "casse non confirmée par le serveur");
    server.await.unwrap();
}

#[test]
fn break_time_selon_outil() {
    use librebot_rs::blocks_gen::break_time_secs_with;
    let stone = librebot_rs::blocks_gen::block_state_by_name("stone").unwrap();
    let with_pickaxe = break_time_secs_with(stone, true);
    let bare_hands = break_time_secs_with(stone, false);
    assert!((with_pickaxe - 2.25).abs() < 0.01, "avec pioche: {with_pickaxe}");
    assert!((bare_hands - 7.5).abs() < 0.01, "mains nues: {bare_hands}");
}

#[test]
fn slot_avec_composants_usuels_se_lit() {
    use librebot_rs::inventory::Inventory;
    use librebot_rs::protocol::{write_varint, Body};
    // slot : count=1, id=eponge, added=3 composants : max_stack_size(1,varint), damage(3,varint), custom_name(5,NBT)
    let mut b = Vec::new();
    write_varint(1, &mut b); // count
    write_varint(item_id("sponge"), &mut b); // id
    write_varint(3, &mut b); // added components
    write_varint(1, &mut b); // max_stack_size
    write_varint(16, &mut b);
    write_varint(3, &mut b); // damage
    write_varint(3, &mut b);
    write_varint(5, &mut b); // custom_name (NBT chat)
    b.extend_from_slice(&[0x0a, 0x00]); // compound vide (tag + TAG_End)
    write_varint(0, &mut b); // removed
    let mut bb = Body(&b);
    let slot = librebot_rs::inventory::read_slot(&mut bb);
    assert!(slot.is_some(), "le slot avec composants usuels doit se lire");
    let s = slot.unwrap().unwrap();
    assert_eq!(s.id, item_id("sponge"));
    assert_eq!(s.count, 1);
    assert!(bb.rest().is_empty(), "tout le slot doit etre consommé, reste {:?}", bb.rest());

    // fenêtre complète avec ce slot : aucun abort
    let mut w = Vec::new();
    w.push(0u8);
    write_varint(1, &mut w);
    write_varint(46, &mut w);
    for i in 0..46 {
        if i == 9 {
            w.extend_from_slice(&b);
        } else {
            write_varint(0, &mut w);
        }
    }
    write_varint(0, &mut w); // carried
    let mut inv = Inventory::new();
    inv.apply_window_items(&w).expect("window_items doit passer malgré les composants");
    assert_eq!(inv.count(item_id("sponge")), 1);
}
