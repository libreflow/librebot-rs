use std::collections::HashMap;
#[cfg(feature = "live")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
#[cfg(feature = "live")]
use azalea::ecs::component::Component;
#[cfg(feature = "live")]
use azalea::pathfinder::PathfinderClientExt;

use thiserror::Error;
use tokio::sync::broadcast;

use crate::events::{EntityInfo, GameEvent};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("non connecte au serveur Minecraft")]
    NotConnected,
    #[error("commande invalide: {0}")]
    InvalidCommand(String),
    #[error("echec de connexion: {0}")]
    Connect(String),
    #[error("action non supportee par cet adaptateur: {0}")]
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub tick: u64,
    pub position: [f64; 3],
    pub dimension: String,
    pub health: f32,
    pub hunger: f32,
    pub inventory: Vec<(String, u32)>,
    pub entities: Vec<EntityInfo>,
    pub nearby_blocks: Vec<([i32; 3], String)>,
}

pub trait MinecraftAdapter: Send + Sync {
    fn events(&self) -> broadcast::Receiver<GameEvent>;
    fn connect(&self) -> Result<(), AdapterError>;
    fn disconnect(&self);
    fn is_connected(&self) -> bool;
    fn snapshot(&self) -> Result<PlayerSnapshot, AdapterError>;
    fn goto(&self, target: [i32; 3]);
    fn stop_moving(&self);
    fn break_block(&self, pos: [i32; 3]) -> Result<(), AdapterError>;
    fn craft(&self, item: &str, count: u32) -> Result<(), AdapterError>;
    fn chat(&self, message: &str);
}

impl<T: MinecraftAdapter + ?Sized> MinecraftAdapter for Arc<T> {
    fn events(&self) -> broadcast::Receiver<GameEvent> {
        (**self).events()
    }

    fn connect(&self) -> Result<(), AdapterError> {
        (**self).connect()
    }

    fn disconnect(&self) {
        (**self).disconnect()
    }

    fn is_connected(&self) -> bool {
        (**self).is_connected()
    }

    fn snapshot(&self) -> Result<PlayerSnapshot, AdapterError> {
        (**self).snapshot()
    }

    fn goto(&self, target: [i32; 3]) {
        (**self).goto(target)
    }

    fn stop_moving(&self) {
        (**self).stop_moving()
    }

    fn break_block(&self, pos: [i32; 3]) -> Result<(), AdapterError> {
        (**self).break_block(pos)
    }

    fn craft(&self, item: &str, count: u32) -> Result<(), AdapterError> {
        (**self).craft(item, count)
    }

    fn chat(&self, message: &str) {
        (**self).chat(message)
    }
}

const SIM_RECIPES: &[(&str, u32, &[(&str, u32)])] = &[
    ("minecraft:oak_planks", 4, &[("minecraft:oak_log", 1)]),
    ("minecraft:stick", 4, &[("minecraft:oak_planks", 2)]),
    ("minecraft:crafting_table", 1, &[("minecraft:oak_planks", 4)]),
    (
        "minecraft:wooden_pickaxe",
        1,
        &[("minecraft:oak_planks", 3), ("minecraft:stick", 2)],
    ),
];

enum Pending {
    Craft { item: String, count: u32, done_tick: u64 },
    Break { pos: [i32; 3], drop: String, done_tick: u64 },
}

struct SimState {
    connected: bool,
    running: bool,
    tick: u64,
    position: [f64; 3],
    target: Option<[f64; 3]>,
    health: f32,
    hunger: f32,
    inventory: Vec<(String, u32)>,
    entities: Vec<EntityInfo>,
    blocks: HashMap<[i32; 3], String>,
    pending: Vec<Pending>,
}

impl SimState {
    fn new() -> Self {
        let mut blocks = HashMap::new();
        blocks.insert([2, 64, 0], "minecraft:oak_log".to_string());
        blocks.insert([3, 64, 0], "minecraft:oak_log".to_string());
        blocks.insert([2, 65, 0], "minecraft:oak_log".to_string());
        Self {
            connected: false,
            running: false,
            tick: 0,
            position: [0.5, 64.0, 0.5],
            target: None,
            health: 20.0,
            hunger: 20.0,
            inventory: vec![],
            entities: vec![EntityInfo {
                id: 1,
                kind: "pig".to_string(),
                pos: [5.0, 64.0, 3.0],
                hostile: false,
            }],
            blocks,
            pending: vec![],
        }
    }

    fn block_at(&self, pos: [i32; 3]) -> String {
        if let Some(b) = self.blocks.get(&pos) {
            return b.clone();
        }
        if pos[1] < 64 {
            "minecraft:stone".to_string()
        } else if pos[1] == 64 {
            "minecraft:grass_block".to_string()
        } else {
            "minecraft:air".to_string()
        }
    }

    fn inv_count(&self, item: &str) -> u32 {
        self.inventory
            .iter()
            .find(|(i, _)| i == item)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    fn inv_add(&mut self, item: &str, count: u32) {
        if let Some(e) = self.inventory.iter_mut().find(|(i, _)| i == item) {
            e.1 += count;
        } else {
            self.inventory.push((item.to_string(), count));
        }
    }

    fn inv_remove(&mut self, item: &str, count: u32) -> bool {
        if let Some(e) = self.inventory.iter_mut().find(|(i, _)| i == item) {
            if e.1 >= count {
                e.1 -= count;
                if e.1 == 0 {
                    self.inventory.retain(|(i, _)| i != item);
                }
                return true;
            }
        }
        false
    }
}

pub struct SimAdapter {
    events_tx: broadcast::Sender<GameEvent>,
    sim: Arc<StdMutex<SimState>>,
    handle: StdMutex<Option<tokio::task::JoinHandle<()>>>,
}

impl SimAdapter {
    pub fn new() -> Self {
        let (events_tx, _) = broadcast::channel(4096);
        Self {
            events_tx,
            sim: Arc::new(StdMutex::new(SimState::new())),
            handle: StdMutex::new(None),
        }
    }

    pub fn add_block(&self, pos: [i32; 3], block: &str) {
        self.sim.lock().unwrap().blocks.insert(pos, block.to_string());
    }

    pub fn add_entity(&self, entity: EntityInfo) {
        self.sim.lock().unwrap().entities.push(entity);
    }

    pub fn give(&self, item: &str, count: u32) {
        self.sim.lock().unwrap().inv_add(item, count);
    }

    pub fn tick_count(&self) -> u64 {
        self.sim.lock().unwrap().tick
    }
}

impl Default for SimAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MinecraftAdapter for SimAdapter {
    fn events(&self) -> broadcast::Receiver<GameEvent> {
        self.events_tx.subscribe()
    }

    fn connect(&self) -> Result<(), AdapterError> {
        let mut sim = self.sim.lock().unwrap();
        sim.connected = true;
        sim.running = true;
        drop(sim);
        let _ = self.events_tx.send(GameEvent::Login);
        let events_tx = self.events_tx.clone();
        let sim = Arc::clone(&self.sim);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(50));
            loop {
                interval.tick().await;
                let mut sim = sim.lock().unwrap();
                if !sim.running {
                    break;
                }
                sim.tick += 1;
                if let Some(target) = sim.target {
                    let pos = sim.position;
                    let delta = [target[0] - pos[0], target[1] - pos[1], target[2] - pos[2]];
                    let dist = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
                    let step = 0.215;
                    if dist <= step {
                        sim.position = target;
                        sim.target = None;
                    } else {
                        sim.position = [
                            pos[0] + delta[0] / dist * step,
                            pos[1] + delta[1] / dist * step,
                            pos[2] + delta[2] / dist * step,
                        ];
                    }
                }
                let tick = sim.tick;
                let mut done = Vec::new();
                sim.pending.retain(|p| match p {
                    Pending::Craft { done_tick, .. } | Pending::Break { done_tick, .. } => {
                        if *done_tick <= tick {
                            done.push(match p {
                                Pending::Craft { item, count, .. } => Pending::Craft {
                                    item: item.clone(),
                                    count: *count,
                                    done_tick: 0,
                                },
                                Pending::Break { pos, drop, .. } => Pending::Break {
                                    pos: *pos,
                                    drop: drop.clone(),
                                    done_tick: 0,
                                },
                            });
                            false
                        } else {
                            true
                        }
                    }
                });
                for p in done {
                    match p {
                        Pending::Craft { item, count, .. } => {
                            sim.inv_add(&item, count);
                            let inv = sim.inventory.clone();
                            let _ = events_tx.send(GameEvent::InventoryUpdate { items: inv });
                        }
                        Pending::Break { pos, drop, .. } => {
                            sim.blocks.remove(&pos);
                            sim.inv_add(&drop, 1);
                            let _ = events_tx.send(GameEvent::BlockUpdate {
                                pos,
                                block: "minecraft:air".to_string(),
                            });
                            let inv = sim.inventory.clone();
                            let _ = events_tx.send(GameEvent::InventoryUpdate { items: inv });
                        }
                    }
                }
                if tick % 100 == 0 {
                    let _ = events_tx.send(GameEvent::HealthUpdate {
                        health: sim.health,
                        hunger: sim.hunger,
                    });
                }
                let _ = events_tx.send(GameEvent::Tick);
            }
        });
        *self.handle.lock().unwrap() = Some(handle);
        Ok(())
    }

    fn disconnect(&self) {
        let mut sim = self.sim.lock().unwrap();
        sim.connected = false;
        sim.running = false;
        drop(sim);
        let _ = self.events_tx.send(GameEvent::Disconnect { reason: None });
        if let Some(h) = self.handle.lock().unwrap().take() {
            h.abort();
        }
    }

    fn is_connected(&self) -> bool {
        self.sim.lock().unwrap().connected
    }

    fn snapshot(&self) -> Result<PlayerSnapshot, AdapterError> {
        let sim = self.sim.lock().unwrap();
        if !sim.connected {
            return Err(AdapterError::NotConnected);
        }
        let center = [
            sim.position[0].floor() as i32,
            sim.position[1].floor() as i32,
            sim.position[2].floor() as i32,
        ];
        let mut nearby_blocks = Vec::new();
        for dx in -4..=4 {
            for dy in -2..=2 {
                for dz in -4..=4 {
                    let pos = [center[0] + dx, center[1] + dy, center[2] + dz];
                    nearby_blocks.push((pos, sim.block_at(pos)));
                }
            }
        }
        Ok(PlayerSnapshot {
            tick: sim.tick,
            position: sim.position,
            dimension: "minecraft:overworld".to_string(),
            health: sim.health,
            hunger: sim.hunger,
            inventory: sim.inventory.clone(),
            entities: sim.entities.clone(),
            nearby_blocks,
        })
    }

    fn goto(&self, target: [i32; 3]) {
        let mut sim = self.sim.lock().unwrap();
        if sim.connected {
            sim.target = Some([target[0] as f64 + 0.5, target[1] as f64, target[2] as f64 + 0.5]);
        }
    }

    fn stop_moving(&self) {
        self.sim.lock().unwrap().target = None;
    }

    fn break_block(&self, pos: [i32; 3]) -> Result<(), AdapterError> {
        let mut sim = self.sim.lock().unwrap();
        if !sim.connected {
            return Err(AdapterError::NotConnected);
        }
        let block = sim.block_at(pos);
        if block == "minecraft:air" {
            return Err(AdapterError::InvalidCommand(format!("bloc air en {pos:?}")));
        }
        let drop = if block.contains("log") || block.contains("wood") {
            "minecraft:oak_log".to_string()
        } else {
            block.clone()
        };
        let done_tick = sim.tick + 10;
        sim.pending.push(Pending::Break { pos, drop, done_tick });
        Ok(())
    }

    fn craft(&self, item: &str, count: u32) -> Result<(), AdapterError> {
        let item = crate::normalize_id(item);
        let mut sim = self.sim.lock().unwrap();
        if !sim.connected {
            return Err(AdapterError::NotConnected);
        }
        let recipe = SIM_RECIPES
            .iter()
            .find(|(out, _, _)| *out == item)
            .ok_or_else(|| AdapterError::InvalidCommand(format!("recette inconnue: {item}")))?;
        let crafts = count.div_ceil(recipe.1);
        for (input, n) in recipe.2 {
            if sim.inv_count(input) < *n * crafts {
                return Err(AdapterError::InvalidCommand(format!(
                    "materiaux insuffisants pour {item}: besoin de {} x{n}",
                    input
                )));
            }
        }
        for (input, n) in recipe.2 {
            sim.inv_remove(input, *n * crafts);
        }
        let done_tick = sim.tick + 20;
        sim.pending.push(Pending::Craft {
            item,
            count: recipe.1 * crafts,
            done_tick,
        });
        Ok(())
    }

    fn chat(&self, message: &str) {
        let _ = self.events_tx.send(GameEvent::Chat {
            sender: Some("sim-server".to_string()),
            content: message.to_string(),
        });
    }
}

#[cfg(feature = "live")]
pub struct AzaleaAdapter {
    username: String,
    address: String,
    events_tx: broadcast::Sender<GameEvent>,
    link: Arc<AgentLink>,
    connected: Arc<AtomicBool>,
}

#[cfg(feature = "live")]
struct AgentLink {
    client: StdMutex<Option<azalea::Client>>,
}

#[cfg(feature = "live")]
#[derive(Clone, Component)]
struct AdapterState {
    link: Arc<AgentLink>,
    events_tx: broadcast::Sender<GameEvent>,
    connected: Arc<AtomicBool>,
}

#[cfg(feature = "live")]
impl Default for AdapterState {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(4096);
        Self {
            link: Arc::new(AgentLink {
                client: StdMutex::new(None),
            }),
            events_tx: tx,
            connected: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(feature = "live")]
async fn handle(
    bot: azalea::Client,
    event: azalea::Event,
    state: AdapterState,
) -> anyhow::Result<()> {
    match &event {
        azalea::Event::Login => {
            *state.link.client.lock().unwrap() = Some(bot.clone());
            state.connected.store(true, Ordering::SeqCst);
            let _ = state.events_tx.send(GameEvent::Login);
        }
        azalea::Event::Tick => {
            let _ = state.events_tx.send(GameEvent::Tick);
        }
        azalea::Event::Chat(m) => {
            let (sender, content) = m.split_sender_and_content();
            let _ = state.events_tx.send(GameEvent::Chat {
                sender,
                content: content.to_string(),
            });
        }
        azalea::Event::Death(_) => {
            let _ = state.events_tx.send(GameEvent::Death { reason: None });
        }
        azalea::Event::Disconnect(reason) => {
            state.connected.store(false, Ordering::SeqCst);
            let _ = state.events_tx.send(GameEvent::Disconnect {
                reason: reason.as_ref().map(|r| r.to_string()),
            });
        }
        _ => {}
    }
    Ok(())
}

#[cfg(feature = "live")]
struct NearGoal {
    target: [i32; 3],
    radius: i32,
}

#[cfg(feature = "live")]
impl azalea::pathfinder::goals::Goal for NearGoal {
    fn heuristic(&self, n: azalea::BlockPos) -> f32 {
        let dx = (self.target[0] - n.x) as f32;
        let dy = (self.target[1] - n.y) as f32;
        let dz = (self.target[2] - n.z) as f32;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    fn success(&self, n: azalea::BlockPos) -> bool {
        let dx = n.x - self.target[0];
        let dy = n.y - self.target[1];
        let dz = n.z - self.target[2];
        dx * dx + dy * dy + dz * dz <= self.radius * self.radius
    }
}

#[cfg(feature = "live")]
impl AzaleaAdapter {
    pub fn new(username: &str, address: &str) -> Self {
        let (events_tx, _) = broadcast::channel(4096);
        Self {
            username: username.to_string(),
            address: address.to_string(),
            events_tx,
            link: Arc::new(AgentLink {
                client: StdMutex::new(None),
            }),
            connected: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(feature = "live")]
impl MinecraftAdapter for AzaleaAdapter {
    fn events(&self) -> broadcast::Receiver<GameEvent> {
        self.events_tx.subscribe()
    }

    fn connect(&self) -> Result<(), AdapterError> {
        let account = azalea::Account::offline(&self.username);
        let address = azalea::protocol::ServerAddress::try_from(self.address.as_str())
            .map_err(|_| AdapterError::Connect(format!("adresse invalide: {}", self.address)))?;
        let state = AdapterState {
            link: Arc::clone(&self.link),
            events_tx: self.events_tx.clone(),
            connected: Arc::clone(&self.connected),
        };
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime tokio interne");
            rt.block_on(async move {
                let result = azalea::ClientBuilder::new()
                    .set_handler(handle)
                    .set_state(state)
                    .start(account, address)
                    .await;
                if let Err(e) = result {
                    tracing::error!("client azalea arrete: {e}");
                }
            });
        });
        Ok(())
    }

    fn disconnect(&self) {
        if let Some(bot) = self.link.client.lock().unwrap().as_ref() {
            bot.disconnect();
        }
        self.connected.store(false, Ordering::SeqCst);
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn snapshot(&self) -> Result<PlayerSnapshot, AdapterError> {
        let guard = self.link.client.lock().unwrap();
        let bot = guard.as_ref().ok_or(AdapterError::NotConnected)?;
        let pos = bot.position();
        let health = bot.health();
        let hunger = bot.hunger().food as f32;
        let mut inventory = Vec::new();
        if let Some(inv) = bot.get_component::<azalea::inventory::Inventory>() {
            for slot in inv.inventory_menu.slots() {
                if let azalea::inventory::ItemSlot::Present(data) = slot {
                    if data.count > 0 {
                        inventory.push((
                            crate::normalize_id(&data.kind.to_string()),
                            data.count as u32,
                        ));
                    }
                }
            }
        }
        let world = bot.world();
        let world = world.read();
        let center = azalea::BlockPos::new(
            pos.x.floor() as i32,
            pos.y.floor() as i32,
            pos.z.floor() as i32,
        );
        let mut nearby_blocks = Vec::new();
        for dx in -4..=4 {
            for dy in -2..=2 {
                for dz in -4..=4 {
                    let p = azalea::BlockPos::new(center.x + dx, center.y + dy, center.z + dz);
                    if let Some(state) = world.get_block_state(&p) {
                        nearby_blocks.push(([p.x, p.y, p.z], format!("{:?}", state)));
                    }
                }
            }
        }
        Ok(PlayerSnapshot {
            tick: 0,
            position: [pos.x, pos.y, pos.z],
            dimension: "minecraft:overworld".to_string(),
            health,
            hunger,
            inventory,
            entities: Vec::new(),
            nearby_blocks,
        })
    }

    fn goto(&self, target: [i32; 3]) {
        if let Some(bot) = self.link.client.lock().unwrap().as_ref() {
            bot.goto(NearGoal { target, radius: 1 });
        }
    }

    fn stop_moving(&self) {
        if let Some(bot) = self.link.client.lock().unwrap().as_ref() {
            bot.stop_pathfinding();
        }
    }

    fn break_block(&self, pos: [i32; 3]) -> Result<(), AdapterError> {
        let mut guard = self.link.client.lock().unwrap();
        let bot = guard.as_mut().ok_or(AdapterError::NotConnected)?;
        bot.start_mining(azalea::BlockPos::new(pos[0], pos[1], pos[2]));
        Ok(())
    }

    fn craft(&self, item: &str, _count: u32) -> Result<(), AdapterError> {
        Err(AdapterError::Unsupported(format!(
            "fabrication live '{item}' via clics d'inventaire: phase 4"
        )))
    }

    fn chat(&self, message: &str) {
        if let Some(bot) = self.link.client.lock().unwrap().as_ref() {
            bot.chat(message);
        }
    }
}
