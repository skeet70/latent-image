#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::{lens::{BiasTable, LensConfiguration, LensControl, LensSteps, Step, StepRequest}, motion::ports::LensTables, profile_adjustment::{LensSteps as ProfileSteps, StepRatios, StepTable}};
use xm1_saclac_config::{curve, numeric::FloatLog, table::{self, Bank, Bounds, Row}};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::calc::Tables;
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::{Argument::{Buffer, Word}, Machine}};
use crate::{services::Services, stock::acquisition::Aligned};

pub(crate) struct Log<'a, M>(pub(crate) &'a M);
impl<M: Machine> FloatLog for Log<'_, M> {
    fn ln(&mut self, value: f32) -> f32 { f32::from_bits(self.0.enter(0x89_4778, &[value.to_bits() as usize])) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensConfiguration for Services<M, R, E, P> {
    fn channel_value(&mut self, channel: i32) -> u32 { Tables(&self.machine).channel_value(channel) }
    fn channel_curve(&mut self, channel: i32) -> u32 { Tables(&self.machine).channel_curve(channel) }
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16 { Tables(&self.machine).bias(resource, table) }
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16 { Tables(&self.machine).biased_position(resource, table) }
    fn resource_count(&mut self) -> u8 { Tables(&self.machine).resource_count() }
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32 { Tables(&self.machine).selection_word(channel, flags, offset) }
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32 { Tables(&self.machine).remap(word, channel, mode) }
    fn position_limits(&mut self, key: u32) -> [i16; 2] { Tables(&self.machine).position_limits(key) }
    fn mapped_value(&mut self, key: u32) -> u32 { Tables(&self.machine).mapped_value(key) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensControl for Services<M, R, E, P> {
    fn position(&mut self) -> u16 { Tables(&self.machine).position() }
    fn travel_margin(&mut self) -> u16 { Tables(&self.machine).travel_margin() }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensSteps for Services<M, R, E, P> {
    fn step(&mut self, request: StepRequest) -> Step { Tables(&self.machine).step(request) }
    fn travel_distance(&mut self, resource: u32, index: u32, travel: i16) -> u16 { Tables(&self.machine).travel_distance(resource, index, travel) }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> ProfileSteps for Services<M, R, E, P> {
    fn near_step(&mut self, reference: u32, limit: u16, which: StepTable) -> u32 {
        let which = match which { StepTable::Standard => Bounds::Primary, StepTable::Lens => Bounds::Selected };
        table::lower_step(&self.stock, Row(reference), xm1_saclac_config::stock::decode_step_limit(u32::from(limit)), which).0
    }
    fn far_step(&mut self, reference: u32, limit: u16, which: StepTable) -> u32 {
        let which = match which { StepTable::Standard => Bounds::Primary, StepTable::Lens => Bounds::Selected };
        table::upper_step(&self.stock, Row(reference), xm1_saclac_config::stock::decode_step_limit(u32::from(limit)), which).0
    }
    fn lens_step(&mut self, step: u32) -> u32 {
        match table::bank(&self.stock, table::Step(step)) {
            Bank::First => step.wrapping_add(31),
            Bank::Second => step,
            Bank::Outside => 0,
        }
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> StepRatios for Services<M, R, E, P> {
    fn step_ratio(&mut self, code: u16, selector: u8) -> u32 {
        let entry = 0x0102_2f54u32.wrapping_add(
            u32::from(selector).wrapping_add(u32::from(code).wrapping_mul(6)).wrapping_mul(8),
        );
        let configured_halfword = |key: u32| {
            u32::from(u16::from_le_bytes([
                self.stock.byte(AdjustmentKey(key)),
                self.stock.byte(AdjustmentKey(key.wrapping_add(1))),
            ]))
        };
        let denominator = configured_halfword(self.machine.read_u32(entry)).max(1);
        let numerator = configured_halfword(self.machine.read_u32(entry.wrapping_add(4))).wrapping_mul(1000);
        let ratio = numerator / denominator;
        (ratio / 10).wrapping_add(u32::from(ratio % 10 > 4))
    }
}
impl<M: Machine + Clone, R: Memory, E: StockEntries, P> LensTables for Services<M, R, E, P> {
    fn channel_positions(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> [i16; 2] {
        let mut output = Aligned([0; 4]);
        self.machine.enter_with(0x74_98e0, &[Word(lens), Word(step), Word(reference), Word(mode), Buffer(0), Buffer(2)], &mut output.0);
        [i16::from_le_bytes([output[0], output[1]]), i16::from_le_bytes([output[2], output[3]])]
    }
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16 {
        xm1_power_lifecycle::record_profile_mapping::helper_747aac(&mut crate::calibration_inputs::runtime(&self.machine), value as i32 as u32, from, to).expect("lens direction configuration") as i16
    }
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32 { i32::from(curve::curve_value(&self.stock, lens, step, i32::from(delta))) }
    fn channel_start(&mut self, lens: u32, channel: u8, reference: u32) -> i16 { self.machine.enter(0x74_8730, &[lens as usize, 0, usize::from(channel), 0, reference as usize, 1]) as i16 }
    fn period_position(&mut self, period: u16, lens: u32, step: u32, reference: u32) -> i32 { self.machine.enter(0x74_99a0, &[usize::from(period), lens as usize, step as usize, reference as usize, 1]) as i32 }
}
