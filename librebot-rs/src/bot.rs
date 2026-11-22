//! Session client : handshake, login offline, configuration, boucle de jeu.
//! IDs de paquets du protocole 1.21.1 (source : minecraft-data).

use crate::inventory::{window_click_body, Inventory};
use crate::nbt;
use crate::protocol::Conn;
use crate::protocol::ReadOutcome::{Eof, Packet};
use crate::protocol::{offline_uuid, write_string, Body, MC_VERSION, PROTOCOL_VERSION};
use crate::entities::Entities;
use crate::path::find_path;
use crate::world::World;
use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;

pub(crate) mod ids {
    pub mod login_c {
        pub const DISCONNECT: i32 = 0x00;
        pub const SUCCESS: i32 = 0x02;
        pub const COMPRESS: i32 = 0x03;
    }
    pub mod login_s {
        pub const START: i32 = 0x00;
        pub const ACKNOWLEDGED: i32 = 0x03;
    }
    pub mod config_c {
        pub const DISCONNECT: i32 = 0x02;
        pub const FINISH: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const PING: i32 = 0x05;
        pub const SELECT_KNOWN_PACKS: i32 = 0x0e;
    }
    pub mod config_s {
        pub const SETTINGS: i32 = 0x00;
        pub const FINISH: i32 = 0x03;
        pub const KEEP_ALIVE: i32 = 0x04;
        pub const PONG: i32 = 0x05;
        pub const SELECT_KNOWN_PACKS: i32 = 0x07;
    }
    #[allow(dead_code)]
    pub mod play_c {
        // IDs protocole 767 (Minecraft 1.21.1) — source : minecraft-data
        pub const WINDOW_ITEMS: i32 = 0x13;
        pub const BLOCK_CHANGE: i32 = 0x09;
        pub const SET_SLOT: i32 = 0x15;
        pub const HELD_ITEM_SLOT_C: i32 = 0x53;
        pub const UNLOAD_CHUNK: i32 = 0x21;
        pub const KICK_DISCONNECT: i32 = 0x1d;
        pub const MAP_CHUNK: i32 = 0x27;
        pub const KEEP_ALIVE: i32 = 0x26;
        pub const LOGIN: i32 = 0x2b;
        pub const POSITION: i32 = 0x40;
        pub const SYSTEM_CHAT: i32 = 0x6c;
        pub const PLAYER_CHAT: i32 = 0x39;
        pub const PLAYER_INFO: i32 = 0x3e;
        pub const SET_HEALTH: i32 = 0x5d;
        pub const EXPERIENCE: i32 = 0x5c;
        pub const GAME_EVENT: i32 = 0x22;
        pub const RESPAWN: i32 = 0x47;
        pub const DECLARE_RECIPES: i32 = 0x77;
        pub const OPEN_WINDOW: i32 = 0x33;
        pub const CLOSE_WINDOW: i32 = 0x12;
        pub const CHUNK_BATCH_START: i32 = 0x0d;
        pub const SPAWN_ENTITY: i32 = 0x01;
        pub const REL_ENTITY_MOVE: i32 = 0x2e;
        pub const ENTITY_MOVE_LOOK: i32 = 0x2f;
        pub const ENTITY_TELEPORT: i32 = 0x70;
        pub const ENTITY_DESTROY: i32 = 0x42;
        pub const CHUNK_BATCH_FINISHED: i32 = 0x0c;
        pub const MULTI_BLOCK_CHANGE: i32 = 0x49;
        pub const SET_DEFAULT_SPAWN: i32 = 0x56;
        pub const UPDATE_VIEW_POSITION: i32 = 0x54;
        pub const UPDATE_VIEW_DISTANCE: i32 = 0x55;
        pub const SET_TIME: i32 = 0x64;
        pub const SET_ENTITY_DATA: i32 = 0x58;
        /// Objet ramassé : le serveur émet « collect » (pas de destroy) — purger
        /// l'entité item, sinon le bot re-cible en boucle un drop déjà ramassé.
        pub const COLLECT: i32 = 0x6f;
    }
    #[allow(dead_code)]
    pub mod play_s {
        // IDs serverbound 767 — source : minecraft-data
        pub const TELEPORT_CONFIRM: i32 = 0x00;
        pub const CHAT_COMMAND: i32 = 0x04;
        pub const CHAT_MESSAGE: i32 = 0x06;
        pub const CHUNK_BATCH_RECEIVED: i32 = 0x08;
        pub const CLIENT_COMMAND: i32 = 0x09;
        pub const SETTINGS: i32 = 0x0a;
        pub const WINDOW_CLICK: i32 = 0x0e;
        pub const CONTAINER_CLOSE: i32 = 0x0f;
        pub const FLYING: i32 = 0x1d;
        pub const POSITION_LOOK: i32 = 0x1b;
        pub const POSITION: i32 = 0x1a;
        pub const CRAFT_RECIPE: i32 = 0x22;
        pub const BLOCK_DIG: i32 = 0x24;
        pub const HELD_ITEM_SLOT: i32 = 0x2f;
        pub const BLOCK_PLACE: i32 = 0x38;
pub const USE_ITEM: i32 = 0x39;
                pub const USE_ENTITY_ID: i32 = 0x16;
                pub const SWING_ID: i32 = 0x36;
        pub const SWING: i32 = 0x36;
        pub const KEEP_ALIVE: i32 = 0x18;
    }
}

pub const GRAVITY: f64 = 32.0;
pub const TERMINAL_VELOCITY: f64 = -78.4;
pub const WALK_SPEED: f64 = 4.3;
/// Apogée ≈ 1,25 bloc : de quoi franchir un muret d'un bloc (sans drag vanilla).
pub const JUMP_VELOCITY: f64 = 8.95;

#[derive(Debug)]
pub enum Event {
    Connected,
    Spawned,
    Disconnected(String),
    Chat(String),
}

pub struct Bot {
    pub(crate) conn: Conn,
    pub username: String,
    pub world: World,
    pub entity_id: Option<i32>,
    pub dimension: Option<String>,
    /// Position des pieds (feet position) du bot.
    pub pos: Option<[f64; 3]>,
    pub yaw: f32,
    pub pitch: f32,
    vy: f64,
    pub inventory: Inventory,
    pub on_ground: bool,
    /// Santé : mort détectée (update_health <= 0) en attente de respawn.
    pub dead: bool,
    /// Waypoints (x, z) au centre des blocs — chemin en cours.
    path: VecDeque<(f64, f64)>,
    /// Minage en cours : (x, y, z, secondes restantes, séquence, nom du bloc).
    mining: Option<(i32, i32, i32, f64, i32, &'static str)>,
    pub(crate) sequence: i32,
    /// Recettes déclarées par le serveur (declare_recipes) : index = recipeId.
    pub recipes: Vec<String>,
    /// Fenêtre ouverte (table de craft) : windowId, ou None si inventaire seul.
    pub open_window_id: Option<u8>,
    /// Compteur de chunks dans le batch en cours.
    pub(crate) chunks_in_batch: i32,
    /// Blocs hors de portée/inaccessibles récemment : on ne les re-cible pas.
    pub failed_targets: Vec<(i32, i32, i32)>,
    /// Distance de vue déclarée au serveur (chunks).
    pub view_distance: i8,
    /// Le serveur s'est déconnecté (vu pendant un drain) : arrêter la mission.
    pub disconnected: bool,
    /// Santé (0-20) et niveau de faim (0-20).
    pub health: f32,
    pub food: i32,
    /// Entités connues autour du bot (animaux, items jetés, mobs).
    pub entities: Entities,
    /// Heure du monde (ticks de journée, 0-24000) — pour savoir quand dormir.
    pub day_time: i64,
    /// Campement du bot, s'il en a établi un.
    pub camp: Option<crate::camp::Camp>,
    /// Anti-spam chat : dernier instant d'envoi + dernier texte dit.
    pub(crate) last_chat_at: Option<std::time::Instant>,
    pub(crate) last_chat_text: Option<String>,
    /// Envois différés : cancel-safe (aucun octet perdu si le future est annulé).
    pub(crate) pending_send: bool,
    pub(crate) pending_respawn: bool,
    pub(crate) pending_dig_finish: Option<(i32, i32, i32, i32, &'static str)>,
    /// Anti-blocage : ticks de sauts stériles + obstacle devant à creuser.
    pub(crate) blocked_ticks: u32,
    pub(crate) blocked_front: Option<(i32, i32, i32)>,
    /// Compteurs de diagnostic (rapport diag régulier en console).
    pub stats_msgs_recus: u64,
    pub stats_batches: u64,
    pub stats_positions: u64,
}

impl Bot {
    pub async fn connect(host: &str, port: u16, username: &str) -> std::io::Result<Self> {
        Self::connect_with_view(host, port, username, 8).await
    }

    /// Comme `connect` avec une distance de vue personnalisée (2-32).
    /// Sur un serveur lent, une petite distance accélère le chargement du monde.
    pub async fn connect_with_view(
        host: &str,
        port: u16,
        username: &str,
        view_distance: i8,
    ) -> std::io::Result<Self> {
        let stream = TcpStream::connect((host, port)).await?;
        let mut conn = Conn::new(stream);

        let mut hs = Vec::new();
        crate::protocol::write_varint(PROTOCOL_VERSION, &mut hs);
        write_string(host, &mut hs);
        hs.extend_from_slice(&port.to_be_bytes());
        crate::protocol::write_varint(2, &mut hs);
        conn.send(0x00, &hs).await?;

        let mut ls = Vec::new();
        write_string(username, &mut ls);
        // Protocole 775 (26.1.1) : Login Start = Name (String) + UUID (16 bytes), sans boolean has_uuid
        ls.extend_from_slice(&offline_uuid(username));
        conn.send(ids::login_s::START, &ls).await?;

        loop {
            let pkt = conn.read_packet().await?;
            if debug_enabled() {
                if let Packet(id, body) = &pkt {
                    eprintln!("[login] 0x{:02x} ({} o)", id, body.len());
                }
            }
            match pkt {
                Eof => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "connexion fermée pendant le login",
                    ))
                }
                Packet(ids::login_c::COMPRESS, body) => {
                    let threshold = Body(&body).varint();
                    conn.set_compression(threshold);
                }
                Packet(ids::login_c::DISCONNECT, body) => {
                    let reason = chat_reason(&body);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        format!("refusé par le serveur : {}", reason),
                    ));
                }
                Packet(ids::login_c::SUCCESS, body) => {
                    let mut b = Body(&body);
                    let _uuid = b.take(16); // UUID = 16 octets bruts
                    let name = b.string();
                    conn.send(ids::login_s::ACKNOWLEDGED, &[]).await?;
                    let mut bot = Bot {
                        conn,
                        username: name,
                        world: World::new(),
                        entity_id: None,
                        dimension: None,
                        pos: None,
                        yaw: 0.0,
                        pitch: 0.0,
                        vy: 0.0,
                        inventory: Inventory::new(),
                        on_ground: false,
                        dead: false,
                        path: VecDeque::new(),
                        mining: None,
                        sequence: 0,
                        recipes: Vec::new(),
                        open_window_id: None,
                        chunks_in_batch: 0,
                        failed_targets: Vec::new(),
                        view_distance: 8,
                        disconnected: false,
                        health: 20.0,
                        food: 20,
                        entities: Entities::new(),
                        day_time: 0,
                        camp: None,
                        last_chat_at: None,
                        last_chat_text: None,
                        pending_send: false,
                        pending_respawn: false,
                        pending_dig_finish: None,
                        blocked_ticks: 0,
                        blocked_front: None,
                        stats_msgs_recus: 0,
                        stats_batches: 0,
                        stats_positions: 0,
                    };
                    bot.view_distance = view_distance.clamp(2, 32);
                    bot.configuration().await?;
                    return Ok(bot);
                }
                Packet(_, _) => {}
            }
        }
    }

    async fn configuration(&mut self) -> std::io::Result<()> {
        loop {
            let pkt = self.conn.read_packet().await?;
            if debug_enabled() {
                if let Packet(id, body) = &pkt {
                    eprintln!("[config] 0x{:02x} ({} o) {:02x?}", id, body.len(), &body[..body.len().min(24)]);
                }
            }
            match pkt {
                Eof => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        "connexion fermée pendant la configuration",
                    ))
                }
                Packet(ids::config_c::KEEP_ALIVE, body) => {
                    let id = Body(&body).i64();
                    let mut out = Vec::new();
                    out.extend_from_slice(&id.to_be_bytes());
                    self.conn.send(ids::config_s::KEEP_ALIVE, &out).await?;
                }
                Packet(ids::config_c::PING, body) => {
                    let id = Body(&body).i32();
                    let mut out = Vec::new();
                    out.extend_from_slice(&id.to_be_bytes());
                    self.conn.send(ids::config_s::PONG, &out).await?;
                }
                Packet(ids::config_c::SELECT_KNOWN_PACKS, _) => {
                    let mut out = Vec::new();
                    crate::protocol::write_varint(1, &mut out);
                    write_string("minecraft", &mut out);
                    write_string("core", &mut out);
                    write_string(MC_VERSION, &mut out);
                    self.conn
                        .send(ids::config_s::SELECT_KNOWN_PACKS, &out)
                        .await?;
                    let mut settings = Vec::new();
                    write_string("fr_FR", &mut settings);   // locale
                    settings.push(self.view_distance as u8);   // viewDistance (i8)
                    crate::protocol::write_varint(0, &mut settings); // chatFlags
                    settings.push(1);                        // chatColors
                    settings.push(0x7f);                     // skinParts (u8)
                    crate::protocol::write_varint(1, &mut settings); // mainHand (1=right)
                    settings.push(0);                        // enableTextFiltering
                    settings.push(0);                        // enableServerListing
                    // En 767: pas de particleStatus
                    if debug_enabled() {
                        eprintln!("[envoi config settings] {:02x?}", settings);
                    }
                    self.conn.send(ids::config_s::SETTINGS, &settings).await?;
                }
                Packet(ids::config_c::FINISH, _) => {
                    self.conn.send(ids::config_s::FINISH, &[]).await?;
                    return Ok(());
                }
                Packet(ids::config_c::DISCONNECT, body) => {
                    let reason = chat_reason(&body);
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        format!("déconnecté (configuration) : {}", reason),
                    ));
                }
                Packet(_, _) => {}
            }
        }
    }

    pub async fn play_turn(&mut self) -> std::io::Result<Event> {
        loop {
            let pkt = self.conn.read_packet().await?;
            self.stats_msgs_recus += 1;
            let debug = debug_enabled();
            if debug {
                if let Packet(id, body) = &pkt {
                    eprintln!(
                        "[pkt] 0x{:02x} ({} o) {:02x?}",
                        id,
                        body.len(),
                        &body[..body.len().min(24)]
                    );
                }
            }
            match pkt {
                Eof => return Ok(Event::Disconnected("connexion fermée".into())),
                Packet(ids::play_c::KEEP_ALIVE, body) => {
                    let id = Body(&body).i64();
                    let mut out = Vec::new();
                    out.extend_from_slice(&id.to_be_bytes());
                    self.conn.send(ids::play_s::KEEP_ALIVE, &out).await?;
                }
                Packet(ids::play_c::LOGIN, body) => {
                    self.apply_login(&body);
                    // Signal « client chargé » — en 767, le client vanilla renvoie
                    // client_information en phase PLAY juste après le paquet login.
                    // Sans lui, le serveur garde le joueur « non chargé » : invisible
                    // pour les autres et plus aucun lot de chunks n'est envoyé.
                    let mut settings = Vec::new();
                    write_string("fr_FR", &mut settings);
                    settings.push(self.view_distance as u8);
                    crate::protocol::write_varint(0, &mut settings);
                    settings.push(1); // chatColors
                    settings.push(0x7f); // skinParts
                    crate::protocol::write_varint(1, &mut settings); // mainHand
                    settings.push(0); // enableTextFiltering
                    settings.push(0); // enableServerListing
                    if debug_enabled() {
                        eprintln!("[envoi play settings] {:02x?}", settings);
                    }
                    self.conn.send(ids::play_s::SETTINGS, &settings).await?;
                    return Ok(Event::Connected);
                }
                // chunk_batch_start (0x0c en 775) : début d'un batch de chunks
                Packet(ids::play_c::CHUNK_BATCH_START, _) => {
                    self.chunks_in_batch = 0;
                }
                // chunk_batch_finished : répondre chunk_batch_received (float: desired chunks/tick en 767)
                Packet(ids::play_c::CHUNK_BATCH_FINISHED, _) => {
                    let out = 64.0f32.to_be_bytes().to_vec();
                    self.conn.send(ids::play_s::CHUNK_BATCH_RECEIVED, &out).await.ok();
                    self.chunks_in_batch = 0;
                    self.stats_batches += 1;
                }
                Packet(ids::play_c::MAP_CHUNK, body) => {
                    self.chunks_in_batch += 1;
                    match self.world.apply_map_chunk(&body) {
                        Ok(()) => {
                            if debug {
                                eprintln!("[chunk] OK ({} octets, {} chunks total)", body.len(), self.world.chunk_count());
                            }
                        }
                        Err(e) => eprintln!("[chunk] ERREUR ({} octets): {} — hex: {:02x?}", body.len(), e, &body[..body.len().min(60)]),
                    }
                }
                Packet(ids::play_c::UNLOAD_CHUNK, body) => {
                    self.world.apply_unload_chunk(&body);
                }
                Packet(ids::play_c::BLOCK_CHANGE, body) => {
                    let mut b = Body(&body);
                    let (x, y, z) = b.position();
                    let state = b.varint() as u32;
                    self.world.set_block_state(x, y, z, state);
                }
                Packet(ids::play_c::WINDOW_ITEMS, body) => {
                    if let Err(e) = self.inventory.apply_window_items(&body) {
                        eprintln!("inventaire partiellement illisible : {:?}", e);
                    }
                }
                Packet(ids::play_c::SET_SLOT, body) => {
                    if let Err(e) = self.inventory.apply_set_slot(&body) {
                        eprintln!("set_slot illisible : {:?}", e);
                    }
                }
                Packet(ids::play_c::HELD_ITEM_SLOT_C, _) => {}
                Packet(ids::play_c::SET_TIME, body) => {
                    let mut b = Body(&body);
                    let _age = b.i64();
                    self.day_time = b.i64();
                }
                Packet(ids::play_c::SET_HEALTH, body) => {
                    let mut b = Body(&body);
                    let hp = b.f32();
                    self.food = b.varint();
                    let _saturation = b.f32();
                    self.health = hp;
                    if hp <= 0.0 {
                        self.dead = true;
                        self.path.clear();
                        self.mining = None;
                        println!("le bot est mort, respawn en cours...");
                    }
                }
                Packet(ids::play_c::POSITION, body) => {
                    let teleport_id = self.apply_position(&body);
                    if let Some(id) = teleport_id {
                        let mut out = Vec::new();
                        crate::protocol::write_varint(id, &mut out);
                        self.conn.send(ids::play_s::TELEPORT_CONFIRM, &out).await?;
                    }
                    // player_loaded : signale que le client est prêt
                    // pas de player_loaded en 767
                    // pas de chunk_batch_received initial non plus (le serveur envoie les chunks directement)
                    return Ok(Event::Spawned);
                }
                Packet(ids::play_c::SYSTEM_CHAT, body) => {
                    let mut rest: &[u8] = &body;
                    let comp = nbt::parse(&mut rest);
                    if let Some(comp) = comp {
                        let t = nbt::text(&comp);
                        if !t.is_empty() {
                            return Ok(Event::Chat(t));
                        }
                    }
                }
                Packet(ids::play_c::PLAYER_CHAT, body) => {
                    if let Some(text) = parse_player_chat(&body) {
                        return Ok(Event::Chat(text));
                    }
                }
                Packet(ids::play_c::KICK_DISCONNECT, body) => {
                    let reason = chat_reason(&body);
                    return Ok(Event::Disconnected(reason));
                }
                // declare_recipes (0x77 en 767) : { count, [ name, type, data... ] }
                Packet(ids::play_c::RESPAWN, _) => {
                    // Nouveau monde (mort ou portail) : purger les données locales,
                    // le serveur renverra chunks et entités.
                    self.entities.alive.clear();
                    self.world = crate::world::World::new();
                    self.path.clear();
                    self.mining = None;
                    self.failed_targets.clear();
                }
                Packet(ids::play_c::SPAWN_ENTITY, body) => {
                    let mut b = Body(&body);
                    let id = b.varint();
                    let _uuid = b.take(16);
                    let etype = b.varint();
                    let x = b.f64();
                    let y = b.f64();
                    let z = b.f64();
                    self.entities.spawn(id, etype, x, y, z);
                }
                Packet(
                    ids::play_c::REL_ENTITY_MOVE | ids::play_c::ENTITY_MOVE_LOOK,
                    body,
                ) => {
                    let mut b = Body(&body);
                    let id = b.varint();
                    let dx = b.i16() as f64 / 4096.0;
                    let dy = b.i16() as f64 / 4096.0;
                    let dz = b.i16() as f64 / 4096.0;
                    self.entities.translate(id, dx, dy, dz);
                }
                Packet(ids::play_c::ENTITY_TELEPORT, body) => {
                    let mut b = Body(&body);
                    let id = b.varint();
                    let x = b.f64();
                    let y = b.f64();
                    let z = b.f64();
                    self.entities.teleport(id, x, y, z);
                }
                Packet(ids::play_c::COLLECT, body) => {
                    let mut b = Body(&body);
                    let collecte = b.varint();
                    let _collecteur = b.varint();
                    self.entities.remove(&[collecte]);
                }
                Packet(ids::play_c::ENTITY_DESTROY, body) => {
                    let mut b = Body(&body);
                    let n = b.varint().max(0) as usize;
                    let mut ids = Vec::with_capacity(n);
                    for _ in 0..n {
                        ids.push(b.varint());
                    }
                    self.entities.remove(&ids);
                }
                Packet(ids::play_c::DECLARE_RECIPES, body) => {
                    self.declare_recipes(&body);
                }
                // open_window (0x33 en 767) : windowId (varint), type (varint), titre (NBT)
                Packet(ids::play_c::OPEN_WINDOW, body) => {
                    let mut b = Body(&body);
                    let window_id = b.varint();
                    let _window_type = b.varint();
                    let _ = crate::nbt::parse(&mut b.0);
                    self.open_window_id = u8::try_from(window_id).ok();
                }
                // close_window (0x12 en 767)
                Packet(ids::play_c::CLOSE_WINDOW, body) => {
                    let mut b = Body(&body);
                    let _window_id = b.u8();
                    self.open_window_id = None;
                }
                Packet(_, _) => {}
            }
        }
    }

    /// declare_recipes (0x77) : compte, puis par recette { nom, type, données typées }.
    fn declare_recipes(&mut self, body: &[u8]) {
        let mut b = Body(body);
        let n = b.varint().max(0) as usize;
        for _ in 0..n {
            let name = b.string();
            let rtype = b.varint();
            if !skip_recipe_data(&mut b, rtype) {
                eprintln!("[recipes] échec type {} à {}", rtype, name);
                self.recipes.push(name);
                break;
            }
            self.recipes.push(name);
        }
        if debug_enabled() {
            eprintln!("[recipes] {} recettes déclarées", self.recipes.len());
        }
    }

    /// Paquet `login` (0x2b) : entityId, dimension, etc.
    fn apply_login(&mut self, body: &[u8]) {
        let mut b = Body(body);
        self.entity_id = Some(b.i32());
        let _hardcore = b.bool();
        let n_worlds = b.varint() as usize;
        for _ in 0..n_worlds {
            let _ = b.string();
        }
        let _max_players = b.varint();
        let _view = b.varint();
        let _sim = b.varint();
        let _reduced_debug = b.bool();
        let _respawn_screen = b.bool();
        let _limited_crafting = b.bool();
        let _world_id = b.varint();
        if let Some(dim) = Self::parse_world(&mut b) {
            self.dimension = Some(dim);
        }
        let _secure_chat = b.bool();
    }

    /// Partie `SpawnInfo` du login : renvoie le nom de la dimension.
    fn parse_world(b: &mut Body) -> Option<String> {
        let dimension = b.string();
        b.take(8);
        let _gamemode = b.u8();
        let _previous = b.u8();
        let _debug = b.bool();
        let _flat = b.bool();
        if b.bool() {
            let _ = b.string();
            b.take(8);
        }
        let _portal_cooldown = b.varint();
        Some(dimension)
    }

    /// Paquet `position` (0x40) : renvoie l'ID de téléport à confirmer.
    fn apply_position(&mut self, body: &[u8]) -> Option<i32> {
        let mut b = Body(body);
        let x = b.f64();
        let y = b.f64();
        let z = b.f64();
        let yaw = b.f32();
        let pitch = b.f32();
        let flags = b.u8();
        let teleport_id = b.varint();
        let cur = self.pos.unwrap_or([0.0, 0.0, 0.0]);
        self.pos = Some([
            if flags & 0x01 != 0 { cur[0] + x } else { x },
            if flags & 0x02 != 0 { cur[1] + y } else { y },
            if flags & 0x04 != 0 { cur[2] + z } else { z },
        ]);
        self.yaw = if flags & 0x08 != 0 { self.yaw + yaw } else { yaw };
        self.pitch = if flags & 0x10 != 0 {
            self.pitch + pitch
        } else {
            pitch
        };
        self.vy = 0.0;
        Some(teleport_id)
    }

    /// A* jusq'à un bloc cible ; le chemin est suivi dans `tick`.
    pub async fn goto(&mut self, x: i32, y: i32, z: i32) -> Option<usize> {
        let p = self.pos?;
        let start = (p[0].floor() as i32, p[1].floor() as i32, p[2].floor() as i32);
        let goal = (x, y, z);
        let path = find_path(&self.world, start, goal, crate::path::MAX_EXPANSIONS)?;
        self.path = path
            .into_iter()
            .map(|(px, _py, pz)| (px as f64 + 0.5, pz as f64 + 0.5))
            .collect();
        Some(self.path.len())
    }

    pub fn path_len(&self) -> usize {
        self.path.len()
    }

    /// Commence à miner un bloc (durée basée sur la dureté, mains nues).
    pub async fn dig(&mut self, x: i32, y: i32, z: i32) -> Option<f64> {
        let state = self.world.block_state(x, y, z)?;
        let face: i8 = 0;
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        crate::protocol::write_varint(0, &mut b);
        crate::protocol::write_position(x, y, z, &mut b);
        b.push(face as u8);
        crate::protocol::write_varint(seq, &mut b);
        self.conn.send(ids::play_s::BLOCK_DIG, &b).await.ok()?;
        let total = crate::blocks_gen::break_time_secs(state);
        let name = crate::blocks_gen::block_name(state);
        self.mining = Some((x, y, z, total, seq, name));
        Some(total)
    }

    /// Pose le bloc de la main sur la face `face` (0=bas..5=haut) du bloc support.
    pub async fn place(&mut self, x: i32, y: i32, z: i32, face: i32) -> std::io::Result<()> {
        let (_dx, _dy, _dz) = match face {
            0 => (0, -1, 0),
            1 => (0, 1, 0),
            2 => (0, 0, -1),
            3 => (0, 0, 1),
            4 => (-1, 0, 0),
            5 => (1, 0, 0),
            _ => (0, 1, 0),
        };
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        crate::protocol::write_varint(0, &mut b);
        crate::protocol::write_position(x, y, z, &mut b);
        crate::protocol::write_varint(face, &mut b);
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.extend_from_slice(&0.5f32.to_be_bytes());
        b.push(0);
        crate::protocol::write_varint(seq, &mut b);
        self.conn.send(ids::play_s::BLOCK_PLACE, &b).await
    }

    /// Sélectionne un slot de la hotbar (0-8) comme item en main.
    pub async fn select_hotbar(&mut self, hotbar_index: usize) -> std::io::Result<()> {
        let mut b = Vec::new();
        b.extend_from_slice(&(hotbar_index as i16).to_be_bytes());
        self.conn.send(ids::play_s::HELD_ITEM_SLOT, &b).await
    }

    /// Clic gauche (ramasser/poser un stack) sur un slot de la fenêtre 0.
    /// Retourne l'état d'inventaire utilisé.
    pub async fn click_slot(&mut self, slot: i16) -> std::io::Result<i32> {
        let state = self.inventory.state_id;
        let body = window_click_body(0, state, slot, 0, 0);
        self.conn.send(ids::play_s::WINDOW_CLICK, &body).await?;
        Ok(state)
    }

    /// Le minage en cours est-il terminé ?
    pub fn mining_done(&self) -> bool {
        self.mining.is_none()
    }

    /// Lit et traite les paquets en attente sans bloquer longtemps (drain).
    /// À appeler pendant les actions longues pour garder le monde à jour.
    /// Borné à ~500 ms réelles ; s'arrête dès que le serveur se déconnecte.
    /// Percée : si la physique a signalé un obstacle devant (sautes stériles),
    /// lancer le minage des blocs pieds/tête devant. Le chrono de minage
    /// avance dans physics_tick et flush_pending envoie la fin : rien à attendre ici.
    pub async fn dig_through_blockade(&mut self) -> bool {
        let Some((x, y, z)) = self.blocked_front.take() else {
            return false;
        };
        self.blocked_ticks = 0;
        let mut lance = false;
        if self.world.is_solid(x, y, z) && self.dig(x, y, z).await.is_some() {
            lance = true;
        }
        if self.world.is_solid(x, y + 1, z) && self.dig(x, y + 1, z).await.is_some() {
            lance = true;
        }
        lance
    }

    pub async fn drain_packets(&mut self) {
        if self.blocked_front.is_some() {
            self.dig_through_blockade().await;
        }
        let fin = std::time::Instant::now() + std::time::Duration::from_millis(500);
        for _ in 0..300 {
            if std::time::Instant::now() >= fin {
                break;
            }
            match tokio::time::timeout(std::time::Duration::from_millis(20), self.play_turn()).await {
                Ok(Ok(Event::Disconnected(_))) => {
                    self.disconnected = true;
                    break;
                }
                Ok(Ok(Event::Chat(m))) => {
                    if debug_enabled() {
                        eprintln!("[chat/drain] {}", m);
                    }
                    continue;
                }
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break, // timeout = plus rien en attente
            }
        }
    }

    pub async fn tick(&mut self, dt: f64) -> std::io::Result<()> {
        self.physics_tick(dt);
        self.flush_pending().await
    }

    /// Mise à jour physique sans émission réseau : cancel-safe, utilisable dans tokio::select!.
    pub fn physics_tick(&mut self, dt: f64) {
        if self.dead {
            self.pending_respawn = true;
        }
        let Some(p) = self.pos else { return };
        let mut pos = p;
        let mut moved = false;

        if let Some(&(tx, tz)) = self.path.front() {
            let dx = tx - pos[0];
            let dz = tz - pos[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < 0.25 {
                self.path.pop_front();
                self.blocked_ticks = 0;
                self.blocked_front = None;
            } else {
                let step = (WALK_SPEED * dt).min(dist);
                let ux = dx / dist;
                let uz = dz / dist;
                let nx = pos[0] + ux * step;
                let nz = pos[2] + uz * step;
                let cx = nx.floor() as i32;
                let cz = nz.floor() as i32;
                let fy = (pos[1] + 0.01).floor() as i32;
                let blocked = self.world.is_solid(cx, fy, cz) || self.world.is_solid(cx, fy + 1, cz);
                if !blocked {
                    pos[0] = nx;
                    pos[2] = nz;
                    self.blocked_ticks = 0;
                    self.blocked_front = None;
                    // Yaw vanilla en DEGRÉS : 0 = +Z (sud), 90 = ouest (-X),
                    // direction regardée = (-sin(yaw), cos(yaw)).
                    self.yaw = (-dx).atan2(dz).to_degrees() as f32;
                    moved = true;
                } else if self.on_ground {
                    self.vy = JUMP_VELOCITY;
                    self.on_ground = false;
                    // Sautes stériles répétées : mémoriser l'obstacle à creuser
                    // (canopée, falaise) au lieu de sauter en boucle pour rien.
                    self.blocked_ticks += 1;
                    if self.blocked_ticks >= 10 {
                        self.blocked_front = Some((cx, fy, cz));
                    }
                }
            }
        }

        if self.on_ground {
            let support = (pos[1] - 0.001).floor() as i32;
            let (fx, fz) = (pos[0].floor() as i32, pos[2].floor() as i32);
            match self.world.block_state(fx, support, fz) {
                // Sol non chargé : ne pas « tomber » à l'aveugle — le serveur
                // rejetterait ces positions (téléportation corrective en boucle).
                None => {}
                Some(_) if !self.world.is_solid(fx, support, fz) => {
                    self.on_ground = false;
                }
                Some(_) => {}
            }
        }

        if !self.on_ground {
            self.vy = (self.vy - GRAVITY * dt).max(TERMINAL_VELOCITY);
            let raw_y = pos[1] + self.vy * dt;
            let mut final_y = raw_y.max(crate::world::MIN_Y as f64);
            if self.vy < 0.0 {
                // Balayer tous les blocs traversés pendant ce tick : à vitesse
                // terminale, un seul test peut sauter une dalle fine (tunneling).
                let above = (pos[1] - 0.001).floor() as i32;
                let below = (final_y - 0.001).floor() as i32;
                let (fx, fz) = (pos[0].floor() as i32, pos[2].floor() as i32);
                for by in (below..=above).rev() {
                    if by >= crate::world::MIN_Y && self.world.is_solid(fx, by, fz) {
                        let top = by as f64 + 1.0;
                        if top <= pos[1] {
                            final_y = top;
                            if !self.on_ground {
                                self.on_ground = true;
                                println!(
                                    "atterri sur {} en ({:.2}, {}, {:.2})",
                                    self.world.block_name(fx, by, fz).unwrap_or("?"),
                                    pos[0],
                                    top,
                                    pos[2]
                                );
                            }
                            self.vy = 0.0;
                            break;
                        }
                    }
                }
            }
            pos[1] = final_y;
            moved = true;
        }

        self.pos = Some(pos);
        if moved {
            self.pending_send = true;
        }
        if let Some((x, y, z, remaining, seq, name)) = self.mining {
            let remaining = remaining - dt;
            if remaining <= 0.0 {
                self.mining = None;
                self.pending_dig_finish = Some((x, y, z, seq, name));
            } else {
                self.mining = Some((x, y, z, remaining, seq, name));
            }
        }
    }

    /// Envoie les paquets différés par physics_tick — jamais annulé, jamais concurrent.
    pub async fn flush_pending(&mut self) -> std::io::Result<()> {
        if self.disconnected {
            return Ok(());
        }
        if self.pending_respawn {
            self.pending_respawn = false;
            self.dead = false;
            self.path.clear();
            self.mining = None;
            // client_command : perform_respawn (0) — sans quoi l'entité disparaît du monde
            let mut b = Vec::new();
            crate::protocol::write_varint(0, &mut b);
            self.conn.send(ids::play_s::CLIENT_COMMAND, &b).await?;
        }
        if self.pending_send {
            self.pending_send = false;
            self.send_position().await?;
        }
        if let Some((x, y, z, seq, name)) = self.pending_dig_finish.take() {
            let mut b = Vec::new();
            crate::protocol::write_varint(2, &mut b);
            crate::protocol::write_position(x, y, z, &mut b);
            b.push(0u8);
            crate::protocol::write_varint(seq, &mut b);
            self.conn.send(ids::play_s::BLOCK_DIG, &b).await?;
            println!("miné {} en ({}, {}, {})", name, x, y, z);
            // Marquer le bloc comme air localement (le serveur confirmera par block_change)
            self.world.set_block_state(x, y, z, 0);
        }
        Ok(())
    }

    /// Attaque une entité (serverbound use_entity, type 0) + animation de bras.
    pub async fn attack_entity(&mut self, entity_id: i32) -> std::io::Result<()> {
        let mut b = Vec::new();
        crate::protocol::write_varint(entity_id, &mut b);
        crate::protocol::write_varint(0, &mut b); // type = attaque
        crate::protocol::write_varint(0, &mut b); // main hand
        b.push(0); // sneaking = false
        self.conn.send(ids::play_s::USE_ENTITY_ID, &b).await?;
        let mut swing = Vec::new();
        crate::protocol::write_varint(0, &mut swing);
        self.conn.send(ids::play_s::SWING_ID, &swing).await
    }

    /// Consomme l'item tenu en main (serverbound use_item, avec rotation en 767).
    pub async fn use_held_item(&mut self) -> std::io::Result<()> {
        let seq = self.sequence + 1;
        self.sequence = seq;
        let mut b = Vec::new();
        crate::protocol::write_varint(0, &mut b); // main hand
        crate::protocol::write_varint(seq, &mut b);
        b.extend_from_slice(&self.yaw.to_be_bytes());
        b.extend_from_slice(&self.pitch.to_be_bytes());
        self.conn.send(ids::play_s::USE_ITEM, &b).await
    }

    /// Ferme la fenêtre de conteneur ouverte (table de craft ouverte par la mission).
    pub async fn close_window(&mut self) -> std::io::Result<()> {
        if let Some(id) = self.open_window_id.take() {
            let b = vec![id];
            self.conn.send(ids::play_s::CONTAINER_CLOSE, &b).await?;
        }
        Ok(())
    }

    /// Rapport de diagnostic : un coup d'œil fiable sur l'état de la connexion.
    pub fn rapport_diag(&self) -> String {
        format!(
            "diag: paquets reçus={}, chunks={}, lots acquittés={}, positions envoyées={}, pv={:.0}, faim={}, blocages_cibles={}",
            self.stats_msgs_recus,
            self.world.chunk_count(),
            self.stats_batches,
            self.stats_positions,
            self.health,
            self.food,
            self.failed_targets.len()
        )
    }

    pub async fn send_position(&mut self) -> std::io::Result<()> {
        self.stats_positions += 1;
        let Some(p) = self.pos else { return Ok(()) };
        let mut b = Vec::with_capacity(41);
        b.extend_from_slice(&p[0].to_be_bytes());
        b.extend_from_slice(&p[1].to_be_bytes());
        b.extend_from_slice(&p[2].to_be_bytes());
        b.extend_from_slice(&self.yaw.to_be_bytes());
        b.extend_from_slice(&self.pitch.to_be_bytes());
        b.push(self.on_ground as u8);
        self.conn.send(ids::play_s::POSITION_LOOK, &b).await
    }

    /// Exécute une commande vanilla/serveur en tant que joueur op.
    /// Corps 767 officiel (minecraft-data 1.21.1) : la commande seule.
    pub async fn run_command(&mut self, commande: &str) -> std::io::Result<()> {
        let mut b = Vec::new();
        write_string(commande, &mut b);
        self.conn.send(ids::play_s::CHAT_COMMAND, &b).await
    }

    pub async fn say(&mut self, message: &str) -> std::io::Result<()> {
        // Une commande (préfixe /) passe par chat_command, pas chat_message.
        if let Some(cmd) = message.strip_prefix('/') {
            return self.run_command(cmd).await;
        }
        let mut b = Vec::new();
        write_string(message, &mut b);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        b.extend_from_slice(&(now.as_millis() as i64).to_be_bytes()); // timestamp
        b.extend_from_slice(&0i64.to_be_bytes()); // salt
        b.push(0); // signature: None (option bool = false)
        crate::protocol::write_varint(0, &mut b); // offset
        b.extend_from_slice(&[0u8; 3]); // acknowledged (3 bytes)
        self.conn.send(ids::play_s::CHAT_MESSAGE, &b).await
    }

    /// Annonce une décision dans le chat SANS spammer :
    /// - jamais deux messages à moins de 2,5 s d'intervalle ;
    /// - jamais le même message répété à moins de 20 s d'intervalle.
    ///
    /// Les messages ignorés restent visibles dans la console locale.
    pub async fn announce(&mut self, msg: &str) {
        if self.disconnected {
            return;
        }
        let now = std::time::Instant::now();
        let trop_tot = self
            .last_chat_at
            .map(|t| now.duration_since(t) < std::time::Duration::from_millis(2500))
            .unwrap_or(false);
        let repetition = self.last_chat_text.as_deref() == Some(msg)
            && self
                .last_chat_at
                .map(|t| now.duration_since(t) < std::time::Duration::from_secs(20))
                .unwrap_or(false);
        if trop_tot || repetition {
            println!("[local] {}", msg);
            // Mémoriser quand même : le message "en attente" n'est pas perdu,
            // il pourra repasser plus tard si la situation persiste.
            return;
        }
        // Le chat brut est rejeté par certains serveurs (chat.disabled.options)
        // pour un client non signé ; en tant qu'op, /say passe toujours.
        let envoye = self
            .run_command(&format!("say {}", msg))
            .await
            .is_ok();
        self.last_chat_at = Some(now);
        self.last_chat_text = Some(msg.to_string());
        if envoye {
            println!("[chat] {}", msg);
        } else {
            self.say(msg).await.ok();
        }
    }

    /// Compte-rendu d'une action : annonce le résultat réussi ou l'échec.
    pub async fn announce_result(&mut self, ok: bool, reussi: &str, echec: &str) {
        let msg = if ok { reussi } else { echec };
        self.announce(msg).await;
    }
}

pub(crate) fn debug_enabled() -> bool {
    static DBG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DBG.get_or_init(|| std::env::var_os("LIBREBOT_DEBUG").is_some())
}

/// Saute les données d'une recette declare_recipes selon son type (protocole 767).
/// Retourne false si le type (ou un ingrédient) est indécodable.
/// Vérifie qu'à cet endroit du flux commence bien une recette (nom plausible).
fn recipe_boundary_ok(b: &Body) -> bool {
    if b.rest().is_empty() {
        return true;
    }
    let mut probe = Body(b.rest());
    let len = probe.varint();
    if !(8..=96).contains(&len) {
        return false;
    }
    let name_start = varint_len_of(len);
    if b.rest().len() < name_start + len as usize {
        return false;
    }
    b.rest()[name_start..name_start + len as usize].starts_with(b"minecraft:")
}

fn varint_len_of(mut v: i32) -> usize {
    let mut n = 1;
    while v & !0x7f != 0 {
        v >>= 7;
        n += 1;
    }
    n
}

fn skip_smelting(b: &mut Body) -> bool {
    let _group = b.string();
    let _category = b.varint();
    skip_ingredient(b)
        && skip_slot(b)
        && {
            let _exp = b.f32();
            let _time = b.varint();
            true
        }
}

fn skip_stonecutting(b: &mut Body) -> bool {
    let _group = b.string();
    skip_ingredient(b) && skip_slot(b)
}

fn skip_smithing_transform(b: &mut Body) -> bool {
    skip_ingredient(b) && skip_ingredient(b) && skip_ingredient(b) && skip_slot(b)
}

fn skip_smithing_trim(b: &mut Body) -> bool {
    skip_ingredient(b) && skip_ingredient(b) && skip_ingredient(b)
}

fn skip_simple_category(b: &mut Body) -> bool {
    let category = b.varint();
    (0..=7).contains(&category)
}

/// Saute les données d'une recette declare_recipes selon son type (protocole 767).
/// L'ordre exact du registre (smelting → decorated_pot) varie selon vanilla/Paper :
/// pour les types 15..=23, chaque format est essayé et validé contre la recette
/// suivante du flux (nom "minecraft:..."), avec retour arrière en cas d'échec.
/// Retourne false si aucun format ne s'applique.
fn skip_recipe_data(b: &mut Body, rtype: i32) -> bool {
    match rtype {
        0 => {
            // crafting_shaped : group, category, w, h, w*h ingrédients, résultat, showNotification
            let _group = b.string();
            let _category = b.varint();
            let w = b.varint().max(0) as usize;
            let h = b.varint().max(0) as usize;
            (0..w.saturating_mul(h)).all(|_| skip_ingredient(b))
                && skip_slot(b)
                && {
                    b.bool(); // showNotification : valeur ignorée, octet à consommer
                    true
                }
        }
        1 => {
            // crafting_shapeless : group, category, n ingrédients, résultat
            let _group = b.string();
            let _category = b.varint();
            let n = b.varint().max(0) as usize;
            (0..n).all(|_| skip_ingredient(b)) && skip_slot(b)
        }
        2..=14 => {
            // recettes spéciales (teinture, réparation...) : catégorie seule
            skip_simple_category(b)
        }
        15..=23 => {
            let snapshot = b.0;
            let formats: [fn(&mut Body) -> bool; 5] = [
                skip_smelting,
                skip_stonecutting,
                skip_smithing_transform,
                skip_smithing_trim,
                skip_simple_category,
            ];
            for format in formats {
                b.0 = snapshot;
                if format(b) && recipe_boundary_ok(b) {
                    return true;
                }
            }
            b.0 = snapshot;
            false
        }
        _ => false,
    }
}

fn skip_ingredient(b: &mut Body) -> bool {
    let n = b.varint().max(0) as usize;
    (0..n).all(|_| skip_slot(b))
}

fn skip_slot(b: &mut Body) -> bool {
    crate::inventory::skip_slot(b)
}

/// Paquet `player_chat` (0x39) : "<pseudo> message" (best effort).
/// Suppose le chat type référencé par ID (registre synchronisé en configuration).
fn parse_player_chat(body: &[u8]) -> Option<String> {
    let mut b = Body(body);
    let _sender_uuid = b.take(16);
    let _index = b.varint();
    if b.bool() {
        b.take(256);
    }
    let message = b.string();
    let _timestamp = b.i64();
    let _salt = b.i64();
    let previous = b.varint().max(0) as usize;
    for _ in 0..previous {
        let _id = b.varint();
        if b.bool() {
            b.take(256);
        }
    }
    if b.bool() {
        nbt::parse(&mut b.0);
    }
    let filter_type = b.varint();
    if filter_type == 2 {
        let n = b.varint() as usize;
        b.take(n * 8);
    }
    let _chat_type_id = b.varint();
    let sender = nbt::parse(&mut b.0).map(|n| nbt::text(&n))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "?".into());
    if b.bool() {
        nbt::parse(&mut b.0);
    }
    Some(format!("<{}> {}", sender, message))
}

fn chat_reason(body: &[u8]) -> String {
    // Composant de chat = NBT en 767 : essayer NBT d'abord, puis string brute
    let mut buf: &[u8] = body;
    if let Some(n) = nbt::parse(&mut buf) {
        let t = nbt::text(&n);
        if !t.is_empty() {
            return t;
        }
    }
    let mut b = Body(body);
    let as_string = b.string();
    if !as_string.is_empty() && as_string.len() < 1000 {
        return as_string;
    }
    // Dernier recours : hex dump
    format!("<raison illisible: {} octets>", body.len())
}
