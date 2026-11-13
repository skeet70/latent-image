//! Decode from and mirror to the stock engine block and last-move pair. The
//! only module that knows stock addresses.

use core::hint::black_box;

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

pub const ROOT_SNAPSHOT: u32 = 0x0094_4414;
pub const FIRST_DEPTH_SNAPSHOTS: u32 = 0x0093_eb74;
pub const SECOND_DEPTH_SNAPSHOTS: u32 = 0x0094_12d4;
pub const SNAPSHOT_LEN: u32 = 0x2c;

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

    fn address(&self, node: NodeRef) -> Option<RecordAddress> {
        self.records.get(node.index()).copied()
    }

    fn node(&self, address: RecordAddress) -> Option<NodeRef> {
        self.records.iter().position(|&record| record == address).map(|index| NodeRef::new(index as u16))
    }
}

/// Word `+0x20` of a node record.
pub fn values_pointer_address(records: &RecordTable, node: NodeRef) -> Option<u32> {
    records.address(node).map(|record| record.get().wrapping_add(0x20))
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

fn cursor_frame<B: BlockWords>(block: &B, offset: usize) -> CursorFrame {
    CursorFrame {
        cursor: RowIndex::new(block.block_word(offset)),
        ordinal: VisibleOrdinal::new(block.block_word(offset + 4)),
        count: VisibleCount::new(block.block_word(offset + 8)),
        scroll: ScrollTop::new(block.block_word(offset + 12)),
    }
}

fn cursor_frame_word(frame: CursorFrame, offset: usize) -> u32 {
    match offset {
        0 => frame.cursor.get(),
        4 => frame.ordinal.get(),
        8 => frame.count.get(),
        _ => frame.scroll.get(),
    }
}

/// Word reads of the block and the last-move pair.
pub trait BlockWords {
    /// The word at a 4-aligned offset below `BLOCK_LEN`.
    fn block_word(&self, offset: usize) -> u32;
    /// Word 0 (class) or 1 (token) of the last-move pair.
    fn last_move_word(&self, index: usize) -> u32;
}

struct Images<'a>(&'a [u8; BLOCK_LEN], &'a [u8; LAST_MOVE_LEN]);

impl BlockWords for Images<'_> {
    fn block_word(&self, offset: usize) -> u32 {
        word(self.0, offset)
    }

    fn last_move_word(&self, index: usize) -> u32 {
        word(self.1, 4 * index)
    }
}

pub fn decode(
    block: &[u8; BLOCK_LEN],
    last_move: &[u8; LAST_MOVE_LEN],
    tree: &Tree,
    records: &RecordTable,
) -> Result<EngineState, DecodeError> {
    decode_words(&Images(block, last_move), tree, records)
}

#[inline]
pub fn decode_words<B: BlockWords>(block: &B, tree: &Tree, records: &RecordTable) -> Result<EngineState, DecodeError> {
    let mut state = EngineState::EMPTY;
    decode_into(block, tree, records, &mut state)?;
    Ok(state)
}

/// On an error `state` is partly written.
#[inline]
pub fn decode_into<B: BlockWords>(
    block: &B,
    tree: &Tree,
    records: &RecordTable,
    state: &mut EngineState,
) -> Result<(), DecodeError> {
    if !records.serves(tree) {
        return Err(DecodeError::TableMismatch);
    }
    state.node = match block.block_word(NODE) {
        0 => None,
        raw => {
            let address = RecordAddress::new(raw);
            Some(records.node(address).ok_or(DecodeError::UnknownRecord(address))?)
        }
    };

    for row in 0..ROW_STATES {
        let raw = block.block_word(ROW_STATE_WORDS + 4 * row);
        let parsed = RowState::parse(raw).ok_or(DecodeError::RowState { row, raw })?;
        if let Some(slot) = state.row_states.get_mut(black_box(row)) {
            *slot = parsed;
        }
    }
    for index in 0..REGISTRY_SLOTS {
        if let Some(slot) = state.remembered.get_mut(black_box(index)) {
            *slot = RowToken::new(block.block_word(REMEMBERED + 4 * index));
        }
    }
    for index in 0..STACK_FRAMES {
        if let Some(slot) = state.stack.get_mut(black_box(index)) {
            *slot = cursor_frame(block, STACK + 16 * index);
        }
    }
    let classifier = block.block_word(PUSH_CLASSIFIER);
    let open = block.block_word(MENU_OPEN);
    let word10 = block.block_word(WORD10);
    state.word10 = NodeWord10::new(word10 as u16);
    state.word10_high = ((word10 >> 16) as u16).to_le_bytes();
    state.live = cursor_frame(block, LIVE);
    state.total = RowCount::new(block.block_word(TOTAL));
    state.visible_at_open = VisibleCount::new(block.block_word(VISIBLE_AT_OPEN));
    state.saved = cursor_frame(block, SAVED);
    state.depth = StackDepth::new(block.block_word(DEPTH));
    state.push_classifier = FrameDepth::parse(classifier).ok_or(DecodeError::PushClassifier(classifier))?;
    state.menu_open = MenuOpen::parse(open).ok_or(DecodeError::MenuOpen(open))?;
    state.last_move = LastMove { class: MenuClass::new(block.last_move_word(0)), token: RowToken::new(block.last_move_word(1)) };
    Ok(())
}

/// The block word at a 4-aligned offset below `BLOCK_LEN`.
pub fn mirror_word(state: &EngineState, records: &RecordTable, offset: usize) -> u32 {
    match offset {
        NODE => state.node.and_then(|node| records.address(node)).map_or(0, RecordAddress::get),
        WORD10 => u32::from(state.word10.get()) | (u32::from(u16::from_le_bytes(state.word10_high)) << 16),
        TOTAL => state.total.get(),
        VISIBLE_AT_OPEN => state.visible_at_open.get(),
        DEPTH => state.depth.get(),
        PUSH_CLASSIFIER => state.push_classifier.raw(),
        MENU_OPEN => state.menu_open.raw(),
        _ if (LIVE..TOTAL).contains(&offset) => cursor_frame_word(state.live, offset - LIVE),
        _ if (ROW_STATE_WORDS..SAVED).contains(&offset) => state.row_states.get((offset - ROW_STATE_WORDS) / 4).map_or(0, |row| row.raw()),
        _ if (SAVED..REMEMBERED).contains(&offset) => cursor_frame_word(state.saved, offset - SAVED),
        _ if (REMEMBERED..STACK).contains(&offset) => state.remembered.get((offset - REMEMBERED) / 4).map_or(0, |token| token.get()),
        _ => state.stack.get((offset - STACK) / 16).map_or(0, |frame| cursor_frame_word(*frame, (offset - STACK) % 16)),
    }
}

/// Word 0 (class) or 1 (token) of the last-move pair.
pub fn mirror_last_move_word(state: &EngineState, index: usize) -> u32 {
    if index == 0 { state.last_move.class.get() } else { state.last_move.token.get() }
}

/// The class and token `state` differs from `current` in, each `None` where
/// the pair already holds it.
#[inline]
pub fn last_move_writes(state: &EngineState, current: [u32; 2]) -> (Option<u32>, Option<u32>) {
    let class = mirror_last_move_word(state, 0);
    let token = mirror_last_move_word(state, 1);
    ((class != current[0]).then_some(class), (token != current[1]).then_some(token))
}

pub fn mirror(state: &EngineState, records: &RecordTable) -> ([u8; BLOCK_LEN], [u8; LAST_MOVE_LEN]) {
    let mut block = [0u8; BLOCK_LEN];
    for offset in (0..BLOCK_LEN).step_by(4) {
        put(&mut block, offset, mirror_word(state, records, offset));
    }
    let mut last_move = [0u8; LAST_MOVE_LEN];
    put(&mut last_move, 0, mirror_last_move_word(state, 0));
    put(&mut last_move, 4, mirror_last_move_word(state, 1));
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
