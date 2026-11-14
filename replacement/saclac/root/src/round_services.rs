#![forbid(unsafe_code)]

use xm1_focus_measurement_round::{planner::{Cadence, Mode, Work}, profile::FrameRate, record::{AlternateSearch, Power, Settings}, replies::{Effect, Effects}, stock as round};
use xm1_saclac_interface::{round::GeometryCalibration, stock::callback_word};
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{commands::CommandMemory, settings::{AdjustmentKey, Adjustments}, stock::Machine};
use crate::services::Services;

#[derive(Clone, Copy)]
pub struct Callbacks {
    pub search_mode: xm1_saclac_interface::stock::StockCallbackWord,
    pub window: xm1_saclac_interface::stock::StockCallbackWord,
    pub auxiliary: xm1_saclac_interface::stock::StockCallbackWord,
    pub source: xm1_saclac_interface::stock::StockCallbackWord,
    pub image: xm1_saclac_interface::stock::StockCallbackWord,
    pub snapshot: xm1_saclac_interface::stock::StockCallbackWord,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    pub fn bind_round_callbacks(&mut self, callbacks: Callbacks) { self.round_callbacks = Some(callbacks); }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Settings for Services<M, R, E, P> {
    fn search_permitted(&mut self) -> bool { round::search_allowed(self.machine.enter(0x0069_f8fc, &[0x3e0009])) }
    fn alternate_search(&mut self) -> AlternateSearch { round::alternate_search_rule(round::SearchModeFlags(self.machine.enter(0x0069_f8fc, &[0x88ef]))) }
    fn cadence(&mut self, work: Work, mode: Mode) -> Cadence {
        let key = round::cadence_property(work, mode).expect("the planner does not query cadence for unconditional work");
        round::cadence(u32::from(self.stock.byte(AdjustmentKey(key.word()))))
    }
    fn frame_rate_threshold(&mut self) -> FrameRate { FrameRate(u16::from(self.stock.byte(AdjustmentKey(round::FRAME_RATE_THRESHOLD.word())))) }
    fn keeps_interval_history(&mut self) -> bool { self.stock.byte(AdjustmentKey(0x5256)) & 2 != 0 }
    fn geometry_calibration(&mut self) -> GeometryCalibration {
        GeometryCalibration([0x8797, 0x8798, 0x8799, 0x879a].map(|key| self.machine.enter(0x0069_f8fc, &[key]) as u16))
    }
    fn traces_measurement(&mut self) -> bool { self.machine.enter(0x0069_f8fc, &[0x24e]) == 1 }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Power for Services<M, R, E, P> {
    fn request_measurement_power(&mut self, held: bool) {
        // 0xed80a8/0xed80c4 enter the external route service with the third argument zero.
        self.machine.enter(0x0064_982c, &[13, if held { 8 } else { 0 }, 0]);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Effects for Services<M, R, E, P> {
    fn emit(&mut self, effect: Effect<'_>) {
        match effect {
            Effect::ClearPending(target) => self.stock.set_pending(target, false),
            Effect::Deliver { callback, status, values, .. } => {
                let bytes = values.bytes();
                assert!(bytes.len() % 4 == 0 && bytes.len() <= 12, "round reply exceeds its scalar callback ABI");
                let mut arguments = [0; 4];
                arguments[0] = status.0 as usize;
                for (argument, word) in arguments[1..].iter_mut().zip(bytes.chunks_exact(4)) {
                    *argument = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) as usize;
                }
                self.machine.enter(callback_word(callback), &arguments[..1 + bytes.len() / 4]);
            }
        }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: xm1_saclac_interface::Calc + xm1_saclac_interface::Calc1 + xm1_saclac_interface::ImgDc> xm1_focus_measurement_round::record::Ports for Services<M, R, E, P> {
    fn request(&mut self, request: xm1_focus_measurement_round::record::Request) {
        use xm1_focus_measurement_round::record::{Request, SearchMode};
        use xm1_saclac_interface::{Calc, Calc1, Calc1Request, CalcRequest, Completion, ImgDc, ImgDcOperation, ImgDcRequest, stock as wire};
        let callbacks = self.round_callbacks.expect("round completion callbacks must be bound before dispatch");
        let refused = match request {
            Request::SearchMode(mode) => {
                let mode: u32 = match mode { SearchMode::Search { profile_selected: false } => 2, SearchMode::Search { profile_selected: true } => 3, SearchMode::Image => 4 };
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&callbacks.search_mode.0.to_le_bytes());
                bytes[4..].copy_from_slice(&mode.to_le_bytes());
                ImgDc::request(&mut self.peers, ImgDcRequest { operation: ImgDcOperation::Request2a, parameters: wire::parameters(&bytes), completions: &[Completion::Image45] }).is_err()
            }
            Request::Geometry(geometry) => {
                let bytes = wire::measurement_geometry(geometry);
                ImgDc::request(&mut self.peers, ImgDcRequest { operation: ImgDcOperation::Request2b, parameters: wire::parameters(&bytes), completions: &[] }).is_err()
            }
            Request::Window(request) => {
                let (operation, bytes) = wire::calc_payload(&round::window_payload(request, callbacks.window));
                Calc::request(&mut self.peers, CalcRequest { operation, parameters: wire::parameters(bytes.bytes()), completions: &[Completion::Calculation8d] }).is_err()
            }
            Request::Auxiliary(request) => {
                let (operation, bytes) = wire::calc1_payload(&round::auxiliary_payload(request, callbacks.auxiliary));
                Calc1::request(&mut self.peers, Calc1Request { operation, parameters: wire::parameters(bytes.bytes()), completions: &[Completion::Calculation8a] }).is_err()
            }
            Request::Source(request) => {
                let (operation, bytes) = wire::calc_payload(&round::source_payload(request, callbacks.source));
                Calc::request(&mut self.peers, CalcRequest { operation, parameters: wire::parameters(bytes.bytes()), completions: &[Completion::Calculation9a] }).is_err()
            }
        };
        if refused { crate::stock::acquisition::fault(&self.machine); }
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: xm1_saclac_interface::Calc + xm1_saclac_interface::Calc1 + xm1_saclac_interface::ImgDc> xm1_focus_measurement_round::window::Ports for Services<M, R, E, P> {
    fn window_result(&mut self, completed: xm1_saclac_interface::Completed<'_>) -> Option<xm1_focus_measurement_round::window::WindowResult> {
        use xm1_focus_measurement_round::profile::{WindowPosition, WindowStatus};
        if completed.completion != xm1_saclac_interface::Completion::Calculation8d { return None; }
        let bytes = completed.parameters.bytes();
        if bytes.len() != 8 { return None; }
        Some(xm1_focus_measurement_round::window::WindowResult {
            status: WindowStatus(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
            position: WindowPosition([u16::from_le_bytes([bytes[4], bytes[5]]), u16::from_le_bytes([bytes[6], bytes[7]])]),
        })
    }
}
