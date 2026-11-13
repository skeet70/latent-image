#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to calc (queue 44) and calc1 (queue 45).
//!
//! A `Record` is bytes a requester built and the peer reads whole; its fields are the
//! requester's to type.

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
    /// The profile selects by record, has a measurement mode, or takes an adjustment word of 1.
    pub selected: bool,
    pub sample: WindowSample,
    /// The sample's level, offset by the reference and the exposure value.
    pub level: u16,
    /// The sample's second level less the gain offset.
    pub reference: u16,
    pub window: WindowRecord,
    pub refresh: bool,
    /// The adjustment state's words the request carries; the second is zero unless the profile takes it.
    pub adjustment: [u32; 2],
}

/// One request to calc, with the callback calc enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalcPayload {
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
    /// 0x3c11 from the source lane's 48-byte record.
    Source { record: Record<48>, callback: StockCallbackWord },
    /// 0x3c12, with no payload and no callback.
    Release,
}

/// The auxiliary statistics snapshot values a 0x3d02 request carries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxiliarySample {
    pub mode: u32,
    pub gain_step: u32,
    pub exposure: [u32; 2],
    pub level: u16,
    pub offset_level: u16,
    pub adjustment: u32,
    pub size: [u32; 2],
    pub origin: [u32; 2],
    pub extent: [u32; 2],
}

/// Calc1 0x3d02: one pass of the auxiliary measurement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxiliaryRequest {
    /// The sample is complete.
    pub complete: bool,
    /// 1 to 3.
    pub pass: u8,
    /// The profile's selection byte.
    pub selection: u8,
    /// Profile values calc1 reads unchanged.
    pub profile: [u32; 4],
    pub sample: AuxiliarySample,
    /// The sample's second level less the gain offset.
    pub reference: u16,
    /// The configured rate scaled by the sample's dimensions, times four.
    pub rate: u32,
    pub level_class: u32,
    pub window_status: u32,
    pub window_halfword: u16,
    pub resource_level: u16,
    pub word: u32,
    pub halfword: u16,
}

/// One request to calc1, with the callback calc1 enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Calc1Payload {
    /// 0x3d00. The grid is stored where calc1 reads it before the request is posted.
    Acquisition { record: Record<168>, callback: StockCallbackWord },
    /// 0x3d01.
    Selection { record: Record<228>, callback: StockCallbackWord },
    /// 0x3d02.
    Auxiliary { request: AuxiliaryRequest, callback: StockCallbackWord },
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
