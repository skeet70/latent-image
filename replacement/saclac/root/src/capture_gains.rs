#![forbid(unsafe_code)]

use xm1_saclac_calc::gains::{BlendSelector, CaptureGains, GainRequest as CalculationRequest, GainClass as CalculationClass, SelectedGains};
use xm1_saclac_imaging::capture::{self, gains::{self, GainClass, GainRequest, GainSources, Gains}};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine};
use crate::{services::Services, stock::acquisition::Aligned};

pub(crate) struct Sources<'a, M>(pub(crate) &'a M);
impl<M: Machine> capture::ports::Adjustments for Sources<'_, M> {
    fn byte(&self, key: capture::AdjustmentKey) -> u8 { self.0.enter(0x69_f8fc, &[key.0 as usize]) as u8 }
}
impl<M: Machine> GainSources for Sources<'_, M> {
    fn class_gains(&mut self, class: GainClass, column: i16) -> Gains { gain_triplet(self.0, 0xef_3434, class.0, column) }
    fn custom_gains(&mut self, source: u32, column: i16) -> Gains { gain_triplet(self.0, 0xef_4fc8, source, column) }
    fn forced_class(&mut self, setting: u8) -> GainClass { GainClass(self.0.enter(0xef_4fd8, &[usize::from(setting)])) }
    fn temperature_offset(&mut self) -> u16 { self.0.enter(0x77_2f28, &[3]) as u16 }
}
fn gain_triplet(machine: &impl Machine, entry: u32, class: u32, column: i16) -> Gains {
    let mut bytes = Aligned([0; 8]);
    machine.enter_with(entry, &[Word(class), Word(column as i32 as u32), Buffer(0)], &mut bytes);
    core::array::from_fn(|index| u16::from_le_bytes([bytes[2 * index], bytes[2 * index + 1]]))
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> CaptureGains for Services<M, R, E, P> {
    fn gains(&mut self, request: &CalculationRequest) -> SelectedGains {
        let (gains, class) = gains::gains(&mut Sources(&self.machine), &GainRequest {
            mode: request.mode, variant: request.variant, custom_source: request.custom_source,
            bank: request.bank, fixed: request.fixed, preset: request.preset,
            temperature: request.temperature, column: request.column, level: request.level,
            custom: request.custom, variant_set: request.variant_set, forced: request.forced,
            variant_subset: request.variant_subset,
        });
        SelectedGains { gains, class: CalculationClass(class.0) }
    }
    fn blend_weight(&mut self, selector: &BlendSelector) -> u8 {
        gains::blend_weight(&Sources(&self.machine), selector.mode, selector.variant, selector.column, selector.sensitivity, selector.preset)
    }
}
