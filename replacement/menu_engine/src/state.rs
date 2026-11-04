//! The typed form of the engine block `[0x009571d0,0x00957600)` and the
//! last-move pair `0x01467580`/`0x01467584`.

use super::frame::{word, FrameDepth, MenuClass, RowIndex, RowToken, ScrollTop, VisibleCount, VisibleOrdinal};
use super::tree::{NodeRef, NodeWord10, REGISTRY_SLOTS};

pub const ROW_STATES: usize = 0x4f;
pub const STACK_FRAMES: usize = 10;

/// A row state from `FUN_00d52684`; the engine selects rows below `Disabled`.
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Offered = 0,
    Variant = 1,
    Disabled = 2,
    Hidden = 3,
}

impl RowState {
    pub const fn parse(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Offered),
            1 => Some(Self::Variant),
            2 => Some(Self::Disabled),
            3 => Some(Self::Hidden),
            _ => None,
        }
    }

    pub const fn raw(self) -> u32 {
        self as u32
    }
}

word!(
    /// Rows of the open node, `0x009571e8`.
    RowCount
);
word!(
    /// Index into `EngineState::stack`, `0x009575f4`.
    StackDepth
);

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuOpen {
    Closed = 0,
    Open = 1,
}

impl MenuOpen {
    pub const fn parse(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Closed),
            1 => Some(Self::Open),
            _ => None,
        }
    }

    pub const fn raw(self) -> u32 {
        self as u32
    }
}

/// Cursor, visible ordinal, visible count and scroll top: the live words at
/// `0x009571d8`, the saved copy at `0x0095732c` and each cursor-stack entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorFrame {
    pub cursor: RowIndex,
    pub ordinal: VisibleOrdinal,
    pub count: VisibleCount,
    pub scroll: ScrollTop,
}

impl CursorFrame {
    pub const ZERO: Self = Self {
        cursor: RowIndex::new(0),
        ordinal: VisibleOrdinal::new(0),
        count: VisibleCount::new(0),
        scroll: ScrollTop::new(0),
    };
}

/// The moved row's class and cursor token, written by `0xbf9f44`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LastMove {
    pub class: MenuClass,
    pub token: RowToken,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EngineState {
    pub node: Option<NodeRef>,
    pub word10: NodeWord10,
    /// Bytes `0x009571d6..0x009571d8`, which the engine never writes.
    pub word10_high: [u8; 2],
    pub live: CursorFrame,
    pub total: RowCount,
    pub visible_at_open: VisibleCount,
    pub row_states: [RowState; ROW_STATES],
    pub saved: CursorFrame,
    /// Row tokens by registry index.
    pub remembered: [RowToken; REGISTRY_SLOTS],
    pub stack: [CursorFrame; STACK_FRAMES],
    pub depth: StackDepth,
    pub push_classifier: FrameDepth,
    pub menu_open: MenuOpen,
    pub last_move: LastMove,
}
