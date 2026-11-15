#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::{metering::MeteringTables, motion::{drive::DriveKind, ports::*, preparation::{CaptureFrames, FrameSize, LensStatus}}, peak::Logarithms, timing::CaptureTiming}, exposure::{correction, ports::Placement}};
use xm1_saclac_config::{dump::DumpDerivations, table::{Row, Step}};
use xm1_saclac_imaging::{capture::gains::{GainClass, GainSources}, driver::Isp};
use xm1_saclac_interface::{ImageProcessor, RecordHandle};
use xm1_saclac_profile_geometry::{controller::Tables, ports::Retained, profile::{ProfileHandle, ProfileTables}};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::calc::Tables as Calibration;
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};
use crate::{calculation_services::Calculation, calibration_inputs::Native, capture_gains::Sources, stock::acquisition::Aligned};

macro_rules! implement {
    ($contract:path { $($methods:tt)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement>
            $contract for Calculation<'_, M, R, E, P, I, T, A> { $($methods)* }
    };
}

implement!(Clock {
    fn now(&mut self) -> Ticks {
        let mut output = Aligned([0; 4]);
        self.services.machine.enter_with(0x7257a8, &[Buffer(0)], &mut output.0);
        Ticks(u32::from_le_bytes(output.0))
    }
    fn elapsed(&mut self, since: Ticks) -> u32 { self.now().0.wrapping_sub(since.0).wrapping_mul(10) }
});
implement!(LensTables {
    fn channel_positions(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> [i16; 2] { self.services.channel_positions(lens, step, reference, mode) }
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16 { self.services.side_adjusted(value, from, to) }
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32 { self.services.table_offset(lens, step, delta) }
    fn channel_start(&mut self, lens: u32, channel: u8, reference: u32) -> i16 { self.services.channel_start(lens, channel, reference) }
    fn period_position(&mut self, period: u16, lens: u32, step: u32, reference: u32) -> i32 { self.services.period_position(period, lens, step, reference) }
});
implement!(LensWords {
    fn position_rate(&mut self, position: i16, lens: u32, step: u32, reference: u32) -> u32 {
        self.services.machine.enter(0x749be4, &[position as isize as usize, lens as usize, step as usize, reference as usize, 1])
    }
    fn curve_word(&mut self, lens: u32) -> u32 {
        xm1_saclac_config::channel::scaled_packed(&self.services.stock, xm1_saclac_shared::settings::Channel(lens as i32))
    }
    fn mapped_step_word(&mut self, step: u32, lens: u32) -> u32 {
        u32::from(Native(&self.services.machine, &self.services.stock).transformed_word(Step(step), Row(lens)))
    }
});
implement!(LensCalibration {
    fn steps(&mut self) -> u32 { self.services.machine.enter(0x74677c, &[]) }
    fn lowest_step(&mut self, lens: u32) -> u32 { self.services.machine.enter(0x746cc8, &[lens as usize, 0xffff]) }
    fn highest_step(&mut self, lens: u32) -> u32 { self.services.machine.enter(0x746d6c, &[lens as usize, 0xffff]) }
    fn depth_positions(&mut self, lens: u32, step: u32, depth: u16) -> i16 { self.services.machine.enter(0x749254, &[lens as usize, step as usize, usize::from(depth)]) as i16 }
    fn table_entry(&mut self, lens: u32, table: xm1_saclac_calc::exposure::ports::Property, entries: u32) -> u32 { self.services.machine.enter(0x747304, &[lens as usize, table.key() as usize, entries as usize]) }
    fn lens_state(&mut self) -> u32 { self.services.machine.enter(0x746054, &[]) }
    fn drive_kind(&mut self) -> DriveKind { DriveKind(self.services.machine.enter(0x76f360, &[])) }
    fn lens_limits(&mut self, query: &LimitQuery) -> LensLimits {
        let mut output = Aligned([0; 4]);
        //74b11c tests argument10;74b584/590 store the two halfwords.
        self.services.machine.enter_with(0x74b05c, &[
            Word(0), Word(u32::from(query.channel)), Word(query.lens), Word(0), Word(query.step), Buffer(0), Buffer(2),
            Word(query.reference), Word(1), Word(u32::from(query.with_margins)), Word(0), Word(query.lens_state),
        ], &mut output.0);
        LensLimits { low: i16::from_le_bytes([output[0], output[1]]), high: i16::from_le_bytes([output[2], output[3]]) }
    }
});
implement!(LensBounds {
    fn search_bounds(&mut self, query: &BoundsQuery) -> SearchBounds {
        let mut output = Aligned([0; 4]);
        //f9e5ac..f9e600 inserts1 at argument12, unlike round area's0 there.
        self.services.machine.enter_with(0x74c568, &[
            Word(query.lens), Word(0), Word(u32::from(query.scene)), Word(u32::from(query.channel)), Word(u32::from(query.class)), Word(query.step),
            Buffer(0), Buffer(2), Word(1), Word(0), Word(query.reference), Word(1), Word(u32::from(query.held)), Word(query.lens_state),
        ], &mut output.0);
        SearchBounds { low: i16::from_le_bytes([output[0], output[1]]), high: i16::from_le_bytes([output[2], output[3]]) }
    }
});
implement!(ExposureTimes {
    fn exposure_time(&mut self, profile: u32, level: i16, gain: u32) -> u32 {
        let mut output = Aligned([0; 4]);
        self.services.machine.enter_with(0x10061ec, &[Word(profile), Word(level as i32 as u32), Word(gain), Buffer(0)], &mut output.0);
        u32::from_le_bytes(output.0)
    }
});
implement!(ProfileGains {
    fn gain_level(&mut self, profile: u32, gain: u32) -> i16 {
        let mut output = Aligned([0; 2]);
        self.services.machine.enter_with(0x1006000, &[Word(profile), Word(gain), Buffer(0)], &mut output.0);
        i16::from_le_bytes(output.0)
    }
    fn shortest_time(&mut self, profile: u32) -> u32 {
        let mut output = Aligned([0; 4]);
        self.services.machine.enter_with(0x1005f60, &[Word(profile), Buffer(0)], &mut output.0);
        u32::from_le_bytes(output.0)
    }
});
implement!(MeasurementReadings {
    fn selected_area(&mut self, record: RecordHandle) -> GridArea {
        let [column, row] = correction::ratio(self, record);
        GridArea { column, row }
    }
    fn selection_value(&mut self, record: RecordHandle, _: u32) -> u32 { self.services.records.chosen_word(record).unwrap_or(0) }
});
implement!(LensStatus { fn control_state(&mut self) -> u32 { self.services.machine.enter(0x76da38, &[]) } });
impl<M: Machine + Clone, R, E, P, I, T: ProfileTables, A> CaptureFrames for Calculation<'_, M, R, E, P, I, T, A> {
    fn frame_size(&mut self, mode: u32) -> FrameSize {
        let frame = self.geometry.ports().relative_rectangle(ProfileHandle(mode), true);
        //f991e0..f991fc requests relative1 for both capture modes.
        FrameSize { height: frame.height, width: frame.width }
    }
}
implement!(ChannelRates { fn channel_rate(&mut self, lens: u32, channel: u8) -> u32 { self.services.machine.enter(0x749e40, &[lens as usize, usize::from(channel)]) } });
implement!(CaptureTiming {
    fn readout_start(&mut self, source: u32) -> u32 { Calibration(&self.services.machine).readout_start(source) }
    fn reference_line(&mut self, source: u32) -> u16 { Calibration(&self.services.machine).reference_line(source) }
    fn line_period(&mut self, source: u32) -> Option<u32> { Calibration(&self.services.machine).line_period(source) }
});
implement!(Logarithms { fn centi_log2(&mut self, value: u64) -> Option<u32> { Calibration(&self.services.machine).centi_log2(value) } });
implement!(Logarithms10 { fn log10(&mut self, value: f32) -> f32 { f32::from_bits(self.services.machine.enter(0xee996c, &[value.to_bits() as usize])) } });
implement!(MeteringTables {
    fn channel_weights(&mut self) -> [u16; 3] { Sources(&self.services.machine).class_gains(GainClass(5), 0) }
    fn feature_bytes(&mut self, profile: u32) -> [u8; 2] { self.geometry.feature_scales(ProfileHandle(profile)) }
});
