use crate::panel_device::*;
use std::vec::Vec;
use std::vec;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Effect { Read(u32), Delay(u32), Serial(u8, u16), Level(u32), Control(u32), ReadColour, Route, Output(bool), Graphic(bool), AudioGate(bool), AudioOutput(bool), Event2, Event4, Complete(u8), Start(u32), Cancel(u32), StoreTimer(u32), Error }
struct Runtime { effects: Vec<Effect>, values: Vec<(u32, u8)>, timer_result: Result<u32, i32>, serial_result: i32, alternate: bool, flip_after_delay: bool, active_target: ActiveTarget }
impl Runtime {
    fn new() -> Self { Self { effects: Vec::new(), values: Vec::new(), timer_result: Ok(37), serial_result: 0, alternate: false, flip_after_delay: false, active_target: ActiveTarget::Other } }
}
impl DeviceRuntime for Runtime {
    fn calibration(&mut self, key: CalibrationKey) -> u8 { self.effects.push(Effect::Read(key.0)); self.values.iter().find(|(k, _)| *k == key.0).map_or(0, |(_, v)| *v) }
    fn alternate_serial_value(&mut self) -> bool { self.alternate }
    fn active_target(&mut self) -> ActiveTarget { self.active_target }
    fn delay(&mut self, ticks: DelayTicks) { self.effects.push(Effect::Delay(ticks.0)); if self.flip_after_delay { self.alternate = true; } }
    fn serial_write(&mut self, register: PanelRegister, value: u16) -> i32 { self.effects.push(Effect::Serial(register.0, value)); self.serial_result }
    fn level_write(&mut self, value: u32) { self.effects.push(Effect::Level(value)); }
    fn panel_control_write(&mut self, value: u32) { self.effects.push(Effect::Control(value)); }
    fn read_colour(&mut self) { self.effects.push(Effect::ReadColour); }
    fn route(&mut self) { self.effects.push(Effect::Route); }
    fn output_enable(&mut self, enabled: bool) { self.effects.push(Effect::Output(enabled)); }
    fn graphic_enable(&mut self, enabled: bool) { self.effects.push(Effect::Graphic(enabled)); }
    fn audio_gate(&mut self, enabled: bool) { self.effects.push(Effect::AudioGate(enabled)); }
    fn audio_output(&mut self, enabled: bool) { self.effects.push(Effect::AudioOutput(enabled)); }
    fn signal_event2(&mut self) { self.effects.push(Effect::Event2); }
    fn signal_event4(&mut self) { self.effects.push(Effect::Event4); }
    fn signal_complete(&mut self, target: u8) { self.effects.push(Effect::Complete(target)); }
    fn timer_start(&mut self, ticks: DelayTicks) -> Result<u32, i32> { self.effects.push(Effect::Start(ticks.0)); self.timer_result }
    fn timer_cancel(&mut self, token: u32) { self.effects.push(Effect::Cancel(token)); }
    fn timer_store(&mut self, token: TimerToken) { self.effects.push(Effect::StoreTimer(token.0)); }
    fn notify_error(&mut self) { self.effects.push(Effect::Error); }
}
#[test]
fn initialization_failure_keeps_program_order_and_reports_timer_error() {
    let mut state = DeviceState::new();
    let mut runtime = Runtime::new();
    runtime.alternate = true;
    runtime.values = vec![(0xeb6, 9), (0xeb7, 11), (0xeb9, 13), (0xfb2, 1)];
    runtime.timer_result = Err(-255);
    assert_eq!(execute(&mut state, SequenceStep::InitializePanel, &mut runtime), ProgrammingOutcome::Finished);
    assert_eq!(&runtime.effects[..7], &[Effect::Read(0xeb6), Effect::Serial(1, 9), Effect::Delay(1), Effect::Read(0xeb7), Effect::Serial(2, 11), Effect::Read(0xeb9), Effect::Serial(3, 13)]);
    let final_effects = &runtime.effects[runtime.effects.len() - 4..];
    assert_eq!(final_effects, &[Effect::Read(0xfb2), Effect::Start(500), Effect::Error, Effect::StoreTimer(0)]);
    assert_eq!(state.timer, TimerToken(0));
    assert!(runtime.effects.contains(&Effect::Delay(84)));
}
#[test]
fn serial_failures_do_not_skip_later_programming_steps() {
    let mut runtime = Runtime::new();
    runtime.serial_result = -30;
    program(&[ProgrammingStep::Write(PanelRegister(4), ValueSource::Constant(25)), ProgrammingStep::Delay(ValueSource::Constant(6)), ProgrammingStep::Write(PanelRegister(9), ValueSource::Constant(77))], &mut runtime);
    assert_eq!(runtime.effects, vec![Effect::Serial(4, 25), Effect::Delay(6), Effect::Serial(9, 77)]);
}
#[test]
fn stop_cancels_timer_before_panel_transfer_and_delay() {
    let mut state = DeviceState::new();
    state.timer = TimerToken(37);
    let mut runtime = Runtime::new();
    runtime.values = vec![(0xfb2, 1), (0xeb6, 12)];
    execute(&mut state, SequenceStep::StopPanel, &mut runtime);
    assert_eq!(runtime.effects, vec![Effect::Read(0xfb2), Effect::Cancel(37), Effect::StoreTimer(0), Effect::Read(0xeb6), Effect::Serial(1, 12), Effect::Delay(100)]);
    assert_eq!(state.timer, TimerToken(0));
}
#[test]
fn level_mode_thirteen_holds_before_control_write() {
    let mut state = DeviceState::new();
    state.format = FormatCode(13);
    let mut runtime = Runtime::new();
    assert_eq!(execute(&mut state, SequenceStep::ProgramLevel, &mut runtime), ProgrammingOutcome::Held);
    assert_eq!(runtime.effects, vec![Effect::ReadColour, Effect::Route, Effect::Read(0xdf5)]);
}
#[test]
fn decrease_only_calibration_does_not_increase_level() {
    let mut state = DeviceState::new();
    state.timing = SourceClass::from_stock(2);
    state.format = FormatCode(1);
    state.programmed_level = PanelLevel(17);
    let mut runtime = Runtime::new();
    runtime.values = vec![(0xde4, 29)];
    assert_eq!(program_level(&mut state, &mut runtime), ProgrammingOutcome::Finished);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5), Effect::Read(0xde4)]);
    assert_eq!(state.programmed_level, PanelLevel(17));
}
#[test]
fn extended_calibration_selects_nonsequential_mode_eight_key() {
    let mut state = DeviceState::new();
    state.format = FormatCode(8);
    let mut runtime = Runtime::new();
    runtime.values = vec![(0x16425, 63)];
    assert_eq!(program_level(&mut state, &mut runtime), ProgrammingOutcome::Finished);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5), Effect::Read(0x16425), Effect::Level(252)]);
    assert_eq!(state.programmed_level, PanelLevel(63));
}
#[test]
fn plan_preserves_software_boundary_and_terminal_completion() {
    let plan = plan(1, 3);
    let mut state = DeviceState::new();
    let mut runtime = Runtime::new();
    assert_eq!(plan.len(), 5);
    assert_eq!(plan.step(0).unwrap(), PlanStep { identifier: 0, callback: Some(SequenceStep::SignalEvent4), linked: false });
    assert_eq!(plan.step(1).unwrap().callback, Some(SequenceStep::InitializePanel));
    assert_eq!(plan.step(2).unwrap().callback, Some(SequenceStep::SignalEvent4));
    assert_eq!(plan.step(3).unwrap().callback, Some(SequenceStep::ProgramLevel));
    execute(&mut state, plan.step(4).unwrap().callback.unwrap(), &mut runtime);
    assert_eq!(runtime.effects, vec![Effect::Complete(3)]);
}
#[test]
fn malformed_stock_copy_is_not_empty_success() {
    let failed = plan(2, 6);
    assert_eq!(failed.fault(), Some(PlanFault::UnboundedStockCopy));
    assert_eq!(failed.step(0), None);
    let unchanged = plan(2, 2);
    assert_eq!(unchanged.fault(), None);
    assert!(unchanged.is_empty());
}
#[test]
fn physical_bits_and_serial_error_poll_are_preserved() {
    assert_eq!(pack_serial(PanelRegister(3), 0x21), 0x621);
    assert!(!SerialStatus { transferred: false, failed: false }.stopped());
    assert!(SerialStatus { transferred: false, failed: true }.stopped());
    assert_eq!(feature_word(0xa5, HdmiFeature::Audio, true), 0xa7);
    assert_eq!(feature_word(0xa7, HdmiFeature::Video, false), 0xa2);
    let mut disabled = RouteState { initialized: false, updating: false, pending: false };
    route_selected(&mut disabled);
    assert!(!disabled.pending);
    let mut active = RouteState { initialized: true, updating: false, pending: false };
    route_selected(&mut active);
    assert!(active.pending);
}

#[test]
fn same_timing_family_updates_state_without_reprogramming() {
    let mut state = DeviceState::new();
    state.timing = SourceClass::from_stock(2);
    let mut runtime = Runtime::new();
    assert_eq!(set_timing(&mut state, BackendDevice::parse(3).unwrap(), SourceClass::from_stock(9), &mut runtime), SetterOutcome::Stored);
    assert_eq!(state.timing, SourceClass::from_stock(9));
    assert!(runtime.effects.is_empty());
}
#[test]
fn active_panel_parameter_change_reprograms_but_secondary_channel_does_not() {
    let mut state = DeviceState::new();
    let mut runtime = Runtime::new();
    runtime.values = vec![(0x16420, 51)];
    assert_eq!(set_parameters(&mut state, BackendDevice::parse(3).unwrap(), HardwareParameters { format: FormatCode(7), second: PhaseCode(0), third: OverrideValue(0) }, &mut runtime), SetterOutcome::Programmed);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5), Effect::Read(0x16420), Effect::Level(204)]);
    runtime.effects.clear();
    assert_eq!(set_channel(&mut state, BackendDevice::parse(3).unwrap(), Channel::Secondary, ChannelValue(9), &mut runtime), SetterOutcome::Stored);
    assert_eq!(state.channel_second, ChannelValue(9));
    assert!(runtime.effects.is_empty());
}

#[test]
fn polarity_is_observed_after_a_delay_changes_the_live_setting() {
    let mut runtime = Runtime::new();
    runtime.flip_after_delay = true;
    runtime.values = vec![(0xeb8, 29), (0xeb9, 61)];
    program(&[ProgrammingStep::Delay(ValueSource::Constant(1)), ProgrammingStep::Write(PanelRegister(3), ValueSource::PanelPolarity)], &mut runtime);
    assert_eq!(runtime.effects, vec![Effect::Delay(1), Effect::Read(0xeb9), Effect::Serial(3, 61)]);
}

#[test]
fn kind_two_channel_change_programs_the_active_panel() {
    let mut state = DeviceState::new();
    state.format = FormatCode(5);
    let mut runtime = Runtime::new();
    runtime.active_target = ActiveTarget::Panel;
    runtime.values = vec![(0x1641e, 43)];
    assert_eq!(set_channel(&mut state, BackendDevice::parse(2).unwrap(), Channel::Primary, ChannelValue(6), &mut runtime), SetterOutcome::Programmed);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5), Effect::Read(0x1641e), Effect::Level(172)]);
    assert_eq!(state.channel_first, ChannelValue(6));
}

#[test]
fn kind_three_channel_change_does_not_enter_held_mode_off_panel() {
    let mut state = DeviceState::new();
    state.format = FormatCode(13);
    let mut runtime = Runtime::new();
    assert_eq!(set_channel(&mut state, BackendDevice::parse(3).unwrap(), Channel::Primary, ChannelValue(6), &mut runtime), SetterOutcome::Stored);
    assert!(runtime.effects.is_empty());
    assert_eq!(state.channel_first, ChannelValue(6));
}

#[test]
fn kind_two_timing_change_enters_held_mode_on_the_active_panel() {
    let mut state = DeviceState::new();
    state.format = FormatCode(13);
    let mut runtime = Runtime::new();
    runtime.active_target = ActiveTarget::Panel;
    assert_eq!(set_timing(&mut state, BackendDevice::parse(2).unwrap(), SourceClass::from_stock(2), &mut runtime), SetterOutcome::Held);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5)]);
    assert_eq!(state.timing, SourceClass::from_stock(2));
}

#[test]
fn kind_three_timing_change_does_not_enter_held_mode_off_panel() {
    let mut state = DeviceState::new();
    state.format = FormatCode(13);
    let mut runtime = Runtime::new();
    assert_eq!(set_timing(&mut state, BackendDevice::parse(3).unwrap(), SourceClass::from_stock(2), &mut runtime), SetterOutcome::Stored);
    assert!(runtime.effects.is_empty());
    assert_eq!(state.timing, SourceClass::from_stock(2));
}

#[test]
fn kind_three_configuration_still_programs_when_the_active_target_is_other() {
    let mut state = DeviceState::new();
    let mut runtime = Runtime::new();
    assert_eq!(set_parameters(&mut state, BackendDevice::parse(3).unwrap(), HardwareParameters { format: FormatCode(13), second: PhaseCode(0), third: OverrideValue(0) }, &mut runtime), SetterOutcome::Held);
    assert_eq!(runtime.effects, vec![Effect::Read(0xdf5)]);
}
