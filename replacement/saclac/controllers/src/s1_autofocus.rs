#![forbid(unsafe_code)]

//! What an autofocus command changes for the running S1 measurement, the lens pair the profile
//! keeps for it, and the lens move toward a changed target.

use crate::layers::{
    Block, Change, CommandParameters, CurveQuery, Field, LensControl, LensCurve, LensMove, LensProbe, LensSlot,
    ProfileRecord, RemapRequest, S1Values,
};
use crate::s1_statistics::AutofocusChange;
use crate::s1_statistics_profile::{block, read, Layers};
use crate::stock;
use xm1_saclac_interface::{CommandKind as K, RecordHandle};

/// The autofocus parameter word the lens pair starts from.
const LENS_WORD: usize = 38;

const BOUNDS_DETAIL: u32 = 1;

fn changed(layers: &mut impl Layers, record: RecordHandle, change: Change) -> bool { layers.profile().changed(record, change) }

fn base(layers: &mut impl Layers) -> u32 { layers.parameters().word(K::Autofocus, LENS_WORD) }

fn write_pair(layers: &mut impl Layers, record: RecordHandle, slot: LensSlot, pair: [u32; 2]) {
    layers.profile().write_block(record, Block::Lens(slot), &pair);
}

/// Whether the pair maps to another table value than `held`, or the selection record no longer
/// tracks one.
fn selection_moved(layers: &mut impl Layers, record: RecordHandle, held: [u32; 2], pair: [u32; 2]) -> bool {
    let held = layers.curve().mapped_value(held[1], held[0]);
    held != layers.curve().mapped_value(pair[1], pair[0]) || read(layers, record, Field::SelectionTracked) == 0
}

/// Classifies the autofocus change (0xf23cb8) and keeps the lens pair it implies.
pub(crate) fn classify(record: RecordHandle, layers: &mut impl Layers) -> AutofocusChange {
    let retune = changed(layers, record, Change::Retune);
    let change = if read(layers, record, Field::Selection) == 1 {
        AutofocusChange::None
    } else if retune {
        AutofocusChange::Retune
    } else if changed(layers, record, Change::LensRange) {
        follow_range(record, layers);
        return AutofocusChange::Lens;
    } else if changed(layers, record, Change::LensWindow) || changed(layers, record, Change::LensValue) {
        if follow_search(record, layers) {
            return AutofocusChange::Lens;
        }
        layers.mark_selection_changed();
        AutofocusChange::None
    } else {
        AutofocusChange::None
    };
    keep_base(record, layers);
    change
}

/// The profile's lens range changed: notices the lens bounds while the target has steps, then
/// takes the pair the range gives.
fn follow_range(record: RecordHandle, layers: &mut impl Layers) {
    let [target]: [u32; 1] = block(layers, record, Block::LensTarget);
    let base = base(layers);
    let range = read(layers, record, Field::LensRange);
    let steps = (target >> 16) as u16;
    if steps != 0 {
        notify_bounds(steps, base, range, record, layers);
    }
    let current: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Current));
    if range != layers.curve().combine_lens_pair(current[0], current[1], base) {
        let pair = [base, layers.curve().combine_lens_pair(base, range, base)];
        write_pair(layers, record, LensSlot::Current, pair);
        write_pair(layers, record, LensSlot::Next, pair);
        layers.mark_selection_changed();
        return;
    }
    let pair = [base, layers.curve().combine_lens_pair(base, range, base)];
    let next: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Next));
    if selection_moved(layers, record, next, pair) {
        layers.mark_selection_changed();
    }
    write_pair(layers, record, LensSlot::Next, pair);
}

/// The lens window or value changed: takes the searched pair when it differs from the next
/// pair's. Returns whether it did.
fn follow_search(record: RecordHandle, layers: &mut impl Layers) -> bool {
    let base = base(layers);
    let next: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Next));
    let held = layers.curve().combine_lens_pair(next[0], next[1], next[0]);
    let searched = layers.values().searched_lens_pair(record);
    if searched == held {
        return false;
    }
    write_pair(layers, record, LensSlot::Current, [base, searched]);
    write_pair(layers, record, LensSlot::Next, [base, searched]);
    layers.mark_selection_changed();
    true
}

/// Restarts the next pair from the autofocus base and tells the round when it moved the
/// selection off the reported pair.
fn keep_base(record: RecordHandle, layers: &mut impl Layers) {
    let mut next: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Next));
    next[0] = base(layers);
    write_pair(layers, record, LensSlot::Next, next);
    let reported: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Reported));
    if selection_moved(layers, record, reported, next) {
        layers.mark_selection_changed();
    }
}

/// Hands the notifier the lens curve positions the target's steps reach (0xf23bc4). They stay
/// unset (all ones) while either lens probe is overridden.
fn notify_bounds(steps: u16, base: u32, range: u32, record: RecordHandle, layers: &mut impl Layers) {
    let restarts = read(layers, record, Field::Command1dRestarts);
    let reference = layers.lens().reference();
    let mode_index = layers.lens().mode_index();
    let level = layers.curve().level(steps, base, reference, mode_index);
    let mut bounds = [u32::MAX; 3];
    let overridden = layers.lens().overridden(LensProbe::Second) || layers.lens().overridden(LensProbe::First);
    if !overridden {
        let query = CurveQuery { range, reference, mode_index, step: layers.lens().step() };
        if !layers.curve().interpolates() && restarts == 0 {
            bounds = layers.curve().positions(level, base, query);
        } else {
            let (channel, fraction) = layers.lens().channel();
            if let Some(positions) = layers.curve().interpolated_positions(level, channel, fraction, query) {
                bounds = positions;
            }
        }
    }
    layers.notify_bounds(BOUNDS_DETAIL, bounds);
}

/// Moves the lens to the changed target (0xf24a94); true when no move is needed.
pub(crate) fn move_lens(record: RecordHandle, layers: &mut impl Layers) -> bool {
    let next: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Next));
    let reference = read(layers, record, Field::LensReference);
    let restarts = read(layers, record, Field::Command1dRestarts);
    let [target]: [u32; 1] = block(layers, record, Block::LensTarget);
    let Some(position) = target_position(target as u16 as i16, next[1], reference, record, layers) else {
        return true;
    };
    let steps = u32::from(position).wrapping_sub(layers.lens().position()) as i16;
    let mode_index = layers.lens().mode_index();
    let settle = if restarts == 0 { 11 } else { 0 };
    layers.move_lens(LensMove { callback: stock::S1_LENS_MOVED, steps, mode_index, words: [1, 0, settle, 4] });
    false
}

/// Returns the live cancellation's lens toward its current pair (0xf251e4).
/// `base` is the lens service's current resource, not its validated configuration index.
pub fn correct_drive(record: RecordHandle, base: u32, callback: xm1_saclac_interface::stock::StockCallbackWord, layers: &mut impl Layers) -> bool {
    let current: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Current));
    let target = layers.curve().combine_lens_pair(current[0], current[1], base);
    let next: [u32; 2] = block(layers, record, Block::Lens(LensSlot::Next));
    let current_value = layers.curve().mapped_value(current[1], current[0]);
    if current_value != layers.curve().mapped_value(next[1], next[0]) {
        layers.mark_selection_changed();
    }
    write_pair(layers, record, LensSlot::Next, [base, target]);
    if target == next[1] { return true; }
    let restarts = read(layers, record, Field::Command1dRestarts);
    let [held]: [u32; 1] = block(layers, record, Block::LensTarget);
    let Some(position) = target_position(held as u16 as i16, target, base, record, layers) else {
        return true;
    };
    let steps = u32::from(position).wrapping_sub(layers.lens().position()) as i16;
    let mode_index = layers.lens().mode_index();
    layers.move_lens(LensMove { callback, steps, mode_index, words: [1, 0, if restarts == 0 { 11 } else { 0 }, 4] });
    false
}

/// The lens target's position, remapped when the release range asks for it; `None` in the
/// unselected profile mode without a lens designation (0xf240b0).
fn target_position(position: i16, target: u32, reference: u32, record: RecordHandle, layers: &mut impl Layers) -> Option<u16> {
    let designation = read(layers, record, Field::LensDesignation);
    if layers.values().unselected(record) == 1 && designation == 0 {
        return None;
    }
    let release: [u32; 12] = block(layers, record, Block::ReleaseRange);
    if release[8] != 1 {
        return Some(position as u16);
    }
    let convert = read(layers, record, Field::LensConversion) == 1;
    let mode_index = layers.lens().mode_index();
    Some(layers.remap_lens_target(RemapRequest { position, reference, target, convert, mode_index }))
}
