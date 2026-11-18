#![forbid(unsafe_code)]

//! What the primary job's leaf work calls outside the capture steps: the capture command's
//! client memory, the image-4 record layer, the capture pipeline's imgdc requests, the profile
//! calibration, the calc library and the profile geometry. Byte layouts and stock entries are in
//! `capture_stock`.

use crate::boundary::ClientRef;
use crate::calculations_work::Processing;
use crate::capture_boundary::{SettingResult, SlotEntry, SlotHandle};
use crate::secondary_boundary::CropGeometry;
use xm1_saclac_interface::area_measurement::AreaEntry;
use xm1_saclac_interface::capture_job::{CaptureEnd, CaptureRequest, DisplayRequest, OutputSelection};
use xm1_saclac_interface::RecordHandle;

/// The three area lists imgdc delivers into the client's buffer and the record keeps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeliveredList { First, Second, Third }

impl DeliveredList {
    /// The record holds this many areas per delivered list.
    pub const CAPACITY: usize = 10;
}

/// The words of the command's source descriptor the capture request carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceWords {
    pub first: u32,
    pub words: [u32; 3],
    pub byte: u8,
    pub option: u32,
}

/// The client block the command names for the capture's calibration when it does not ask for
/// the default.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClientCalibration {
    pub word: u32,
    pub calibration: Calibration,
    pub rate: u8,
}

/// Client memory the command-7 parameters name, beyond `CaptureClient`.
pub trait CaptureJobClient {
    fn source_words(&mut self, sources: ClientRef) -> SourceWords;
    fn store_source_output(&mut self, sources: ClientRef, marker: u16, value: u32);
    /// The selection's byte at +4.
    fn selection_option(&mut self, selection: ClientRef) -> u8;
    /// The readout mode the selection keeps.
    fn readout_mode(&mut self, selection: ClientRef) -> u32;
    fn store_output(&mut self, selection: ClientRef, output: &OutputSelection, offset: u32, rate: u32);
    fn store_capture_end(&mut self, selection: ClientRef, end: &CaptureEnd);
    fn calibration_block(&mut self, block: ClientRef) -> ClientCalibration;
    /// Left, top, height and width.
    fn statistics_window(&mut self, block: ClientRef) -> [u32; 4];
    fn statistics_selector(&mut self, block: ClientRef) -> u32;
    fn clear_area_list(&mut self, buffer: ClientRef, list: DeliveredList);
    fn area_count(&mut self, buffer: ClientRef, list: DeliveredList) -> u32;
    fn area(&mut self, buffer: ClientRef, list: DeliveredList, index: u8) -> AreaEntry;
    fn set_area(&mut self, buffer: ClientRef, list: DeliveredList, index: u8, area: &AreaEntry);
}

/// A lens position of the record's slot table and the speed of a move to it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlotPosition {
    pub position: u16,
    pub speed: u32,
}

/// The record's slot positions, in slot order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlotPositions {
    pub count: u8,
    pub positions: [SlotPosition; 4],
}

/// The image-4 record layer beyond `CaptureRecords`.
pub trait CaptureJobRecords {
    fn record_calibration(&mut self, record: RecordHandle) -> Calibration;
    /// A word and a byte the statistics record carries.
    fn statistics_option(&mut self, record: RecordHandle) -> (u32, u8);
    fn slot_positions(&mut self, record: RecordHandle) -> SlotPositions;
    fn setting_result(&mut self, record: RecordHandle) -> SettingResult;
    fn slot_record(&mut self, slot: SlotHandle) -> SlotEntry;
    fn delivered_count(&mut self, record: RecordHandle, list: DeliveredList) -> u32;
    fn delivered_area(&mut self, record: RecordHandle, list: DeliveredList, index: u8) -> AreaEntry;
    fn store_delivered_count(&mut self, record: RecordHandle, list: DeliveredList, count: u32);
    fn store_delivered_area(&mut self, record: RecordHandle, list: DeliveredList, index: u8, area: &AreaEntry);
}

/// The optional completions a capture request asks imgdc for; 0x5e and 0x62 come always.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureCompletions {
    /// 0x5f and 0x60.
    pub outputs: bool,
    /// 0x61.
    pub readout: bool,
    /// 0x64.
    pub delivery: bool,
}

/// The capture pipeline's side of the primary job.
pub trait CapturePipeline {
    /// Imgdc 0x3f49; its last callback replies status 8 to the client when imgdc reports 0.
    fn capture(&mut self, request: &CaptureRequest, completions: CaptureCompletions);
    /// Imgdc 0x3f4a with the command-0xc parameters, completed through that command's callback.
    fn request_command_c(&mut self);
    /// Imgdc 0x3f4f, completed through the command-0x28 callback.
    fn request_command_28(&mut self);
    /// Imgdc 0x3f4e.
    fn display(&mut self, request: &DisplayRequest);
    /// The processing the readout's source has.
    fn readout_processing(&mut self, source: u8) -> Option<Processing>;
    /// Whether this capture's statistics record becomes the template of capture settings.
    fn keeps_template(&mut self, subject_source: u32, options: u32, mode: u8, profile_class: u32) -> bool;
    /// Whether the statistics the client waits for are ready.
    fn statistics_ready(&mut self) -> bool;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Calibration(pub [u16; 3]);

/// What the profile calibration builds its factors from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FactorRequest {
    pub record_words: [u32; 2],
    pub slot_reference: u32,
    pub kind: u32,
    pub calibration: Calibration,
    pub mode: u32,
}

/// The profile calibration of the image pipeline.
pub trait ProfileCalibration {
    fn calibrate(&mut self, request: &FactorRequest) -> Calibration;
    fn clamp_geometry(&mut self, word: u32, calibration: &Calibration, rate: u8, exposure: i16) -> Calibration;
    fn apply_positive(&mut self, exposure: i32, calibration: &mut Calibration);
    fn clamp_output(&mut self, calibration: &Calibration) -> Calibration;
}

/// What the default calibration takes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CalibrationSeed {
    pub record: Option<RecordHandle>,
    pub base: Calibration,
    pub pair: [u16; 2],
    pub words: [u32; 2],
}

/// Calc-library and lens-profile operations of the primary job.
pub trait CaptureJobCalc {
    /// The calibration and its word when the command asks for the default.
    fn default_calibration(&mut self, seed: &CalibrationSeed) -> (Calibration, u32);
    fn output_scale(&mut self, index: i16, distance: u32, reference: u32, words: [u32; 2]) -> i32;
}

/// One displayed window of the profile geometry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayArea {
    pub origin: u16,
    pub window: [u32; 4],
    pub extent: [u32; 4],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayLayout {
    pub areas: [DisplayArea; 2],
}

/// The frame of the delivered areas: the crop's frame, the window's corner within it and its
/// size, then the sensor window's corner within the second crop and its size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaFrame {
    pub crop: [u32; 6],
    pub sensor: [u32; 4],
}

/// How the profile geometry generates a delivered list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AreaMode { Second, Third }

/// The record words every profile-geometry query of the job carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryWords {
    pub offset: u32,
    pub frame_words: [u32; 2],
    pub layout: [u16; 2],
}

/// Profile-geometry operations of the primary job, beyond `MeasurementProfiles`.
pub trait CaptureProfiles {
    fn area_selector(&mut self, profile: u32, index: u8) -> u32;
    fn display_layout(&mut self, selector: u32, kind: u8) -> DisplayLayout;
    fn paired(&mut self, selector: u32) -> bool;
    fn sensor_window(&mut self, crop: &CropGeometry) -> [u32; 4];
    fn generate_areas(&mut self, mode: AreaMode, selector: u32, frame: &AreaFrame, buffer: ClientRef);
    /// Lays the first delivered list out for a window of `size` (vertical, horizontal).
    fn frame_areas(&mut self, words: &GeometryWords, size: [u32; 2], buffer: ClientRef);
    fn display_extent(&mut self, selector: u32, pair: [u32; 2], words: &GeometryWords) -> [u32; 2];
    /// The 72-byte measurement table the frame's dump carries.
    fn measurement_table(&mut self) -> [u32; 18];
}

/// Statuses the primary job replies to the capture command's client with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaptureStatus { Progress, Finished, Accounted, Displayed }

pub trait CaptureReplies {
    fn reply(&mut self, record: RecordHandle, status: CaptureStatus);
}
