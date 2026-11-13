//! The capture's calc requests 0x3c0d, 0x3c0e, 0x3c0f, 0x3c10 and 0x3c11: what the profile record,
//! the lens and the capture pipeline's processing contribute to each.

use xm1_saclac_interface::calc::{
    AreaEvaluationRequest, AreaGeometry, CalcPayload, CaptureFirstRequest, CaptureMeasurement, CaptureProcessing, CaptureSecondRequest,
    CaptureThirdRequest, SourceRequest, SourceStatistics,
};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_04, group_05, group_07, group_08};
use xm1_saclac_profile_records::stock::shared::group_27;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::focus_profile::LensResources;
use crate::exposure::ports::{CaptureProfiles, Lens, ProfileAccess, Properties};
use crate::requests::Peers;
use crate::stock::capture::{self, RefinedScene, FIRST_ELEMENT};

/// The lens calibration service of image 4.
pub trait LensCalibration {
    /// Thousandths of log2 of `lens`'s calibrated value at `index` over 100 (0x7468b8).
    fn calibration(&mut self, lens: u32, index: u32) -> i16;
}

/// Everything the capture's requests read besides their arguments.
pub trait CapturePorts: ProfileAccess + Properties + Lens + CaptureProfiles + LensResources + LensCalibration {}

impl<T: ProfileAccess + Properties + Lens + CaptureProfiles + LensResources + LensCalibration> CapturePorts for T {}

/// Scene flags under which the capture offset takes the configured adjustment (0xf51bb0).
const ADJUSTED_SCENE_FLAGS: u16 = 2;
/// Window calculation statuses that count as measured (0xf51f80..0xf51f8c).
const MEASURED_WINDOW_STATUSES: [u32; 2] = [3, 4];
/// The lens temperature while the lens word reports none, and the range a reported one is
/// clamped to (0x7482d0, 0x7482f0, 0xf4cdd0..0xf4cde8).
const DEFAULT_LENS_TEMPERATURE: i8 = 25;
const LENS_TEMPERATURES: core::ops::RangeInclusive<i8> = -5..=55;
const LENS_TEMPERATURE_INVALID: u32 = 0xc000_0000;

/// The scene and scene class a record carries to the second calculation (0xfde08c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RefinedSceneClass {
    pub scene: u8,
    pub class: u8,
}

/// With header byte 0x63 of 1, a refinement record measures portrait variants as portrait, a
/// landscape or night scene with a person as portrait or, where configured, as the scene without
/// the person, and close-up as auto; the landscape families name their class.
pub fn refined_scene(properties: &mut impl Properties, scene: u8, header_63: u8) -> RefinedSceneClass {
    if header_63 != 1 {
        return RefinedSceneClass { scene, class: 0 };
    }
    let mut configured = |which: RefinedScene, kept: u8| if properties.property(capture::refined_scene(which)) != 0 { kept } else { PORTRAIT };
    let (scene, class) = match scene {
        1 | 4 => (PORTRAIT, 0),
        8 | 9 => (configured(RefinedScene::Landscape, 5), 0),
        14 => (configured(RefinedScene::NightPortrait, 11), 0),
        15 => (configured(RefinedScene::NightPortrait, 12), 0),
        18 => (18, 18),
        19 | 20 => (configured(RefinedScene::Beach, 18), 18),
        21 => (21, 21),
        22 | 23 => (configured(RefinedScene::Snow, 21), 21),
        25 => (25, 25),
        26 | 27 => (configured(RefinedScene::Sunset, 25), 25),
        34 | 37 | 40 => (scene, scene),
        35 | 36 => (PORTRAIT, 34),
        38 | 39 => (PORTRAIT, 37),
        41 | 42 => (PORTRAIT, 40),
        48 => (0, 0),
        other => (other, 0),
    };
    RefinedSceneClass { scene, class }
}

const PORTRAIT: u8 = 1;

/// The record's capture offset: its chosen word, plus the configured adjustment while the scene
/// flags are 2 (0xf51b94).
pub fn capture_offset(ports: &mut (impl ProfileAccess + Properties), record: RecordHandle) -> u32 {
    let (chosen, flags) = {
        let profile = ports.profile();
        (profile.chosen_word(record), profile.read_shared(record, &group_27::AT_0740))
    };
    if flags == ADJUSTED_SCENE_FLAGS { chosen.wrapping_add(ports.property(capture::CAPTURE_OFFSET_ADJUSTMENT)) } else { chosen }
}

fn selection_byte(selection: MeasurementSelection) -> u8 {
    match selection {
        MeasurementSelection::Unselected => 0,
        MeasurementSelection::Record => 1,
        MeasurementSelection::Other(byte) => byte,
    }
}

/// Calc 0x3c0f (0xf4d448).
pub fn request_first<Q: Peers>(ports: &mut impl CapturePorts, peers: &mut Q, record: RecordHandle, processing: &CaptureProcessing, callback: StockCallbackWord) -> Result<(), Q::Error> {
    let at_0022 = ports.profile().read(record, &group_00::AT_0022);
    peers.calc(&CalcPayload::CaptureFirst { request: CaptureFirstRequest { at_0022, processing: *processing }, callback })
}

/// Calc 0x3c0e (0xf4d350).
pub fn request_third<Q: Peers>(
    ports: &mut impl CapturePorts,
    peers: &mut Q,
    record: RecordHandle,
    processing: &CaptureProcessing,
    detail: u32,
    callback: StockCallbackWord,
) -> Result<(), Q::Error> {
    let (scene, at_0022, scene_flags, lens) = {
        let mut profile = ports.profile();
        let scene = profile.header(record, SharedHeaderByte::At31);
        (scene, profile.read(record, &group_00::AT_0022), profile.read_shared(record, &group_27::AT_0740), profile.read(record, &group_05::AT_0170.at(FIRST_ELEMENT)))
    };
    let calibration = [ports.calibration(lens, 0), ports.calibration(0, 0)];
    let request = CaptureThirdRequest { scene, scene_flags, at_0022, processing: *processing, calibration, detail };
    peers.calc(&CalcPayload::CaptureThird { request, callback })
}

/// Calc 0x3c11 from the capture (0xf52120).
pub fn request_fourth<Q: Peers>(
    ports: &mut impl CapturePorts,
    peers: &mut Q,
    record: RecordHandle,
    processing: &CaptureProcessing,
    detail: u32,
    callback: StockCallbackWord,
) -> Result<(), Q::Error> {
    let offset = capture_offset(ports, record);
    let (profile_halfword, level, selection) = {
        let mut profile = ports.profile();
        let profile_halfword = profile.read(record, &group_00::AT_001C);
        let level = profile.read(record, &group_01::AT_00A0);
        (profile_halfword, level, profile.measurement_selection(record))
    };
    let request = SourceRequest {
        statistics: SourceStatistics::Capture,
        profile: [offset, processing.setting],
        decade: processing.decade,
        profile_halfword: profile_halfword as u16,
        level: level as u16,
        reference: processing.reference,
        size: processing.extent,
        exposure: processing.lens_reference,
        offset_mode: processing.offset_mode,
        mode: processing.profile,
        selection: selection_byte(selection),
        extension: detail,
    };
    peers.calc(&CalcPayload::Source { request, callback })
}

/// The sum of the record's level fields 0x178, 0x17a, 0x17c and 0x186 (0xf4cb58..0xf4cb9c,
/// 0xf51ccc..0xf51d08).
fn level_fields_total(ports: &mut impl ProfileAccess, record: RecordHandle) -> u16 {
    let profile = ports.profile();
    [&group_05::AT_0178, &group_05::AT_017A, &group_05::AT_017C, &group_05::AT_0186]
        .into_iter()
        .map(|array| profile.read(record, &array.at(FIRST_ELEMENT)) as u16)
        .fold(0, u16::wrapping_add)
}

/// Calc 0x3c10 (0xf51bcc).
pub fn request_second<Q: Peers>(
    ports: &mut impl CapturePorts,
    peers: &mut Q,
    record: RecordHandle,
    processing: &CaptureProcessing,
    measurement: &CaptureMeasurement,
    detail: u32,
    callback: StockCallbackWord,
) -> Result<(), Q::Error> {
    let level_total = level_fields_total(ports, record);
    let capture_offset = capture_offset(ports, record);
    let mut profile = ports.profile();
    let header_63 = profile.header(record, SharedHeaderByte::At63);
    let raw_scene = profile.header(record, SharedHeaderByte::At31);
    let auxiliary = profile.header(record, SharedHeaderByte::At46);
    let reference = profile.read(record, &group_01::AT_00BC);
    let lens_field = profile.read(record, &group_08::AT_01F2.at(FIRST_ELEMENT));
    let level_total = level_total.wrapping_add(profile.read(record, &group_01::AT_00A4) as u16);
    let offsets = if processing.offset_mode != 0 {
        [profile.read(record, &group_07::AT_01B0.at(FIRST_ELEMENT)), profile.read(record, &group_07::AT_01B4.at(FIRST_ELEMENT))]
    } else {
        [0, 0]
    };
    let at_01ec = profile.read(record, &group_07::AT_01EC.at(FIRST_ELEMENT));
    let window = profile.read_shared(record, &group_27::WINDOW_STATUS);
    let mut request = CaptureSecondRequest {
        scene: raw_scene,
        scene_class: 0,
        header_63,
        scene_flags: profile.read_shared(record, &group_27::AT_0740),
        at_003e: profile.read(record, &group_00::AT_003E),
        at_003f: profile.read(record, &group_00::AT_003F),
        auxiliary,
        at_0048: profile.read(record, &group_00::AT_0048),
        at_0045: profile.read(record, &group_00::AT_0045),
        profile_kind: profile.read(record, &group_00::AT_0043),
        at_0022: profile.read(record, &group_00::AT_0022),
        at_000e: profile.read(record, &group_00::AT_000E),
        at_002a: profile.read(record, &group_00::AT_002A),
        at_00a0: profile.read(record, &group_01::AT_00A0),
        at_0092: profile.read(record, &group_01::AT_0092),
        at_001a: profile.read(record, &group_00::AT_001A),
        level_total,
        lens_level: 0,
        offsets,
        at_01ec,
        at_017a: profile.read(record, &group_05::AT_017A.at(FIRST_ELEMENT)),
        at_017c: profile.read(record, &group_05::AT_017C.at(FIRST_ELEMENT)),
        at_017e: profile.read(record, &group_05::AT_017E.at(FIRST_ELEMENT)),
        at_0180: profile.read(record, &group_05::AT_0180.at(FIRST_ELEMENT)),
        at_0186: profile.read(record, &group_05::AT_0186.at(FIRST_ELEMENT)),
        at_0188: profile.read(record, &group_05::AT_0188.at(FIRST_ELEMENT)),
        capture_offset,
        at_0164: profile.read(record, &group_04::AT_0164),
        at_0168: profile.read(record, &group_04::AT_0168),
        at_01f4: profile.read(record, &group_08::AT_01F4.at(FIRST_ELEMENT)),
        processing: *processing,
        window_measured: MEASURED_WINDOW_STATUSES.contains(&window.0),
        measurement: *measurement,
        detail,
    };
    let refined = refined_scene(ports, raw_scene, header_63);
    request.scene = refined.scene;
    request.scene_class = refined.class;
    request.lens_level = lens_field.wrapping_add(ports.reference_offset(reference));
    if !(processing.offset_mode == 2 && ports.property(capture::KEEPS_OFFSET_FIELD) != 0) {
        request.at_01ec = 0;
    }
    peers.calc(&CalcPayload::CaptureSecond { request, callback })
}

/// The lens temperature the lens state word reports: its low byte in degrees, clamped, unless
/// either top bit marks it invalid.
fn lens_temperature(word: u32) -> i8 {
    if word & LENS_TEMPERATURE_INVALID != 0 {
        DEFAULT_LENS_TEMPERATURE
    } else {
        (word as u8 as i8).clamp(*LENS_TEMPERATURES.start(), *LENS_TEMPERATURES.end())
    }
}

/// Calc 0x3c0d (0xf4ca60), over the secondary job's geometry and its capture frame and detail.
pub fn request_area_evaluation<Q: Peers>(
    ports: &mut impl CapturePorts,
    peers: &mut Q,
    record: RecordHandle,
    processing: &CaptureProcessing,
    frames: [u32; 2],
    geometry: &AreaGeometry,
    callbacks: [StockCallbackWord; 2],
) -> Result<(), Q::Error> {
    let lens_temperature = lens_temperature(ports.flags());
    let level_total = level_fields_total(ports, record);
    let mut profile = ports.profile();
    let scene = profile.header(record, SharedHeaderByte::At31);
    let header_47 = profile.header(record, SharedHeaderByte::At47);
    let at_0174 = profile.read(record, &group_05::AT_0174.at(FIRST_ELEMENT));
    let mut request = AreaEvaluationRequest {
        scene,
        profile_kind: profile.read(record, &group_00::AT_0043),
        header_47,
        at_001a: profile.read(record, &group_00::AT_001A),
        at_0022: profile.read(record, &group_00::AT_0022),
        at_0045: profile.read(record, &group_00::AT_0045),
        at_004b: profile.read(record, &group_00::AT_004B),
        at_0048: profile.read(record, &group_00::AT_0048),
        at_0041: profile.read(record, &group_00::AT_0041),
        at_01b4: profile.read(record, &group_07::AT_01B4.at(FIRST_ELEMENT)),
        at_01f2: profile.read(record, &group_08::AT_01F2.at(FIRST_ELEMENT)),
        level_total,
        at_017c: profile.read(record, &group_05::AT_017C.at(FIRST_ELEMENT)),
        at_0186: profile.read(record, &group_05::AT_0186.at(FIRST_ELEMENT)),
        at_0170: profile.read(record, &group_05::AT_0170.at(FIRST_ELEMENT)),
        at_0174,
        at_0176: profile.read(record, &group_05::AT_0176.at(FIRST_ELEMENT)),
        at_0188: profile.read(record, &group_05::AT_0188.at(FIRST_ELEMENT)),
        at_01f0: profile.read(record, &group_08::AT_01F0.at(FIRST_ELEMENT)),
        frames,
        lens_level: 0,
        level_offset: 0,
        processing: *processing,
        geometry: *geometry,
        scene_flags: profile.read_shared(record, &group_27::AT_0740),
        lens_temperature,
    };
    let lens_level = ports.level(processing.lens, processing.lens_reference);
    request.lens_level = lens_level.wrapping_add(processing.level as i16).wrapping_sub(at_0174);
    request.level_offset = ports.level_offset(processing.profile);
    peers.calc(&CalcPayload::AreaEvaluation { request, callbacks })
}
