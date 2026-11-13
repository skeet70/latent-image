#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to calc (queue 44) and calc1 (queue 45).
//!
//! A `Record` is bytes a requester built and the peer reads whole; its fields are the
//! requester's to type.

use crate::exposure::EvaluationRequest;
use crate::stock::StockCallbackWord;

/// An address in saclac memory that the peer reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SharedAddress(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record<const BYTES: usize>(pub [u8; BYTES]);

/// The shared record fields the window calculation reports on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRecord {
    pub first: u32,
    pub second: u32,
    pub status: u32,
    pub halfword: u16,
}

/// The primary statistics snapshot values the window calculation reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowSample {
    pub mode: u32,
    pub flags: u8,
    pub exposure: u32,
    pub level: u16,
    pub adjustment: u32,
}

/// Calc 0x3c0c: one window calculation over the primary statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRequest {
    /// The profile's selection byte.
    pub selection: u8,
    pub mode: u8,
    /// The profile selects by record, or has a measurement mode.
    pub selected: bool,
    pub sample: WindowSample,
    /// The sample's level, offset by the reference and the exposure value.
    pub level: u16,
    /// The sample's second level less the gain offset.
    pub reference: u16,
    pub window: WindowRecord,
    pub refresh: bool,
}

/// The statistics buffer a source reading is taken over.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceStatistics {
    /// The source lane's primary statistics.
    #[default]
    Primary,
    /// The capture pipeline's statistics, for the capture's fourth calculation.
    Capture,
}

/// Calc 0x3c11: one source reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRequest {
    pub statistics: SourceStatistics,
    /// Profile values calc reads unchanged.
    pub profile: [u32; 2],
    pub decade: u16,
    pub profile_halfword: u16,
    pub level: u16,
    /// The sample's second level.
    pub reference: u16,
    pub size: [u32; 2],
    pub exposure: u32,
    /// Zero from the source lane.
    pub offset_mode: u32,
    pub mode: u32,
    /// The profile's selection byte.
    pub selection: u8,
    pub extension: u32,
}

/// One request to calc, with the callback calc enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalcPayload {
    /// 0x3c00.
    Evaluation { request: EvaluationRequest, callback: StockCallbackWord },
    /// 0x3c02: calc reads `block` and `source` in place.
    Calculation { block: SharedAddress, source: SharedAddress, callback: StockCallbackWord },
    /// 0x3c03.
    PipelineFirst { first: Record<60>, second: Record<24>, callback: StockCallbackWord },
    /// 0x3c04.
    PipelineSecond { block: Record<64>, callback: StockCallbackWord },
    /// 0x3c05.
    PipelineThird { block: Record<116>, callback: StockCallbackWord },
    /// 0x3c09: completes with 0x9d for the S1 sender and 0x89 for the search.
    Search { record: Record<120>, callback: StockCallbackWord },
    /// 0x3c0a.
    Image { record: Record<20>, callback: StockCallbackWord },
    /// 0x3c0c.
    Window { request: WindowRequest, callback: StockCallbackWord },
    /// 0x3c0d: completes first with `callbacks[0]` (0x94), then with `callbacks[1]` (0x95).
    AreaEvaluation { request: AreaEvaluationRequest, callbacks: [StockCallbackWord; 2] },
    /// 0x3c0e.
    CaptureThird { request: CaptureThirdRequest, callback: StockCallbackWord },
    /// 0x3c0f.
    CaptureFirst { request: CaptureFirstRequest, callback: StockCallbackWord },
    /// 0x3c10.
    CaptureSecond { request: CaptureSecondRequest, callback: StockCallbackWord },
    /// 0x3c11.
    Source { request: SourceRequest, callback: StockCallbackWord },
    /// 0x3c12, with no payload and no callback.
    Release,
}

/// One request to calc1, with the callback calc1 enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Calc1Payload {
    /// 0x3d00. The grid is stored where calc1 reads it before the request is posted.
    Acquisition { record: Record<168>, callback: StockCallbackWord },
    /// 0x3d01.
    Selection { record: Record<228>, callback: StockCallbackWord },
    /// 0x3d02.
    Parameters { record: Record<124>, callback: StockCallbackWord },
}

/// The block calc1 reads with a 0x3d00 request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcquisitionGrid(pub [u8; 1236]);

/// The secondary job's measurement geometry, which the calc 0x3c0d request is built from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaGeometry {
    pub active: bool,
    pub list: Option<crate::area_measurement::EvaluatedList>,
    /// Configuration bytes the calc request carries.
    pub settings: [u8; 3],
    /// The chosen list's area count, up to three.
    pub count: u8,
    /// Each area's longer side as a percentage of the frame.
    pub shares: [u32; 3],
    pub scale: u16,
    pub list_value: u32,
    /// The areas that were measured, nonzero only while `active`.
    pub area_count: u32,
    pub areas: crate::area_measurement::MeasuredAreas,
    pub accepted: [bool; 3],
    /// Rows of the measurement band on each side of the frame's middle.
    pub span: u16,
    pub shift: i16,
    pub record_value: u32,
    pub within_limit: bool,
    pub slot_rate: u32,
}

/// The capture pipeline's processing values for one readout, which the capture's calc requests
/// carry. Calc reads them unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureProcessing {
    pub extent: [u32; 2],
    /// Columns and rows of the statistics grid.
    pub grid: [u32; 2],
    pub geometry: [u8; 24],
    pub level: u16,
    pub reference: u16,
    pub decade: u16,
    pub companion: u16,
    pub setting: u32,
    /// The capture profile.
    pub profile: u32,
    pub lens: u32,
    pub lens_reference: u32,
    /// 1 asks the second calculation to add the configured offset to an unrefined base.
    pub offset_mode: u32,
}

/// Calc 0x3c0f: the capture's first calculation. `at_*` fields are the profile record's fields of
/// that offset (element 0 of an array), which calc reads unchanged.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureFirstRequest {
    pub at_0022: i16,
    pub processing: CaptureProcessing,
}

/// Calc 0x3c0e: the capture's third calculation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureThirdRequest {
    pub scene: u8,
    pub scene_flags: u16,
    pub at_0022: i16,
    pub processing: CaptureProcessing,
    /// The lens calibration's value for the record's lens, then for lens 0.
    pub calibration: [i16; 2],
    pub detail: u32,
}

/// The secondary measurement the capture's second calculation carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureMeasurement {
    pub first: i16,
    pub second: i16,
    /// With the configured offset when the processing asks for it.
    pub base: i16,
    pub third: i16,
    pub fourth: i16,
    pub slot: [u8; 12],
    pub fifth: i16,
    pub sixth: i16,
    pub paired: bool,
    pub count: u8,
    pub block: [u8; 24],
    pub seventh: i16,
}

/// Calc 0x3c10: the capture's second calculation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CaptureSecondRequest {
    /// The record's scene, with person scenes removed while `header_63` is 1.
    pub scene: u8,
    /// While person scenes are removed: the beach, snow, sunset, sky, green or sky-and-green
    /// scene a scene of that family belongs to; otherwise 0.
    pub scene_class: u8,
    /// The record's shared header byte 0x63.
    pub header_63: u8,
    pub scene_flags: u16,
    pub at_003e: u8,
    pub at_003f: u8,
    /// The record's selection auxiliary byte.
    pub auxiliary: u8,
    pub at_0048: u8,
    pub at_0045: u8,
    pub profile_kind: u8,
    pub at_0022: i16,
    pub at_000e: [u8; 6],
    pub at_002a: u16,
    pub at_00a0: i16,
    pub at_0092: i16,
    pub at_001a: i16,
    /// The sum of the record's level fields 0x178, 0x17a, 0x17c, 0x186 and 0xa4.
    pub level_total: u16,
    /// Record field 0x1f2 plus the lens's level offset at the record's lens reference.
    pub lens_level: i16,
    /// Record fields 0x1b0 and 0x1b4 while the processing has an offset mode, else zero.
    pub offsets: [u32; 2],
    /// Record field 0x1ec, or zero unless the offset mode is 2 and the configuration keeps it.
    pub at_01ec: u32,
    pub at_017a: i16,
    pub at_017c: i16,
    pub at_017e: i16,
    pub at_0180: i16,
    pub at_0186: i16,
    pub at_0188: u32,
    pub capture_offset: u32,
    pub at_0164: u32,
    pub at_0168: u32,
    pub at_01f4: u32,
    pub processing: CaptureProcessing,
    /// The latest window calculation measured.
    pub window_measured: bool,
    pub measurement: CaptureMeasurement,
    pub detail: u32,
}

/// Calc 0x3c0d: the evaluation of the secondary area measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaEvaluationRequest {
    pub scene: u8,
    pub profile_kind: u8,
    /// The record's shared header byte 0x47.
    pub header_47: u8,
    pub at_001a: i16,
    pub at_0022: i16,
    pub at_0045: u8,
    pub at_004b: u8,
    pub at_0048: u8,
    pub at_0041: u8,
    pub at_01b4: u32,
    pub at_01f2: i16,
    /// The sum of the record's level fields 0x178, 0x17a, 0x17c and 0x186.
    pub level_total: u16,
    pub at_017c: i16,
    pub at_0186: i16,
    pub at_0170: u32,
    pub at_0174: i16,
    pub at_0176: i16,
    pub at_0188: u32,
    pub at_01f0: i16,
    /// The capture frame and its detail word.
    pub frames: [u32; 2],
    /// The lens's level at the processing's lens reference, plus the processing's level, less
    /// record field 0x174.
    pub lens_level: i16,
    /// The capture profile's level offset.
    pub level_offset: i16,
    pub processing: CaptureProcessing,
    pub geometry: AreaGeometry,
    pub scene_flags: u16,
    /// Degrees from the lens state, 25 while the lens reports none.
    pub lens_temperature: i8,
}
