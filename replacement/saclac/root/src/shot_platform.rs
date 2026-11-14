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

#[derive(Default)]
pub(crate) struct PipelineState {
    pub(crate) restart: bool,
    pub(crate) frame: Option<xm1_shot_sequence::boundary::Frame>,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    xm1_shot_sequence::pipeline_work::PipelinePlatform for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn setting(&mut self, setting: xm1_shot_sequence::pipeline_work::PipelineSetting) -> u32 {
        use xm1_shot_sequence::stock::pipeline::{setting_entry, SettingReader};
        let (reader, key) = setting_entry(setting);
        let services = self.services.borrow();
        match reader {
            SettingReader::Byte => u32::from(services.stock.byte(AdjustmentKey(key))),
            SettingReader::Halfword => u32::from(xm1_saclac_shared::settings::halfword(&services.stock, AdjustmentKey(key))),
            SettingReader::SignedByte => xm1_saclac_shared::settings::signed_byte(&services.stock, AdjustmentKey(key)) as i32 as u32,
        }
    }
    fn load_test_state(&mut self) -> u32 { self.services.borrow().machine.enter(0x74_2054, &[]) }
    fn load_test_setting(&mut self) -> u32 { self.services.borrow().machine.enter(0x74_2064, &[]) }
    fn load_test_register(&mut self, register: u8) -> u8 {
        self.services.borrow().machine.enter(0x74_2088, &[usize::from(register)]) as u8
    }
    fn set_load_test_register(&mut self, register: u8, value: u8) {
        self.services.borrow().machine.enter(0x74_2098, &[usize::from(register), usize::from(value)]);
    }
    fn trace(&mut self, trace: xm1_shot_sequence::pipeline_work::PipelineTrace) {
        self.services.borrow().machine.enter(0x72_872c, &[xm1_shot_sequence::stock::pipeline::trace_word(trace) as usize]);
    }
    fn stamp_pipeline_frame(&mut self, frame: xm1_shot_sequence::boundary::Frame, words: &xm1_shot_sequence::pipeline_work::PipelineFrame) {
        use xm1_shot_sequence::stock::pipeline as layout;
        let mut machine = self.services.borrow().machine.clone();
        for (offset, word) in [
            layout::FRAME_MARKER, (layout::FRAME_RESOURCE, words.resource), (layout::FRAME_BANK, words.bank),
            (layout::FRAME_LENS_POSITION, words.lens_position), (layout::FRAME_LENS_REFERENCE, words.lens_reference),
            layout::FRAME_CALIBRATION_MARKER,
        ] { machine.write_u32(frame.0.wrapping_add(offset), word); }
    }
    fn clear_frame_work(&mut self, frame: xm1_shot_sequence::boundary::Frame) {
        self.services.borrow().machine.clone().write_u32(frame.0.wrapping_add(xm1_shot_sequence::stock::pipeline::FRAME_WORK), 0);
    }
    fn store_bound_frame(&mut self, frame: Option<xm1_shot_sequence::boundary::Frame>) {
        self.services.borrow_mut().pipeline.frame = frame;
    }
    fn restart_marked(&mut self) -> bool { self.services.borrow().pipeline.restart }
    fn mark_restart(&mut self) { self.services.borrow_mut().pipeline.restart = true; }
    fn store_work(&mut self, area: xm1_shot_sequence::pipeline_work::WorkArea, store: xm1_shot_sequence::pipeline_work::WorkStore) {
        let mut machine = self.services.borrow().machine.clone();
        xm1_shot_sequence::stock::pipeline::work_stores(store, |offset, bytes| {
            for (index, &byte) in bytes.iter().enumerate() { machine.write_u8(area.0.wrapping_add(offset).wrapping_add(index as u32), byte); }
        });
    }
    fn work_marked(&mut self, area: xm1_shot_sequence::pipeline_work::WorkArea) -> bool {
        self.services.borrow().machine.read_u32(area.0.wrapping_add(xm1_shot_sequence::stock::pipeline::WORK_MARK)) == 1
    }
    fn flush_work(&mut self, area: xm1_shot_sequence::pipeline_work::WorkArea) {
        self.services.borrow().machine.enter(0x74_0e3c, &[area.0 as usize, xm1_shot_sequence::stock::pipeline::WORK_BYTES as usize]);
    }
}
