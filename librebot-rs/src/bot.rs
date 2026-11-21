//! Session client : handshake, login offline, configuration, boucle de jeu.
//! IDs de paquets du protocole 1.21.1 (source : minecraft-data).

use crate::inventory::{window_click_body, Inventory};
use crate::nbt;
use crate::protocol::Conn;
use crate::protocol::ReadOutcome::{Eof, Packet};
use crate::protocol::{offline_uuid, write_string, Body, MC_VERSION, PROTOCOL_VERSION};
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
    pub mod play_c {
        // IDs protocole 767 (Minecraft 1.21.1) — valeurs originales validées
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
        pub const SET_HEALTH: i32 = 0x59;
        pub const EXPERIENCE: i32 = 0x58;
        pub const GAME_EVENT: i32 = 0x1e;
        pub const RESPAWN: i32 = 0x41;
        pub const DECLARE_RECIPES: i32 = 0x6d;
        pub const OPEN_WINDOW: i32 = 0x2e;
        pub const CLOSE_WINDOW: i32 = 0x13;
        pub const CHUNK_BATCH_START: i32 = 0x0d;
        pub const CHUNK_BATCH_FINISHED: i32 = 0x0c;
        pub const MULTI_BLOCK_CHANGE: i32 = 0x3c;
        pub const SET_DEFAULT_SPAWN: i32 = 0x4b;
        pub const UPDATE_VIEW_POSITION: i32 = 0x4a;
        pub const UPDATE_VIEW_DISTANCE: i32 = 0x4d;
        pub const SET_TIME: i32 = 0x64;
        pub const SET_ENTITY_DATA: i32 = 0x52;
    }
    pub mod play_s {
        // IDs serverbound 767
        pub const TELEPORT_CONFIRM: i32 = 0x00;
        pub const ATTACK: i32 = 0x01;
        pub const CHAT_MESSAGE: i32 = 0x06;
        pub const CHUNK_BATCH_RECEIVED: i32 = 0x08;
        pub const WINDOW_CLICK: i32 = 0x0e;
        pub const CONTAINER_CLOSE: i32 = 0x0f;
        pub const POSITION_LOOK: i32 = 0x1b;
        pub const POSITION: i32 = 0x1a;
        pub const CRAFT_RECIPE: i32 = 0x1d;
        pub const BLOCK_DIG: i32 = 0x24;
        pub const PLAYER_LOADED: i32 = 0x2c;
        pub const HELD_ITEM_SLOT: i32 = 0x2f;
        pub const BLOCK_PLACE: i32 = 0x38;
        pub const USE_ITEM: i32 = 0x2e;
        pub const SWING: i32 = 0x2d;
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
}

impl Bot {
    pub async fn connect(host: &str, port: u16, username: &str) -> std::io::Result<Self> {
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
                        path: VecDeque::new(),
                        mining: None,
                        sequence: 0,
                        recipes: Vec::new(),
                        open_window_id: None,
                        chunks_in_batch: 0,
                    };
                    bot.configuration().await?;
                    return Ok(bot);
                }
                Packet(_, _) => {}
            }
        }
    }

    async fn configuration(&mut self) -> std::io::Result<()> {
        loop {
            match self.conn.read_packet().await? {
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
                    settings.push(8);                        // viewDistance (i8)
                    crate::protocol::write_varint(0, &mut settings); // chatFlags
                    settings.push(1);                        // chatColors
                    settings.push(0x7f);                     // skinParts (u8)
                    crate::protocol::write_varint(1, &mut settings); // mainHand (1=right)
                    settings.push(0);                        // enableTextFiltering
                    settings.push(0);                        // enableServerListing
                    // En 767: pas de particleStatus
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
            if let Packet(id, body) = &pkt {
                if *id != 0x2c && *id != 0x35 { // pas keep_alive ni held_item
                    eprintln!("[pkt] 0x{:02x} ({} o)", id, body.len());
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
                }
                Packet(ids::play_c::MAP_CHUNK, body) => {
                    eprintln!("[chunk] reçu {} octets", body.len());
                    self.chunks_in_batch += 1;
                    match self.world.apply_map_chunk(&body) {
                        Ok(()) => eprintln!("[chunk] OK ({} octets, {} chunks total)", body.len(), self.world.chunk_count()),
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
                // declare_recipes (0x85 en 775) : liste des recettes {name, items}
                Packet(ids::play_c::DECLARE_RECIPES, body) => {
                    let mut b = Body(&body);
                    let n = b.varint().max(0) as usize;
                    for _ in 0..n {
                        let name = b.string();
                        let n_items = b.varint().max(0) as usize;
                        for _ in 0..n_items { b.varint(); }
                        self.recipes.push(name);
                    }
                    // stonecutter recipes (on les ignore)
                    let _n_sc = b.varint().max(0) as usize;
                }
                // open_window (0x3b en 775) : ouverture d'une fenêtre (table, fourneau...)
                Packet(ids::play_c::OPEN_WINDOW, body) => {
                    let mut b = Body(&body);
                    let window_id = b.u8();
                    let _window_type = b.varint();
                    let _title_len = b.varint();
                    // skip title NBT/string
                    let _ = crate::nbt::parse(&mut b.0);
                    self.open_window_id = Some(window_id);
                }
                // close_window (0x11 en 775)
                Packet(ids::play_c::CLOSE_WINDOW, body) => {
                    let mut b = Body(&body);
                    let _window_id = b.u8();
                    self.open_window_id = None;
                }
                Packet(_, _) => {}
            }
        }
    }

    /// Paquet `login` (0x2b) : entityId, dimension, etc.
    fn apply_login(&mut self, body: &[u8]) {
        let mut b = Body(&body);
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
        let mut b = Body(&body);
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
        let mut slot = Vec::new();
        slot.extend_from_slice(&0i16.to_be_bytes());
        self.conn.send(ids::play_s::HELD_ITEM_SLOT, &slot).await?;
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

    /// Pas de simulation : avance du chemin, saut sur obstacle, gravité, contact au sol.
    /// Lit et traite tous les paquets en attente sans bloquer (drain).
    /// À appeler régulièrement pendant les actions longues pour garder le monde à jour.
    pub async fn drain_packets(&mut self) {
        // Lire jusqu'à 300 paquets ou timeout de 50ms sans données
        for _ in 0..300 {
            match tokio::time::timeout(std::time::Duration::from_millis(50), self.play_turn()).await {
                Ok(Ok(_)) => continue,
                Ok(Err(_)) => break,
                Err(_) => break, // timeout = plus rien en attente
            }
        }
    }

    pub async fn tick(&mut self, dt: f64) -> std::io::Result<()> {
        let Some(p) = self.pos else { return Ok(()) };
        let mut pos = p;
        let mut moved = false;

        if let Some(&(tx, tz)) = self.path.front() {
            let dx = tx - pos[0];
            let dz = tz - pos[2];
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < 0.25 {
                self.path.pop_front();
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
                    self.yaw = (dz).atan2(dx) as f32;
                    moved = true;
                } else if self.on_ground {
                    self.vy = JUMP_VELOCITY;
                    self.on_ground = false;
                }
            }
        }

        if self.on_ground {
            let support = (pos[1] - 0.001).floor() as i32;
            let (fx, fz) = (pos[0].floor() as i32, pos[2].floor() as i32);
            if !(support >= crate::world::MIN_Y && self.world.is_solid(fx, support, fz)) {
                self.on_ground = false;
            }
        }

        if !self.on_ground {
            self.vy = (self.vy - GRAVITY * dt).max(TERMINAL_VELOCITY);
            let raw_y = pos[1] + self.vy * dt;
            let mut final_y = raw_y.max(crate::world::MIN_Y as f64);
            if self.vy < 0.0 {
                let below = (final_y - 0.001).floor() as i32;
                let (fx, fz) = (pos[0].floor() as i32, pos[2].floor() as i32);
                if below >= crate::world::MIN_Y && self.world.is_solid(fx, below, fz) {
                    let top = below as f64 + 1.0;
                    if top <= pos[1] {
                        final_y = top;
                        if !self.on_ground {
                            self.on_ground = true;
                            println!(
                                "atterri sur {} en ({:.2}, {}, {:.2})",
                                self.world.block_name(fx, below, fz).unwrap_or("?"),
                                pos[0],
                                top,
                                pos[2]
                            );
                        }
                        self.vy = 0.0;
                    }
                }
            }
            pos[1] = final_y;
            moved = true;
        }

        self.pos = Some(pos);
        if moved {
            self.send_position().await?;
        }
        // client_tick_end (0x0d en 775) : envoyé à chaque tick pour débloquer l'envoi des chunks
        // tick_end désactivé en 767
        if let Some((x, y, z, remaining, seq, name)) = self.mining {
            let remaining = remaining - dt;
            if remaining <= 0.0 {
                self.mining = None;
                let mut b = Vec::new();
                crate::protocol::write_varint(2, &mut b);
                crate::protocol::write_position(x, y, z, &mut b);
                b.push(0u8);
                crate::protocol::write_varint(seq, &mut b);
                self.conn.send(ids::play_s::BLOCK_DIG, &b).await?;
                println!("miné {} en ({}, {}, {})", name, x, y, z);
                // Marquer le bloc comme air localement (le serveur confirmera par block_change)
                self.world.set_block_state(x, y, z, 0);
            } else {
                self.mining = Some((x, y, z, remaining, seq, name));
            }
        }
        Ok(())
    }

    pub async fn send_position(&mut self) -> std::io::Result<()> {
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

    pub async fn say(&mut self, message: &str) -> std::io::Result<()> {
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
        b.push(0); // checksum (u8) — protocole 775
        self.conn.send(ids::play_s::CHAT_MESSAGE, &b).await
    }
}

/// Paquet `player_chat` (0x39) : "<pseudo> message" (best effort).
/// Suppose le chat type référencé par ID (registre synchronisé en configuration).
fn parse_player_chat(body: &[u8]) -> Option<String> {
    let mut b = Body(&body);
    let _sender_uuid = b.string();
    let _index = b.varint();
    if b.bool() {
        b.take(256);
    }
    let message = b.string();
    let _timestamp = b.i64();
    let _salt = b.i64();
    let previous = b.varint() as usize;
    for _ in 0..previous {
        let id = b.varint();
        if id == 0 {
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
    // Essayer d'abord comme une string préfixée par VarInt (format texte brut)
    let mut b = Body(&body);
    let as_string = b.string();
    if !as_string.is_empty() && as_string.len() < 1000 {
        return as_string;
    }
    // Sinon essayer NBT (composant de chat)
    let mut buf: &[u8] = body;
    if let Some(n) = nbt::parse(&mut buf) {
        let t = nbt::text(&n);
        if !t.is_empty() {
            return t;
        }
    }
    // Dernier recours : hex dump
    format!("<raison illisible: {} octets>", body.len())
}
