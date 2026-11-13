#![forbid(unsafe_code)]

//! The capture dump of a finished capture: the bound record, the shot context's lens marks and
//! the calc library's workspaces with the adjustment bytes they depend on, then the marks of
//! the secondary job's trace area, the configuration's part and the capture pipeline's work
//! storage. `capture_stock::CAPTURE_DUMP` lays the first part out.

use crate::boundary::Frame;
use crate::capture_boundary::{
    CaptureClient, CaptureParameters, CaptureRecords, FrameOffset, FrameTable, LensGeometry, RecordField, RemapRequest, SlotField,
};
use crate::capture_stock;
use crate::dump_boundary::{AdjustmentKey, DumpSettings, DumpShared, DumpWorkspaces, RateGrid, ShotWord, Workspace, WorkspaceField};
use crate::secondary_boundary::MeasurementCalc;
use xm1_saclac_interface::RecordHandle;

pub trait DumpBoundary: CaptureRecords + CaptureClient + LensGeometry + MeasurementCalc + DumpWorkspaces + DumpSettings + DumpShared {}
impl<T> DumpBoundary for T where T: CaptureRecords + CaptureClient + LensGeometry + MeasurementCalc + DumpWorkspaces + DumpSettings + DumpShared {}

/// How the dump packs adjustment bytes into a word, from the byte at the key onward.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Packing {
    Byte,
    /// Two bytes, the first in bits 8..16.
    Pair,
    /// Two bytes, the first in bits 16..24.
    Spread,
    /// Four bytes, the first in the top byte.
    Quad,
    /// Two bytes, the first in the low byte.
    Half,
    Tenfold,
}

/// What one of the dump's frame words holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DumpWord {
    Constant(u32),
    /// Zero; stock leaves bytes of its stack in four of these words.
    Zero,
    Record(RecordField),
    FirstSlot(SlotField),
    /// The record's subject flags; when the record names a subject area, bit 0 is whether the
    /// area reference's fifth word is 1 and the flags keep 16 bits.
    SubjectFlags,
    /// The output marker the primary job left in the client's selection.
    OutputMarker,
    Shot(ShotWord),
    CaptureOffset,
    SettingsClass,
    Adjustment(AdjustmentKey, Packing),
    /// How many entries a workspace list holds: the adjustment byte, 1 for zero, at most 20.
    EntryCount(AdjustmentKey),
    Workspace(Workspace, WorkspaceField),
    /// The grid's lens position remapped at the grid's reference and state, less the position.
    GridRemap(RateGrid),
    GridNextReference(RateGrid),
}

/// What a run of the dump's frame words holds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DumpSource {
    Word(DumpWord),
    /// Two words: the halves of the record's layout.
    Layout,
    /// Two words: the capture window of the record's subject mode at its layout, zero without
    /// one.
    CaptureWindow,
    /// `count` words from fields `stride` bytes apart.
    Run { workspace: Workspace, field: WorkspaceField, stride: u32, count: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DumpEntry {
    pub at: FrameOffset,
    pub source: DumpSource,
}

/// The secondary mode whose capture leaves the secondary job's trace area unmarked.
const UNTRACED_MODE: u32 = 6;
const MAX_ENTRIES: u8 = 20;
/// The lens kind the grids' positions are remapped at.
const REMAP_KIND: u32 = 1;

/// `output` is the primary job's last output word.
pub fn write(record: RecordHandle, parameters: &CaptureParameters, output: u32, boundary: &mut impl DumpBoundary) {
    let frame = boundary.frame(parameters.sources);
    for entry in capture_stock::CAPTURE_DUMP {
        write_entry(frame, entry, record, parameters, boundary);
    }
    if boundary.field(record, RecordField::SecondaryMode) != UNTRACED_MODE {
        let (start, end) = capture_stock::TRACE_MARKS;
        boundary.set_frame_word(frame, capture_stock::TRACE_AREA, start);
        boundary.set_frame_word(frame, capture_stock::TRACE_OUTPUT, output);
        boundary.set_frame_word(frame, capture_stock::TRACE_END, end);
    }
    let distance = boundary.slot_field(record, 0, SlotField::Distance);
    boundary.export_configuration(frame, distance);
    let (start, end) = capture_stock::PIPELINE_MARKS;
    boundary.set_frame_word(frame, capture_stock::PIPELINE_DUMP, start);
    boundary.copy_table(frame, capture_stock::PIPELINE_WORK, FrameTable::PipelineWork);
    boundary.set_frame_word(frame, capture_stock::PIPELINE_END, end);
}

fn write_entry(frame: Frame, entry: DumpEntry, record: RecordHandle, parameters: &CaptureParameters, boundary: &mut impl DumpBoundary) {
    let put = |boundary: &mut _, index: u32, value: u32| {
        CaptureClient::set_frame_word(boundary, frame, FrameOffset(entry.at.0 + 4 * index), value);
    };
    match entry.source {
        DumpSource::Word(word) => {
            let value = word_value(word, record, parameters, boundary);
            put(boundary, 0, value);
        }
        DumpSource::Layout => {
            let halves = layout(record, boundary);
            for (index, half) in (0..).zip(halves) { put(boundary, index, u32::from(half)); }
        }
        DumpSource::CaptureWindow => {
            let words = capture_window(record, boundary).unwrap_or_default();
            for (index, word) in (0..).zip(words) { put(boundary, index, word); }
        }
        DumpSource::Run { workspace, field, stride, count } => {
            for index in 0..count {
                let value = boundary.workspace_field(workspace, field.advanced(stride * index));
                put(boundary, index, value);
            }
        }
    }
}

fn word_value(word: DumpWord, record: RecordHandle, parameters: &CaptureParameters, boundary: &mut impl DumpBoundary) -> u32 {
    match word {
        DumpWord::Constant(value) => value,
        DumpWord::Zero => 0,
        DumpWord::Record(field) => boundary.field(record, field),
        DumpWord::FirstSlot(field) => boundary.slot_field(record, 0, field),
        DumpWord::SubjectFlags => subject_flags(record, boundary),
        DumpWord::OutputMarker => i32::from(boundary.output_marker(parameters.selection)) as u32,
        DumpWord::Shot(word) => boundary.shot_word(word),
        DumpWord::CaptureOffset => boundary.capture_offset(record),
        DumpWord::SettingsClass => boundary.settings_class(),
        DumpWord::Adjustment(key, packing) => packed(key, packing, boundary),
        DumpWord::EntryCount(key) => u32::from(boundary.adjustment_byte(key).clamp(1, MAX_ENTRIES)),
        DumpWord::Workspace(workspace, field) => boundary.workspace_field(workspace, field),
        DumpWord::GridRemap(grid) => grid_remap(grid, boundary),
        DumpWord::GridNextReference(grid) => boundary.workspace_field(grid.workspace(), capture_stock::GRID_REFERENCE).wrapping_add(1),
    }
}

fn subject_flags(record: RecordHandle, boundary: &mut impl DumpBoundary) -> u32 {
    let flags = boundary.field(record, RecordField::At002e);
    if boundary.field(record, RecordField::SubjectArea) == 0 { return flags; }
    let reference = boundary.area_reference(record);
    (flags & 0xfffe) | u32::from(reference.0[4] == 1)
}

fn layout(record: RecordHandle, records: &mut impl CaptureRecords) -> [u16; 2] {
    let layout = records.field(record, RecordField::Layout);
    [layout as u16, (layout >> 16) as u16]
}

fn capture_window(record: RecordHandle, boundary: &mut impl DumpBoundary) -> Option<[u32; 2]> {
    let locked = boundary.field(record, RecordField::SubjectsLocked);
    let layout = layout(record, boundary);
    let mode = if locked == 0 { boundary.slot_entry(record, 0).subject_mode() } else { locked };
    boundary.capture_window(mode, layout)
}

fn packed(key: AdjustmentKey, packing: Packing, settings: &mut impl DumpSettings) -> u32 {
    let mut byte = |index: u32| u32::from(settings.adjustment_byte(AdjustmentKey(key.0 + index)));
    match packing {
        Packing::Byte => byte(0),
        Packing::Pair => (byte(0) << 8) | byte(1),
        Packing::Spread => (byte(0) << 16) | byte(1),
        Packing::Quad => (byte(0) << 24) | (byte(1) << 16) | (byte(2) << 8) | byte(3),
        Packing::Half => byte(0) | (byte(1) << 8),
        Packing::Tenfold => byte(0) * 10,
    }
}

fn grid_remap(grid: RateGrid, boundary: &mut impl DumpBoundary) -> u32 {
    let workspace = grid.workspace();
    let position = boundary.workspace_field(workspace, capture_stock::GRID_POSITION);
    let request = RemapRequest {
        position,
        reference: boundary.workspace_field(workspace, capture_stock::GRID_REFERENCE),
        state: boundary.workspace_field(workspace, capture_stock::GRID_STATE),
        target: 0,
        remap: false,
        kind: REMAP_KIND,
    };
    (boundary.remap(request) as u32).wrapping_sub(position)
}
