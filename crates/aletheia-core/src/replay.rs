use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum InputButton {
    A = 0,
    B = 1,
    Start = 2,
    Select = 3,
    Up = 4,
    Down = 5,
    Left = 6,
    Right = 7,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum InputState {
    Released = 0,
    Pressed = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InputEvent {
    pub cycle: u64,
    pub port: u8,
    pub button: InputButton,
    pub state: InputState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayLog {
    pub version: u16,
    pub events: Vec<InputEvent>,
}

impl Default for ReplayLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplayLog {
    pub const CURRENT_VERSION: u16 = 1;

    pub fn new() -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            events: Vec::new(),
        }
    }

    pub fn record(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    pub fn sorted_events(&self) -> Vec<InputEvent> {
        let mut sorted = self.events.clone();
        sorted.sort_by_key(|event| {
            (
                event.cycle,
                event.port,
                event.button as u8,
                event.state as u8,
            )
        });
        sorted
    }
}

impl From<Vec<InputEvent>> for ReplayLog {
    fn from(events: Vec<InputEvent>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            events,
        }
    }
}
