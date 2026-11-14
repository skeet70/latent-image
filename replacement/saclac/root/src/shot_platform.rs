#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_search_bindings::stock as search;
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::{Argument::Buffer, Machine}};
use xm1_shot_sequence::boundary::{Adjustment, Hold, Platform};
use crate::shot_services::Libraries;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Platform for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn hold(&mut self, hold: Hold, held: bool) {
        let channel = match hold { Hold::Preparation => 4 };
        let machine = self.services.borrow().machine.clone();
        machine.enter(if held { 0x740d3c } else { 0x740d50 }, &[channel]);
    }
    fn adjustment(&mut self, adjustment: Adjustment) -> u8 {
        let key = match adjustment { Adjustment::TrackedSubjectsWide => 0x1307, Adjustment::StatisticsDump => 0xa8 };
        self.services.borrow().stock.byte(AdjustmentKey(key))
    }
    fn level(&mut self) -> u8 {
        let machine = self.services.borrow().machine.clone();
        let mut output = [0; 1];
        machine.enter_with(0xee8e70, &[Buffer(0)], &mut output);
        output[0]
    }
    fn tick(&mut self) -> u32 {
        let machine = self.services.borrow().machine.clone();
        let mut output = [0; 4];
        // f15e60 supplies the output to743dc8, whose body calls7257a8 unchanged.
        search::buffer(&machine, search::TICKS, &[search::out(0)], &mut output);
        u32::from_le_bytes(output)
    }
    fn reset_image_progress(&mut self) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(0x105386c, &[]);
    }
}
