//! Classifications of exposure modes, selection kinds and their flag words.

/// The exposure context's mode word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExposureMode(pub u32);

impl ExposureMode {
    pub const fn is_primary(self) -> bool { matches!(self.0, 0 | 1 | 4) }
    pub const fn uses_primary_limit(self) -> bool { matches!(self.0, 1 | 2 | 4) }
    pub const fn uses_adjustment(self) -> bool { matches!(self.0, 3 | 5) }
}

/// The selection mode the profile's selection policy reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionMode(pub u32);

impl SelectionMode {
    pub const fn category(self) -> u8 {
        match self.0 { 5 => 2, 2 | 13 => 3, _ => 1 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionKind(pub u32);

impl SelectionKind {
    /// Whether the kind is accepted when neither an override nor the enable flag admits it.
    pub const fn accepted_alone(self) -> bool {
        matches!(self.0, 1 | 3 | 5 | 10 | 11 | 12 | 16 | 18 | 21 | 25 | 28 | 29 | 30 | 32 | 51 | 52)
    }

    pub const fn accepted(self, enabled: bool, overridden: bool) -> bool {
        overridden || enabled || self.accepted_alone()
    }

    pub const fn qualifies(self, disabled: bool, enabled: bool, fallback: bool) -> bool {
        if disabled || !enabled {
            return false;
        }
        match self.0 { 6 => false, 7 => true, _ => fallback }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowFlags(pub u32);

impl RowFlags {
    pub const fn class(self) -> u8 {
        if self.0 & 1 != 0 { 1 } else if self.0 & 0xa != 0 { 2 } else if self.0 & 4 != 0 { 3 } else { 0 }
    }

    pub const fn bit_two(self) -> bool { self.0 & 4 != 0 }

    pub const fn bit_five(self) -> bool { self.0 & 0x20 != 0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionFlags(pub u8);

impl SelectionFlags {
    pub const fn category(self) -> u8 {
        if self.0 & 0x10 != 0 { 3 } else if self.0 & 1 != 0 { 1 } else { self.0 & 2 }
    }
}

/// Moves `base` by `delta`, clamped to 200 either way, up when `upward`; the result wraps to 16 bits.
pub const fn offset_position(base: i16, delta: i32, upward: bool) -> i16 {
    let step = if delta >= 200 { 200 } else if delta < -200 { -200 } else { delta };
    let moved = if upward { (base as i32).wrapping_add(step) } else { (base as i32).wrapping_sub(step) };
    moved as i16
}

/// Adds `offset` to every value, keeping each at most `limit`; a zero offset changes nothing.
pub fn raise_all(values: &mut [i16], offset: i16, limit: i32) {
    if offset == 0 {
        return;
    }
    for value in values {
        let raised = value.wrapping_add(offset);
        *value = if i32::from(raised) >= limit { limit as i16 } else { raised };
    }
}
