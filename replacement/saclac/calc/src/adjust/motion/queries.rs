//! The motion layer's answers to the measurement pipeline's queries between a run's steps.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_29;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::motion::ports::{Clock, LensTables};
use crate::adjust::motion::state::MotionState;
use crate::exposure::ports::{ProfileAccess, Properties, StockMemory};
use crate::exposure::settings;
use crate::stock::motion::{self as trace, PEAK_CHECK, PEAK_THRESHOLD, RESULT_AREAS, STEP_SCALE_HIGH, STEP_SCALE_LOW, STEP_SCALE_SELECTOR};

/// `value` in tenths, rounded half away from zero (0x741fa0).
pub(crate) fn round_tenths(value: i32) -> i32 {
    let quotient = value / 10;
    match value % 10 {
        remainder if remainder > 4 => quotient + 1,
        remainder if remainder < -4 => quotient - 1,
        _ => quotient,
    }
}

/// Whether the measurement may take its second channel for `record` (0xf9d414).
pub fn second_channel_available(ports: &mut impl ProfileAccess, record: RecordHandle) -> bool {
    let mut profile = ports.profile();
    if profile.read_shared(record, &group_29::AT_079C) == 0 {
        return false;
    }
    let refused = if profile.header(record, SharedHeaderByte::At47) == 0 {
        profile.read(record, &group_00::AT_0067) != 0 || profile.read(record, &group_00::AT_0058) == 2
    } else {
        profile.read(record, &group_00::AT_0070) != 0
    };
    !refused && profile.read(record, &group_00::AT_003E) == 0
}

/// The table offset of `position` from the lens's channel position at `step`, scaled down
/// toward the configured low bound by `kind`'s scale, in steps (0xfa3534).
pub fn step_offset(ports: &mut (impl LensTables + Properties), lens: u32, step: u32, position: i16, side: u32, reference: u32, kind: u32) -> i16 {
    let [base, _] = ports.channel_positions(lens, step, reference, 0);
    let base = ports.side_adjusted(base, 1, side);
    let offset = ports.table_offset(lens, step, position.wrapping_sub(base));
    let high = i32::from(settings::word(ports, STEP_SCALE_HIGH));
    let low = i32::from(settings::word(ports, STEP_SCALE_LOW));
    if high <= low {
        return 0;
    }
    let selector = match ports.property(STEP_SCALE_SELECTOR) & 0xf {
        selector if selector > 2 => 0,
        selector => selector,
    };
    let scale = i32::from(settings::word(ports, trace::step_scale(kind, selector))).clamp(low, high);
    let reduction = (low - scale).wrapping_mul(offset) / (high - low);
    round_tenths(offset.wrapping_add(reduction)) as i16
}

/// Whether a measured area's level in `results` has reached the configured peak level; the
/// threshold is recorded in the results when they hold a measurement (0xf9620c).
pub(crate) fn level_peaked(ports: &mut (impl Properties + StockMemory), results: SharedAddress) -> bool {
    if ports.read_u32(trace::results_present(results)) == 0 {
        return false;
    }
    let threshold = u32::from(settings::word(ports, PEAK_THRESHOLD));
    ports.write_u32(trace::results_threshold(results), threshold);
    (0..RESULT_AREAS).any(|area| {
        ports.read_u32(trace::result_measured(results, area)) == 1 && trace::result_levels(results, area).into_iter().any(|level| threshold <= ports.read_u32(level))
    })
}

impl MotionState {
    /// Whether the pipeline set its flag in the dump frame (0xf9d3dc).
    pub fn pipeline_flag(&self, memory: &mut impl StockMemory) -> bool { memory.read_u32(trace::pipeline_flag(self.section)) != 0 }

    /// Records the time since preparation in the dump frame (0xf9cda4).
    pub fn trace_elapsed(&self, ports: &mut (impl Clock + StockMemory)) {
        let elapsed = ports.elapsed(self.prepared_at);
        ports.write_u32(trace::elapsed(self.section), elapsed);
    }

    /// Whether a measured area's level has reached the configured peak level, when the run
    /// checks for it (0xf9ed44).
    pub fn peak_reached(&self, ports: &mut (impl Properties + StockMemory)) -> bool {
        ports.property(PEAK_CHECK) != 0 && self.plan.checks_peak && level_peaked(ports, self.results)
    }
}
