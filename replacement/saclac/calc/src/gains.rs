//! What the library asks the capture pipeline's gain selection for.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_07};
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::capture_requests::refined_scene;
use crate::exposure::ports::{ProfileAccess, Properties};
use crate::stock::capture::FIRST_ELEMENT;

/// One gain per channel.
pub type Gains = [u16; 3];

/// A gain class of the capture pipeline's gain selection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GainClass(pub u32);

/// A request to the capture pipeline's gain selection; the fields are the ones it names.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GainRequest {
    pub mode: u32,
    pub variant: u32,
    pub custom_source: u32,
    pub bank: u32,
    pub fixed: Gains,
    pub preset: u32,
    pub temperature: u16,
    pub column: i16,
    pub level: i16,
    pub custom: Gains,
    pub variant_set: u32,
    pub forced: bool,
    pub variant_subset: u32,
}

/// The gains a request selects and the class they come from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectedGains {
    pub gains: Gains,
    pub class: GainClass,
}

/// What the gain selection reads a blend weight for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlendSelector {
    pub mode: u32,
    pub variant: u32,
    /// 0..=2.
    pub column: u32,
    pub sensitivity: u32,
    pub preset: u32,
}

/// The capture pipeline's gain selection.
pub trait CaptureGains {
    fn gains(&mut self, request: &GainRequest) -> SelectedGains;
    /// The weight `selector` names, 10 where its class or weight is out of range.
    fn blend_weight(&mut self, selector: &BlendSelector) -> u8;
}

/// What a capture job's default calibration is selected from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationSeed {
    pub record: RecordHandle,
    /// The record's own calibration.
    pub custom: Gains,
    pub column: i16,
    pub level: i16,
    /// The record's preset counts only while this is nonzero.
    pub variant_set: u32,
}

/// The record's fixed gains.
pub(crate) fn fixed_gains(ports: &mut impl ProfileAccess, record: RecordHandle) -> Gains {
    let bytes: [u8; 6] = ports.profile().read(record, &group_00::AT_000E);
    core::array::from_fn(|channel| u16::from_le_bytes([bytes[2 * channel], bytes[2 * channel + 1]]))
}

/// The gains the record's settings select for a capture job that asks for the library's default
/// calibration (0xf4d4cc).
pub fn default_calibration(ports: &mut (impl ProfileAccess + CaptureGains), seed: &CalibrationSeed) -> SelectedGains {
    let record = seed.record;
    let fixed = fixed_gains(ports, record);
    let request = {
        let mut profile = ports.profile();
        GainRequest {
            mode: u32::from(profile.header(record, SharedHeaderByte::At31)),
            variant: u32::from(profile.read(record, &group_00::AT_0048)),
            custom_source: u32::from(profile.read(record, &group_00::AT_002A)),
            bank: u32::from(profile.header(record, SharedHeaderByte::At46)),
            fixed,
            preset: if seed.variant_set == 0 { 0 } else { profile.read(record, &group_07::AT_01B0.at(FIRST_ELEMENT)) },
            temperature: profile.read(record, &group_01::AT_0090_2) as u16,
            column: seed.column,
            level: seed.level,
            custom: seed.custom,
            variant_set: seed.variant_set,
            forced: false,
            variant_subset: 0,
        }
    };
    ports.gains(&request)
}

const REFERENCE_COLUMN: i16 = 500;
const REFERENCE_LEVEL: i16 = 600;

/// The gains `request` selects at column 500 and level 600 (0xf4d5bc).
pub fn reference_gains(gains: &mut impl CaptureGains, request: &GainRequest) -> SelectedGains {
    gains.gains(&GainRequest { column: REFERENCE_COLUMN, level: REFERENCE_LEVEL, ..*request })
}

/// What the focus grid reads a blend weight for.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlendQuery {
    /// The record's selection kind.
    pub scene: u32,
    /// The result pass, 0..=2.
    pub column: u32,
    /// Record byte +0x45.
    pub sensitivity: u32,
    /// Shared header byte 0x63.
    pub header_63: u32,
    /// Record byte +0x3e.
    pub variant: u32,
}

/// The blend weight of the query's scene, refined as a refinement record measures it, with header
/// byte 0x63 as the preset (0xf4d700).
pub fn focus_blend(ports: &mut (impl Properties + CaptureGains), query: &BlendQuery) -> u8 {
    let mode = match u8::try_from(query.scene) {
        Ok(scene) if query.header_63 == 1 => u32::from(refined_scene(ports, scene, 1).scene),
        _ => query.scene,
    };
    let selector = BlendSelector { mode, variant: query.variant, column: query.column, sensitivity: query.sensitivity, preset: query.header_63 };
    ports.blend_weight(&selector)
}
