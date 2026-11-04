//! The owned engine at the stock wrapper `0xbf95c8` (`entry.S`). The block and
//! the last-move pair stay the source of truth: every event decodes them, every
//! service call writes the in-progress state's changed words to them first, and
//! the transition's final state is written back before the result returns.

use core::ptr::{read_volatile, write_volatile};

use crate::menu_engine::block::{
    self, decode_words, mirror_last_move_word, mirror_word, BlockWords, RecordTable, BLOCK_ADDRESS, BLOCK_LEN,
    FIRST_DEPTH_SNAPSHOTS, LAST_MOVE_ADDRESS, ROOT_SNAPSHOT, SECOND_DEPTH_SNAPSHOTS, SNAPSHOT_LEN,
};
use crate::menu_engine::engine::{
    apply, Effect, EngineResult, EngineServices, KindSwitchCall, MenuEvent, SeekCall, StageEvent, StageKind, StageNode,
};
use crate::menu_engine::frame::{FrameDepth, MenuClass, MenuFrameSnapshot, NodeId, RowIndex, RowToken};
use crate::menu_engine::state::{EngineState, RowState};
use crate::menu_engine::tree::Tree;
#[allow(unused_imports)]
use crate::menu_engine::{
    block::RecordAddress,
    frame::{NodeType, RowsPerPage},
    tree::{Availability, Label, Node, NodeRef, NodeWord10, Remember, Row, StringId, TreeIdentity, Wrap},
};

// `//replacement/modal_runtime:menu-engine-tree`: `TREE` and `RECORDS`.
include!(env!("RB_MENU_ENGINE_TREE"));
include!(env!("RB_MENU_ENGINE_RECORDS"));

/// The token returned by `FUN_00d588b8` for no selection.
const NO_SELECTION: u32 = 0x7fff_fffe;

unsafe extern "C" {
    /// `FUN_00d52684(class, token)`.
    fn rb_menu_engine_call_availability(class: u32, token: u32) -> u32;
    /// `FUN_00d588b8(kind, node, class)`.
    fn rb_menu_engine_call_initial_row(kind: u32, node: u32, class: u32) -> u32;
    /// `FUN_00d5b350(kind, node, id, class)`.
    fn rb_menu_engine_call_notify(kind: u32, node: u32, id: u32, class: u32);
    /// `FUN_00bfa234(kind, node, id, extra)`.
    fn rb_menu_engine_call_kind_switch(kind: u32, node: u32, id: u32, extra: u32) -> u32;
    /// `FUN_00bfad04(kind, node, type, id, seek, target)`.
    fn rb_menu_engine_call_seek(kind: u32, node: u32, node_type: u32, id: u32, seek: u32, target: u32) -> u32;
    /// The guard trap, a traced site.
    fn rb_menu_engine_guard_failure() -> !;
    /// The bank's page-entry visit at each `FUN_00bfa800` call site.
    fn rb_ui_probe_visit();
}

struct Live;

impl BlockWords for Live {
    fn block_word(&self, offset: usize) -> u32 {
        unsafe { read_volatile((BLOCK_ADDRESS as usize + offset) as *const u32) }
    }

    fn last_move_word(&self, index: usize) -> u32 {
        unsafe { read_volatile((LAST_MOVE_ADDRESS as usize + 4 * index) as *const u32) }
    }
}

fn store_word(address: u32, value: u32) {
    unsafe { write_volatile(address as usize as *mut u32, value) }
}

/// Writes the words of `state` that differ from the block and the last-move
/// pair.
fn write_state(state: &EngineState) {
    for offset in (0..BLOCK_LEN).step_by(4) {
        let value = mirror_word(state, &RECORDS, offset);
        if Live.block_word(offset) != value {
            store_word(BLOCK_ADDRESS + offset as u32, value);
        }
    }
    for index in 0..2 {
        let value = mirror_last_move_word(state, index);
        if Live.last_move_word(index) != value {
            store_word(LAST_MOVE_ADDRESS + 4 * index as u32, value);
        }
    }
}

fn read_state() -> EngineState {
    match decode_words(&Live, &TREE, &RECORDS) {
        Ok(state) => state,
        Err(_) => unsafe { rb_menu_engine_guard_failure() },
    }
}

fn result_of(raw: u32) -> EngineResult {
    match raw {
        0 => EngineResult::Rejected,
        1 => EngineResult::Accepted,
        _ => unsafe { rb_menu_engine_guard_failure() },
    }
}

/// The owned frame writer's stores: the detached defaults, then the record.
/// Inlined so each store keeps its depth's constant base.
#[inline(always)]
fn write_record(address: u32, record: &MenuFrameSnapshot) {
    for words in [MenuFrameSnapshot::DETACHED.words(), record.words()].iter() {
        for (index, word) in words.iter().enumerate() {
            store_word(address.wrapping_add(4 * index as u32), *word);
        }
    }
}

struct Stock;

impl EngineServices for Stock {
    fn perform(&mut self, state: &EngineState, effect: Effect) {
        write_state(state);
        match effect {
            Effect::Notify(notify) => unsafe {
                rb_menu_engine_call_notify(notify.kind.raw(), notify.node.get(), notify.id.get(), notify.class.get())
            },
            Effect::StoreSnapshot { depth, slot, record } => {
                let offset = slot.get().wrapping_mul(SNAPSHOT_LEN);
                match depth {
                    FrameDepth::Root => write_record(ROOT_SNAPSHOT, &record),
                    FrameDepth::First => write_record(FIRST_DEPTH_SNAPSHOTS.wrapping_add(offset), &record),
                    FrameDepth::Second => write_record(SECOND_DEPTH_SNAPSHOTS.wrapping_add(offset), &record),
                }
            }
            Effect::StoreFromBlock { slot } => {
                crate::owned_menu::frame::rb_menu_frame_store(NodeId::new(slot.get()));
            }
            Effect::PageEntry => unsafe { rb_ui_probe_visit() },
        }
    }

    fn availability(&mut self, state: &EngineState, class: MenuClass, token: RowToken) -> RowState {
        write_state(state);
        let raw = unsafe { rb_menu_engine_call_availability(class.get(), token.get()) };
        RowState::parse(raw).unwrap_or_else(|| unsafe { rb_menu_engine_guard_failure() })
    }

    fn initial_row(&mut self, state: &EngineState, kind: StageKind, node: StageNode, class: MenuClass) -> Option<RowToken> {
        write_state(state);
        match unsafe { rb_menu_engine_call_initial_row(kind.raw(), node.get(), class.get()) } {
            NO_SELECTION => None,
            token => Some(RowToken::new(token)),
        }
    }

    fn token_past_rows(&mut self, state: &EngineState, row: RowIndex) -> RowToken {
        let values = match state.node.and_then(|node| block::values_pointer_address(&RECORDS, node)) {
            Some(field) => unsafe { read_volatile(field as usize as *const u32) },
            None => unsafe { rb_menu_engine_guard_failure() },
        };
        let address = values.wrapping_add(row.get().wrapping_mul(8));
        RowToken::new(unsafe { read_volatile(address as usize as *const u32) })
    }

    fn kind_switch(&mut self, state: &EngineState, call: KindSwitchCall) -> (EngineState, EngineResult) {
        write_state(state);
        let raw = unsafe { rb_menu_engine_call_kind_switch(call.kind.raw(), call.node.get(), call.id.get(), call.extra.get()) };
        (read_state(), result_of(raw))
    }

    fn seek(&mut self, state: &EngineState, call: SeekCall) -> (EngineState, EngineResult) {
        write_state(state);
        let raw = unsafe {
            rb_menu_engine_call_seek(
                call.kind.raw(),
                call.node.get(),
                call.node_type.get(),
                call.id.get(),
                call.seek.get(),
                call.target.get(),
            )
        };
        (read_state(), result_of(raw))
    }

    fn invariant_failure(&mut self) -> ! {
        unsafe { rb_menu_engine_guard_failure() }
    }
}

/// Returns 0 when the event falls back to stock, otherwise 1 + the result
/// `FUN_00bf95c8` returns.
#[no_mangle]
pub extern "C" fn rb_menu_engine_apply(kind: u32, node: u32, id: u32, extra: u32) -> u32 {
    let Ok(state) = decode_words(&Live, &TREE, &RECORDS) else {
        return 0;
    };
    let event = MenuEvent::decode(StageEvent { kind, node, id, extra }, &TREE, &state);
    match apply(&TREE, &state, event, &mut Stock) {
        Err(_) => 0,
        Ok(transition) => {
            write_state(&transition.state);
            match transition.result {
                EngineResult::Rejected => 1,
                EngineResult::Accepted => 2,
            }
        }
    }
}

const _: () = assert!(block::BLOCK_LEN % 4 == 0);
