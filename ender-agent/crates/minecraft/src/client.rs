use tokio::sync::broadcast;

use crate::adapter::{AdapterError, MinecraftAdapter, PlayerSnapshot};
use crate::events::GameEvent;

pub struct Client {
    adapter: Box<dyn MinecraftAdapter>,
}

impl Client {
    pub fn new(adapter: impl MinecraftAdapter + 'static) -> Self {
        Self {
            adapter: Box::new(adapter),
        }
    }

    pub fn connect(&self) -> Result<(), AdapterError> {
        self.adapter.connect()
    }

    pub fn disconnect(&self) {
        self.adapter.disconnect();
    }

    pub fn is_connected(&self) -> bool {
        self.adapter.is_connected()
    }

    pub fn events(&self) -> broadcast::Receiver<GameEvent> {
        self.adapter.events()
    }

    pub fn snapshot(&self) -> Result<PlayerSnapshot, AdapterError> {
        self.adapter.snapshot()
    }

    pub fn adapter(&self) -> &dyn MinecraftAdapter {
        self.adapter.as_ref()
    }
}
