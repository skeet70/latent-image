#![forbid(unsafe_code)]

//! What the secondary job's leaf work calls outside the capture steps: the record's area lists,
//! calc-library and profile-geometry operations not owned yet, the lens layer, the capture
//! pipeline's statistics and its area measurement, and configuration. Byte layouts and stock
//! entries are in `capture_stock`.

use crate::calculations_work::Processing;
use crate::finish_work::FinishLensValue;
use xm1_saclac_interface::area_measurement::{AreaEntry, CaptureSelection, MeasuredAreas, MeasurementRequest, TransferRequest};
use xm1_saclac_interface::calc::AreaGeometry;
use xm1_saclac_interface::{Completion, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AreaList { First, Second }

/// The bound record's two area lists.
pub trait AreaLists {
    fn area_count(&mut self, record: RecordHandle, list: AreaList) -> u32;
    fn area(&mut self, record: RecordHandle, list: AreaList, index: u8) -> AreaEntry;
    fn list_detail(&mut self, record: RecordHandle, list: AreaList) -> u32;
    fn prefers_second_list(&mut self, record: RecordHandle) -> bool;
    /// The first slot's 16-byte profile extent.
    fn store_slot_extent(&mut self, record: RecordHandle, extent: [u32; 4]);
    /// The first slot's geometry extent and cell sizes.
    fn store_slot_cells(&mut self, record: RecordHandle, cells: [u32; 4]);
}

/// Calc-library operations of the secondary job, owned by the calc library.
pub trait MeasurementCalc {
    fn capture_selection(&mut self, record: RecordHandle) -> CaptureSelection;
    /// The key pair of the first-pass statistics.
    fn statistics_key(&mut self, processing: &Processing) -> [i16; 2];
    /// The `index`th of eight scale steps.
    fn scale_step(&mut self, index: u16) -> i16;
    fn capture_offset(&mut self, record: RecordHandle, distance: u32) -> u32;
    fn curve_within(&mut self, distance: u32, selection: u32, drive: u32, limit: i32) -> bool;
    /// Calc 0x3c0d, completing with `completions` in that order.
    fn request_evaluation(&mut self, record: RecordHandle, processing: &Processing, frames: [u32; 2], geometry: &AreaGeometry, completions: [Completion; 2]);
}

/// What a profile-geometry query takes from the record and the lens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryRequest {
    pub selector: u32,
    pub limits: [i16; 2],
    pub reference: u32,
    pub distance: u32,
    pub offset: u32,
    pub frame_words: [u32; 2],
    pub layout: [u16; 2],
}

/// A frame laid out as a grid of cells with gaps; each pair is (vertical, horizontal) except
/// `gap`, which is (horizontal, vertical).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileGeometry {
    pub frame: [u32; 2],
    pub cell: [u32; 2],
    pub extent: [u32; 2],
    pub gap: [u16; 2],
    pub factors: [u8; 2],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CropRequest {
    pub selector: u32,
    pub offset: u32,
    pub frame_words: [u32; 2],
    pub layout: [u16; 2],
    pub full: bool,
}

/// The cropped frame (vertical, horizontal) and the window inside it: left and top offsets, then
/// its vertical and horizontal size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CropGeometry {
    pub frame: [u32; 2],
    pub window: [u32; 4],
}

/// Profile-geometry operations, owned by the profile geometry.
pub trait MeasurementProfiles {
    fn profile_geometry(&mut self, request: &GeometryRequest) -> ProfileGeometry;
    fn crop_geometry(&mut self, request: &CropRequest) -> CropGeometry;
    fn flagged_scale_pair(&mut self, selector: u32) -> [u32; 2];
    fn profile_extent(&mut self, selector: u32, inset: bool) -> [u32; 4];
}

/// The lens layer (image 4).
pub trait MeasurementLens {
    fn lens(&mut self, value: FinishLensValue) -> u32;
    fn clamp_step(&mut self, position: i16, kind: u32, direction: u32) -> i16;
    /// The lens reference table's value at `index`, 0xffff outside the table.
    fn reference_value(&mut self, index: i32) -> u32;
}

/// Where the capture pipeline keeps a measurement's statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsStore { First, Later }

/// Words the capture pipeline keeps about the latest area measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementWord { FirstList, SecondList, DumpFirst, DumpSecond, DumpThird }

/// The capture pipeline's side of the area measurement.
pub trait MeasurementPipeline {
    /// Fills `processing` and the store from the source's statistics; false when the source has
    /// none.
    fn prepare_statistics(&mut self, source: u8, store: StatisticsStore, processing: &mut Processing) -> bool;
    fn measurement_word(&mut self, word: MeasurementWord) -> u32;
    fn release_measurement(&mut self);
    /// Imgdc 0x3f46; imgdc reads `areas` after this returns.
    fn measure_areas(&mut self, request: &MeasurementRequest, areas: &MeasuredAreas, completions: [Completion; 2]);
    /// Imgdc 0x3f47.
    fn transfer(&mut self, request: &TransferRequest, completion: Completion);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MeasurementSetting {
    /// 1 lets a chosen list be measured.
    Measure,
    /// 1 lets the second list be chosen when the first is empty.
    SecondListAlone,
    /// Nonzero searches every area of the chosen list for the band, else only the first.
    SearchAll,
    CarriedFirst,
    CarriedSecond,
    /// The most rows the band moves towards an area outside it.
    ShiftLimit,
    /// Nonzero drops the measurement when no area is within `ShiftLimit`.
    RequiresArea,
    /// Evaluation defaults of an alternative pass, which takes ten times each.
    MeasuredDefault,
    TransferDefault,
    PrimaryLimitDefault,
}

pub trait MeasurementSettings {
    fn measurement_setting(&mut self, setting: MeasurementSetting) -> u32;
}
