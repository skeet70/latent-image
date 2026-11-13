//! The capture layer's stock side: image-5 control and profile tables, the capture pipeline's
//! entries and the stock services the layer calls, at their X-M1 1.50 addresses.

use core::ptr::read_volatile;

use super::read_u32;
use crate::capture::gains::{GainClass, GainSources, Gains};
use crate::capture::pipeline::{
    CapturePipeline, CaptureRecord, CaptureResults, CompletedStatistics, PipelineSnapshot, ProcessingWord, RecordBuffers,
    SampleMeasurements, SavedSnapshot, StockBlock, STATISTICS_BLOCK_BYTES, STATISTICS_PARAMETER_BYTES,
};
use crate::capture::ports::{
    Adjustments, ControlState, ControlTables, LoadTest, LoadTestSetting, Profiles, Reference, ReferenceIndex, Runtime,
};
use crate::capture::{AdjustmentKey, ProfileHandle, ProfileKind, Rectangle};

fn read_u16(address: u32) -> u16 { unsafe { read_volatile(address as usize as *const u16) } }

/// Readout pairs: the reference index word, then a flag word.
const READOUT_PAIRS: u32 = 0x101_e014;
/// Readout references, 0x1c bytes each.
const REFERENCES: u32 = 0x101_de8c;
const SHUTTER_RATES: u32 = 0x100_99c8;
const DECADES: u32 = 0x100_a60c;
const RATE_DIVISORS: u32 = 0x100_9988;
/// Two adjustment keys per entry, six entries per group.
const SCALE_KEYS: u32 = 0x102_2f54;
/// The control state's configured frame lines.
const CONFIGURED_LINES: u32 = 0x16f_8668 + 0x8be;

const fn profile_kind(byte: u8) -> Option<ProfileKind> {
    Some(match byte {
        0 => ProfileKind::Untimed,
        1 => ProfileKind::Tabled,
        2 => ProfileKind::Configured,
        3 => ProfileKind::ConfiguredAlternate,
        4 => ProfileKind::RateLimited,
        5 => ProfileKind::Exposed,
        _ => return None,
    })
}

/// The capture layer's ports, all bound to stock.
#[derive(Clone, Copy, Debug, Default)]
pub struct StockCapture;

impl Adjustments for StockCapture {
    fn byte(&self, key: AdjustmentKey) -> u8 { entry!(0x69_f8fc, fn(u32) -> u8)(key.0) }
}

impl Profiles for StockCapture {
    fn kind(&self, profile: ProfileHandle) -> Option<ProfileKind> { profile_kind(entry!(0xef_5448, fn(u32) -> u8)(profile.0)) }

    fn rate_threshold(&self, profile: ProfileHandle) -> u32 { entry!(0xef_5360, fn(u32) -> u32)(profile.0) }

    fn crop(&mut self, profile: ProfileHandle, preserve_axes: bool) -> Rectangle {
        let mut words = [0u32; 4];
        entry!(0xef_5470, fn(u32, *mut u32, u32))(profile.0, words.as_mut_ptr(), u32::from(preserve_axes));
        let [top, left, width, height] = words;
        Rectangle { top, left, width, height }
    }
}

impl ControlTables for StockCapture {
    fn reference_index(&self, profile: ProfileHandle) -> ReferenceIndex { ReferenceIndex(read_u32(READOUT_PAIRS + 8 * profile.0)) }

    fn reference(&self, index: ReferenceIndex) -> Reference {
        let base = REFERENCES + 0x1c * index.0;
        Reference {
            lines: read_u32(base + 4),
            line_length: read_u32(base + 8),
            frame_period: read_u32(base + 0x10),
            left_reference: read_u16(base + 0x18),
            left: read_u16(base + 0x1a),
        }
    }

    fn shutter_rate(&self, step: u32) -> u32 { read_u32(SHUTTER_RATES + 4 * step) }

    fn decade(&self, step: u32) -> u32 { read_u32(DECADES + 4 * step) }

    fn channel_divisor(&self, channel: u32) -> u16 { read_u16(RATE_DIVISORS + 2 * channel) }

    fn scale_keys(&self, group: u32, entry: u32) -> [AdjustmentKey; 2] {
        let base = SCALE_KEYS + 8 * (entry + 6 * group);
        [AdjustmentKey(read_u32(base)), AdjustmentKey(read_u32(base + 4))]
    }
}

impl LoadTest for StockCapture {
    fn load_test(&mut self) -> Option<LoadTestSetting> {
        (entry!(0x74_2054, fn() -> u32)() != 0).then(|| LoadTestSetting(entry!(0x74_2064, fn() -> u32)()))
    }
}

impl ControlState for StockCapture {
    fn configured_lines(&mut self) -> u16 { read_u16(CONFIGURED_LINES) }

    fn sample_period(&mut self, profile: ProfileHandle) -> (u16, u8) {
        let (mut period, mut flag) = (0u16, 0u8);
        entry!(0x100_6360, fn(u32, *mut u16, *mut u8))(profile.0, &mut period, &mut flag);
        (period, flag)
    }
}

impl Runtime for StockCapture {
    fn log(&self, value: f32) -> f32 { entry!(0x89_4778, fn(f32) -> f32)(value) }

    fn reference_ratio(&mut self, value: u32) -> Option<u32> {
        let mut ratio = 0u32;
        (entry!(0x100_34b4, fn(u32, u32, *mut u32) -> u32)(value, 0, &mut ratio) == 0).then_some(ratio)
    }
}

impl GainSources for StockCapture {
    fn class_gains(&mut self, class: GainClass, column: i16) -> Gains {
        let mut gains = [0u16; 3];
        entry!(0xef_3434, fn(u32, i32, *mut u16))(class.0, i32::from(column), gains.as_mut_ptr());
        gains
    }

    fn custom_gains(&mut self, source: u32, column: i16) -> Gains {
        let mut gains = [0u16; 3];
        entry!(0xef_4fc8, fn(u32, i32, *mut u16))(source, i32::from(column), gains.as_mut_ptr());
        gains
    }

    fn forced_class(&mut self, setting: u8) -> GainClass { GainClass(entry!(0xef_4fd8, fn(u32) -> u32)(u32::from(setting))) }

    fn temperature_offset(&mut self) -> u16 { entry!(0x77_2f28, fn(u32) -> u16)(3) }
}

const fn processing_word_reader(word: ProcessingWord) -> u32 {
    match word {
        ProcessingWord::FirstList => 0xfe_dab8,
        ProcessingWord::SecondList => 0xfe_dad8,
        ProcessingWord::DumpFirst => 0xfe_da78,
        ProcessingWord::DumpSecond => 0xfe_da98,
        ProcessingWord::DumpThird => 0xfe_daf8,
    }
}

impl CapturePipeline for StockCapture {
    fn start_buffers(&mut self, buffers: RecordBuffers) {
        let words = [buffers.primary, buffers.secondary, buffers.primary_bytes, buffers.secondary_bytes];
        entry!(0xfe_90a0, fn(*const u32))(words.as_ptr());
    }

    fn clear_request_state(&mut self) { entry!(0xfe_90c0, fn())() }

    fn output_pending(&mut self) -> bool {
        let mut pending = 0u32;
        entry!(0xff_9bb8, fn(*mut u32))(&mut pending);
        pending != 0
    }

    fn snapshot(&mut self, index: u32, into: &mut PipelineSnapshot) { entry!(0xff_6f94, fn(u32, *mut u8))(index, into.0.as_mut_ptr()) }

    fn snapshot_counter(&mut self) -> u32 {
        let mut counter = 0u32;
        entry!(0xff_6fc0, fn(*mut u32))(&mut counter);
        counter
    }

    fn sample_measurements(&mut self) -> SampleMeasurements {
        let mut index = 0u16;
        let measurements = entry!(0xfe_f8e8, fn(*mut u16) -> u32)(&mut index);
        SampleMeasurements { index, measurements: StockBlock(measurements) }
    }

    fn sample_idle(&mut self) -> bool { entry!(0xfe_f910, fn() -> u32)() != 0 }

    fn saved_snapshot(&mut self, into: &mut SavedSnapshot) { entry!(0xfe_f938, fn(*mut u8))(into.0.as_mut_ptr()) }

    fn capture_record(&mut self, index: u32, into: &mut CaptureRecord) { entry!(0xff_2c5c, fn(u32, *mut u8))(index, into.0.as_mut_ptr()) }

    fn cancel_step_timer(&mut self, index: u32) { entry!(0xfe_c984, fn(u32))(index) }

    fn processing_word(&mut self, word: ProcessingWord) -> u32 { entry!(processing_word_reader(word), fn() -> u32)() }

    fn completed_statistics(&mut self, kind: u8) -> Option<CompletedStatistics> {
        let mut statistics =
            CompletedStatistics { source: 0, parameters: [0; STATISTICS_PARAMETER_BYTES], block: [0; STATISTICS_BLOCK_BYTES] };
        let found = entry!(0xff_cfd0, fn(u32, *mut u32, *mut u8, *mut u8) -> u32)(
            u32::from(kind),
            &mut statistics.source,
            statistics.parameters.as_mut_ptr(),
            statistics.block.as_mut_ptr(),
        );
        (found != 0).then_some(statistics)
    }

    fn configuration_status(&mut self) -> u32 { entry!(0xff_de8c, fn() -> u32)() }

    fn apply_completion_descriptor(&mut self) { entry!(0x100_29bc, fn())() }

    fn control_work_storage(&mut self) -> StockBlock { StockBlock(entry!(0x100_5044, fn() -> u32)()) }
}

impl CaptureResults for StockCapture {
    fn clear_result_completion(&mut self) { entry!(0x102_8adc, fn())() }

    fn set_dispatch_enabled(&mut self, enabled: bool) {
        if enabled { entry!(0x105_4388, fn())() } else { entry!(0x105_43ac, fn())() }
    }

    fn history_status(&mut self) -> u32 { entry!(0x105_38a4, fn() -> u32)() }

    fn clear_history_status(&mut self) { entry!(0x105_386c, fn())() }
}
