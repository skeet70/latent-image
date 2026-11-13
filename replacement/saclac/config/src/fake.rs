use std::collections::HashMap;

use crate::adjustment::{AdjustmentKey, Adjustments};

#[derive(Clone, Default)]
pub struct Area(HashMap<u32, u8>);

impl Area {
    pub fn with(&self, at: u32, value: u8) -> Self {
        let mut area = self.clone();
        area.0.insert(at, value);
        area
    }

    pub fn with_half(&self, at: u32, value: u16) -> Self {
        let [low, high] = value.to_le_bytes();
        self.with(at, low).with(at + 1, high)
    }

    pub fn with_word(&self, at: u32, value: u32) -> Self {
        let [a, b, c, d] = value.to_le_bytes();
        self.with(at, a).with(at + 1, b).with(at + 2, c).with(at + 3, d)
    }
}

impl Adjustments for Area {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.0.get(&key.0).copied().unwrap_or(0) }
}
