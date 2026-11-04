//! Decode from and mirror to the stock engine block and last-move pair. The
//! only module that knows stock addresses.

use super::engine::SnapshotSlot;
use super::frame::{FrameDepth, MenuClass, RowIndex, RowToken, ScrollTop, VisibleCount, VisibleOrdinal};
use super::state::{
    CursorFrame, EngineState, LastMove, MenuOpen, RowCount, RowState, StackDepth, ROW_STATES, STACK_FRAMES,
};
use super::tree::{NodeRef, NodeWord10, Tree, TreeIdentity, REGISTRY_SLOTS};

pub const BLOCK_ADDRESS: u32 = 0x0095_71d0;
pub const BLOCK_LEN: usize = 0x430;
pub const LAST_MOVE_ADDRESS: u32 = 0x0146_7580;
pub const LAST_MOVE_LEN: usize = 8;

const NODE: usize = 0x00;
const WORD10: usize = 0x04;
const WORD10_HIGH: usize = 0x06;
const LIVE: usize = 0x08;
const TOTAL: usize = 0x18;
const VISIBLE_AT_OPEN: usize = 0x1c;
const ROW_STATE_WORDS: usize = 0x20;
const SAVED: usize = 0x15c;
const REMEMBERED: usize = 0x16c;
const STACK: usize = 0x384;
const DEPTH: usize = 0x424;
const PUSH_CLASSIFIER: usize = 0x428;
const MENU_OPEN: usize = 0x42c;

const _: () = assert!(ROW_STATE_WORDS + 4 * ROW_STATES == SAVED);
const _: () = assert!(REMEMBERED + 4 * REGISTRY_SLOTS == STACK);
const _: () = assert!(STACK + 16 * STACK_FRAMES == DEPTH);
const _: () = assert!(MENU_OPEN + 4 == BLOCK_LEN);

const ROOT_SNAPSHOT: u32 = 0x0094_4414;
const FIRST_DEPTH_SNAPSHOTS: u32 = 0x0093_eb74;
const SECOND_DEPTH_SNAPSHOTS: u32 = 0x0094_12d4;
const SNAPSHOT_LEN: u32 = 0x2c;

/// A stock node record address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecordAddress(u32);

impl RecordAddress {
    pub const fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Record address of each tree node, indexed by `NodeRef`.
#[derive(Clone, Copy, Debug)]
pub struct RecordTable {
    pub tree: TreeIdentity,
    pub records: &'static [RecordAddress],
}

impl RecordTable {
    pub fn serves(&self, tree: &Tree) -> bool {
        self.tree == tree.identity && self.records.len() == tree.nodes.len()
    }

    fn address(&self, node: NodeRef) -> RecordAddress {
        self.records[node.index()]
    }

    fn node(&self, address: RecordAddress) -> Option<NodeRef> {
        self.records.iter().position(|&record| record == address).map(|index| NodeRef::new(index as u16))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeError {
    TableMismatch,
    UnknownRecord(RecordAddress),
    RowState { row: usize, raw: u32 },
    PushClassifier(u32),
    MenuOpen(u32),
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]])
}

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn cursor_frame(bytes: &[u8], offset: usize) -> CursorFrame {
    CursorFrame {
        cursor: RowIndex::new(word(bytes, offset)),
        ordinal: VisibleOrdinal::new(word(bytes, offset + 4)),
        count: VisibleCount::new(word(bytes, offset + 8)),
        scroll: ScrollTop::new(word(bytes, offset + 12)),
    }
}

fn put_cursor_frame(bytes: &mut [u8], offset: usize, frame: CursorFrame) {
    put(bytes, offset, frame.cursor.get());
    put(bytes, offset + 4, frame.ordinal.get());
    put(bytes, offset + 8, frame.count.get());
    put(bytes, offset + 12, frame.scroll.get());
}

pub fn decode(
    block: &[u8; BLOCK_LEN],
    last_move: &[u8; LAST_MOVE_LEN],
    tree: &Tree,
    records: &RecordTable,
) -> Result<EngineState, DecodeError> {
    if !records.serves(tree) {
        return Err(DecodeError::TableMismatch);
    }
    let node = match word(block, NODE) {
        0 => None,
        raw => {
            let address = RecordAddress::new(raw);
            Some(records.node(address).ok_or(DecodeError::UnknownRecord(address))?)
        }
    };
    let mut row_states = [RowState::Offered; ROW_STATES];
    for (row, state) in row_states.iter_mut().enumerate() {
        let raw = word(block, ROW_STATE_WORDS + 4 * row);
        *state = RowState::parse(raw).ok_or(DecodeError::RowState { row, raw })?;
    }
    let mut remembered = [RowToken::new(0); REGISTRY_SLOTS];
    for (index, token) in remembered.iter_mut().enumerate() {
        *token = RowToken::new(word(block, REMEMBERED + 4 * index));
    }
    let mut stack = [CursorFrame::ZERO; STACK_FRAMES];
    for (index, frame) in stack.iter_mut().enumerate() {
        *frame = cursor_frame(block, STACK + 16 * index);
    }
    let classifier = word(block, PUSH_CLASSIFIER);
    let open = word(block, MENU_OPEN);
    Ok(EngineState {
        node,
        word10: NodeWord10::new(u16::from_le_bytes([block[WORD10], block[WORD10 + 1]])),
        word10_high: [block[WORD10_HIGH], block[WORD10_HIGH + 1]],
        live: cursor_frame(block, LIVE),
        total: RowCount::new(word(block, TOTAL)),
        visible_at_open: VisibleCount::new(word(block, VISIBLE_AT_OPEN)),
        row_states,
        saved: cursor_frame(block, SAVED),
        remembered,
        stack,
        depth: StackDepth::new(word(block, DEPTH)),
        push_classifier: FrameDepth::parse(classifier).ok_or(DecodeError::PushClassifier(classifier))?,
        menu_open: MenuOpen::parse(open).ok_or(DecodeError::MenuOpen(open))?,
        last_move: LastMove { class: MenuClass::new(word(last_move, 0)), token: RowToken::new(word(last_move, 4)) },
    })
}

pub fn mirror(state: &EngineState, records: &RecordTable) -> ([u8; BLOCK_LEN], [u8; LAST_MOVE_LEN]) {
    let mut block = [0u8; BLOCK_LEN];
    put(&mut block, NODE, state.node.map_or(0, |node| records.address(node).get()));
    block[WORD10..WORD10 + 2].copy_from_slice(&state.word10.get().to_le_bytes());
    block[WORD10_HIGH..WORD10_HIGH + 2].copy_from_slice(&state.word10_high);
    put_cursor_frame(&mut block, LIVE, state.live);
    put(&mut block, TOTAL, state.total.get());
    put(&mut block, VISIBLE_AT_OPEN, state.visible_at_open.get());
    for (row, row_state) in state.row_states.iter().enumerate() {
        put(&mut block, ROW_STATE_WORDS + 4 * row, row_state.raw());
    }
    put_cursor_frame(&mut block, SAVED, state.saved);
    for (index, token) in state.remembered.iter().enumerate() {
        put(&mut block, REMEMBERED + 4 * index, token.get());
    }
    for (index, frame) in state.stack.iter().enumerate() {
        put_cursor_frame(&mut block, STACK + 16 * index, *frame);
    }
    put(&mut block, DEPTH, state.depth.get());
    put(&mut block, PUSH_CLASSIFIER, state.push_classifier.raw());
    put(&mut block, MENU_OPEN, state.menu_open.raw());
    let mut last_move = [0u8; LAST_MOVE_LEN];
    put(&mut last_move, 0, state.last_move.class.get());
    put(&mut last_move, 4, state.last_move.token.get());
    (block, last_move)
}

/// The record a `StoreSnapshot` effect writes, as `FUN_0061ceac` indexes it:
/// unbounded at depths one and two.
pub fn snapshot_address(depth: FrameDepth, slot: SnapshotSlot) -> u32 {
    match depth {
        FrameDepth::Root => ROOT_SNAPSHOT,
        FrameDepth::First => FIRST_DEPTH_SNAPSHOTS.wrapping_add(slot.get().wrapping_mul(SNAPSHOT_LEN)),
        FrameDepth::Second => SECOND_DEPTH_SNAPSHOTS.wrapping_add(slot.get().wrapping_mul(SNAPSHOT_LEN)),
    }
}
