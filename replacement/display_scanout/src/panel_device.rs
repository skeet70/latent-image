#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceClass(u32);
impl SourceClass {
    pub const fn from_stock(value: u32) -> Self { Self(value) }
    pub const fn code(self) -> u32 { self.0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationKey(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelRegister(pub u8);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayTicks(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatCode(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureValue(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhaseCode(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverrideValue(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerToken(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PanelLevel(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtendedCalibration { Five, Six, Seven, Eight, Nine, Ten, Eleven, Twelve }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibrationMode { Disabled, Phase, Level, DecreaseOnly, AlternateLevel, Extended(ExtendedCalibration), Held }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceState {
    pub timing: SourceClass,
    pub source: SourceClass,
    pub format: FormatCode,
    pub first_flag: FeatureValue,
    pub second_flag: FeatureValue,
    pub channel_first: ChannelValue,
    pub channel_second: ChannelValue,
    pub transition_enabled: FeatureValue,
    pub parameter_second: PhaseCode,
    pub parameter_third: OverrideValue,
    pub timer: TimerToken,
    pub programmed_level: PanelLevel,
    pub alternate_serial_value: bool,
}
impl DeviceState {
    pub const fn new() -> Self { Self { timing: SourceClass(11), source: SourceClass(0), format: FormatCode(0), first_flag: FeatureValue(0), second_flag: FeatureValue(1), channel_first: ChannelValue(5), channel_second: ChannelValue(5), transition_enabled: FeatureValue(0), parameter_second: PhaseCode(0), parameter_third: OverrideValue(0), timer: TimerToken(0), programmed_level: PanelLevel(0), alternate_serial_value: false } }
    pub const fn source_code(&self) -> u32 { self.source.code() }
    pub fn calibration_mode(&self) -> CalibrationMode {
        if self.format.0 == 13 { return CalibrationMode::Held; }
        let extended = match self.format.0 { 5 => Some(ExtendedCalibration::Five), 6 => Some(ExtendedCalibration::Six), 7 => Some(ExtendedCalibration::Seven), 8 => Some(ExtendedCalibration::Eight), 9 => Some(ExtendedCalibration::Nine), 10 => Some(ExtendedCalibration::Ten), 11 => Some(ExtendedCalibration::Eleven), 12 => Some(ExtendedCalibration::Twelve), _ => None };
        if let Some(mode) = extended { return CalibrationMode::Extended(mode); }
        if self.second_flag.0 == 0 { return CalibrationMode::Disabled; }
        if matches!(self.timing.code(), 9 | 2 | 4) {
            if self.parameter_second.0 != 3 && self.format.0 == 1 { CalibrationMode::DecreaseOnly }
            else if self.format.0 == 3 { CalibrationMode::AlternateLevel }
            else if self.parameter_second.0 == 3 || self.parameter_third.0 == 1 { CalibrationMode::Phase }
            else { CalibrationMode::Level }
        } else if self.parameter_second.0 == 3 { CalibrationMode::Phase } else { CalibrationMode::Level }
    }
}
impl Default for DeviceState { fn default() -> Self { Self::new() } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SequenceStep {
    SignalEvent2, SignalEvent4, Complete(u8), GraphicDisable, GraphicEnable,
    DisablePanel, EnableHdmi, DisableHdmi,
    AudioGateOn, AudioGateOff, EnableAudio, DisableAudio,
    InitializePanel, StopPanel, ProgramLevel, Noop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlanStep { pub identifier: u32, pub callback: Option<SequenceStep>, pub linked: bool }
impl PlanStep { const EMPTY: Self = Self { identifier: 0, callback: None, linked: false }; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanFault { UnboundedStockCopy, InvalidTransition }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SequencePlan { steps: [PlanStep; 32], count: usize, fault: Option<PlanFault> }
impl SequencePlan {
    pub const fn empty() -> Self { Self { steps: [PlanStep::EMPTY; 32], count: 0, fault: None } }
    pub const fn len(&self) -> usize { self.count }
    pub const fn is_empty(&self) -> bool { self.count == 0 }
    pub const fn fault(&self) -> Option<PlanFault> { self.fault }
    pub fn step(&self, index: usize) -> Option<PlanStep> { if index < self.count { Some(self.steps[index]) } else { None } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgrammingOutcome { Finished, Held }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueSource { Constant(u8), Calibration(CalibrationKey), PanelPolarity }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgrammingStep { Write(PanelRegister, ValueSource), Delay(ValueSource) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveTarget { Panel, Other }
pub trait DeviceRuntime {
    fn calibration(&mut self, key: CalibrationKey) -> u8;
    fn alternate_serial_value(&mut self) -> bool;
    fn active_target(&mut self) -> ActiveTarget;
    fn delay(&mut self, ticks: DelayTicks);
    fn serial_write(&mut self, register: PanelRegister, value: u16) -> i32;
    fn level_write(&mut self, value: u32);
    fn panel_control_write(&mut self, value: u32);
    fn read_colour(&mut self);
    fn route(&mut self);
    fn output_enable(&mut self, enabled: bool);
    fn graphic_enable(&mut self, enabled: bool);
    fn audio_gate(&mut self, enabled: bool);
    fn audio_output(&mut self, enabled: bool);
    fn signal_event2(&mut self);
    fn signal_event4(&mut self);
    fn signal_complete(&mut self, target: u8);
    fn timer_start(&mut self, ticks: DelayTicks) -> Result<u32, i32>;
    fn timer_cancel(&mut self, token: u32);
    fn timer_store(&mut self, token: TimerToken);
    fn notify_error(&mut self);
}
fn value(source: ValueSource, runtime: &mut impl DeviceRuntime) -> u8 {
    match source {
        ValueSource::Constant(v) => v,
        ValueSource::Calibration(k) => runtime.calibration(k),
        ValueSource::PanelPolarity => { let alternate = runtime.alternate_serial_value(); runtime.calibration(CalibrationKey(if alternate { 0xeb9 } else { 0xeb8 })) }
    }
}
pub fn program(steps: &[ProgrammingStep], runtime: &mut impl DeviceRuntime) {
    for step in steps {
        match *step {
            ProgrammingStep::Write(register, source) => { let data = value(source, runtime); let _ = runtime.serial_write(register, u16::from(data)); }
            ProgrammingStep::Delay(source) => { let ticks = value(source, runtime); runtime.delay(DelayTicks(u32::from(ticks))); }
        }
    }
}
pub fn program_level(state: &mut DeviceState, runtime: &mut impl DeviceRuntime) -> ProgrammingOutcome {
    let _ = runtime.calibration(CalibrationKey(0xdf5));
    let key = match state.calibration_mode() {
        CalibrationMode::Disabled => 0xe06,
        CalibrationMode::Phase => 0xdfb_u32.wrapping_add(state.channel_first.0),
        CalibrationMode::Level => 0xdf0_u32.wrapping_add(state.channel_first.0),
        CalibrationMode::AlternateLevel => 0xde5_u32.wrapping_add(state.channel_first.0),
        CalibrationMode::DecreaseOnly => 0xde4,
        CalibrationMode::Extended(n) => match n { ExtendedCalibration::Five => 0x1641e, ExtendedCalibration::Six => 0x1641f, ExtendedCalibration::Seven => 0x16420, ExtendedCalibration::Eight => 0x16425, ExtendedCalibration::Nine => 0x16421, ExtendedCalibration::Ten => 0x16422, ExtendedCalibration::Eleven => 0x16423, ExtendedCalibration::Twelve => 0x16424 },
        CalibrationMode::Held => return ProgrammingOutcome::Held,
    };
    let level = u16::from(runtime.calibration(CalibrationKey(key)));
    if state.calibration_mode() == CalibrationMode::DecreaseOnly && state.programmed_level.0 < level { return ProgrammingOutcome::Finished; }
    state.programmed_level = PanelLevel(level);
    runtime.level_write(u32::from(level) << 2);
    ProgrammingOutcome::Finished
}
pub fn execute(state: &mut DeviceState, step: SequenceStep, runtime: &mut impl DeviceRuntime) -> ProgrammingOutcome {
    match step {
        SequenceStep::SignalEvent2 => runtime.signal_event2(),
        SequenceStep::SignalEvent4 => runtime.signal_event4(),
        SequenceStep::Complete(target) => runtime.signal_complete(target),
        SequenceStep::GraphicDisable => runtime.graphic_enable(false),
        SequenceStep::GraphicEnable => runtime.graphic_enable(true),
        SequenceStep::DisablePanel => { runtime.read_colour(); runtime.route(); runtime.panel_control_write(0x80000000); }
        SequenceStep::EnableHdmi => { runtime.read_colour(); runtime.route(); runtime.output_enable(true); }
        SequenceStep::DisableHdmi => { runtime.read_colour(); runtime.route(); runtime.output_enable(false); }
        SequenceStep::AudioGateOn => runtime.audio_gate(true),
        SequenceStep::AudioGateOff => runtime.audio_gate(false),
        SequenceStep::EnableAudio => { runtime.read_colour(); runtime.route(); runtime.audio_output(true); }
        SequenceStep::DisableAudio => { runtime.read_colour(); runtime.route(); runtime.audio_output(false); }
        SequenceStep::ProgramLevel => { runtime.read_colour(); runtime.route(); let outcome = program_level(state, runtime); if outcome == ProgrammingOutcome::Held { return outcome; } runtime.panel_control_write(0x80008000); }
        SequenceStep::InitializePanel => { program(INITIAL_PROGRAM, runtime); program(&RESET_TAIL, runtime); runtime.delay(DelayTicks(84)); program(RESET_PROGRAM, runtime); if runtime.calibration(CalibrationKey(0xfb2)) != 0 { match runtime.timer_start(DelayTicks(500)) { Ok(token) => { state.timer = TimerToken(token); runtime.timer_store(state.timer); } Err(_) => { runtime.notify_error(); state.timer = TimerToken(0); runtime.timer_store(state.timer); } } } }
        SequenceStep::StopPanel => { if runtime.calibration(CalibrationKey(0xfb2)) != 0 && state.timer.0 != 0 { runtime.timer_cancel(state.timer.0); state.timer = TimerToken(0); runtime.timer_store(state.timer); } let data = runtime.calibration(CalibrationKey(0xeb6)); let _ = runtime.serial_write(PanelRegister(1), u16::from(data)); runtime.delay(DelayTicks(100)); }
        SequenceStep::Noop => (),
    }
    ProgrammingOutcome::Finished
}

const fn op(ticks: u32, callback: Option<SequenceStep>, linked: bool) -> PlanStep { PlanStep { identifier: ticks, callback, linked } }
use SequenceStep::*;
const STOP: [PlanStep; 2] = [op(0, Some(SignalEvent4), false), op(0, Some(StopPanel), true)];
const START: [PlanStep; 4] = [op(0, Some(SignalEvent4), false), op(0, Some(InitializePanel), true), op(0, Some(SignalEvent4), false), op(0, Some(ProgramLevel), true)];
const DISABLE: [PlanStep; 4] = [op(0, Some(SignalEvent4), false), op(0, Some(DisablePanel), true), op(0, Some(SignalEvent4), false), op(0, Some(StopPanel), true)];
const AUDIO_OFF: [PlanStep; 4] = [op(0, Some(GraphicEnable), true), op(0, Some(AudioGateOff), true), op(480, None, true), op(0, Some(DisableAudio), true)];
const AUDIO_ON: [PlanStep; 3] = [op(0, Some(EnableAudio), true), op(0, Some(AudioGateOn), true), op(100, Some(GraphicDisable), true)];
const BRIGHTNESS: [PlanStep; 2] = [op(0, Some(SignalEvent4), false), op(0, Some(ProgramLevel), true)];
const DIGITAL_ON: [PlanStep; 3] = [op(0, Some(EnableHdmi), true), op(0, Some(GraphicDisable), true), op(0, Some(Noop), true)];
const DIGITAL_OFF: [PlanStep; 4] = [op(0, Some(Noop), true), op(0, Some(GraphicEnable), true), op(11, None, true), op(0, Some(DisableHdmi), true)];
#[derive(Clone, Copy)]
enum Recipe { Empty, Start2, Start4, Stop, Disable2, Disable4, AudioOff3, AudioOff4, AudioOn1, AudioOn2, AudioOn3, Brightness, DigitalOn2, DigitalOn3, DigitalOff1, DigitalOff3, DigitalOff4, AudioPrimary, PrimaryEnable, Unbounded }
use Recipe::*;
fn recipe(current: u32, target: u32) -> (Recipe, Recipe) {
    let primary = match (current, target) {
        (0, _) | (_, 12..=13) | (12.., _) => Empty,
        (1, 2) => Start2, (1, 3) => Start4, (1, 4 | 8) => AudioOff3, (1, 5 | 9) => AudioOff4, (1, 6) => Unbounded, (1, 10) => DigitalOff3, (1, 11) => DigitalOff4,
        (2, 0 | 1) => Stop, (2, 3) => Brightness, (2, 4 | 5 | 8..=11) => Stop, (2, 6 | 7) => Unbounded,
        (3, 0 | 1 | 4 | 5 | 8..=11) => Disable4, (3, 2) => Disable2, (3, 6) => Unbounded,
        (4 | 8, 0..=3 | 6 | 7 | 10 | 11) => AudioOn2, (4 | 8, 5 | 9) => AudioPrimary,
        (5 | 9, 0..=3 | 6 | 7 | 10 | 11) => AudioOn3, (5 | 9, 4 | 8) => AudioOn1,
        (6, 0..=5 | 8..=11) => Unbounded, (6, 7) => Stop,
        (7, 2) => Unbounded, (7, 6) => Start2,
        (10, 0..=9) => DigitalOn2, (10, 11) => PrimaryEnable,
        (11, 0..=9) => DigitalOn3, (11, 10) => DigitalOff1,
        _ => Empty,
    };
    let secondary = match (current, target) {
        (2 | 3, 4 | 8) | (6 | 7, 4 | 8) | (10 | 11, 4 | 8) => AudioOff3,
        (2 | 3, 5 | 9) | (6 | 7, 5 | 9) | (10 | 11, 5 | 9) => AudioOff4,
        (2..=9, 10) => DigitalOff3, (2..=9, 11) => DigitalOff4,
        (4 | 5 | 8 | 9 | 10 | 11, 2) => Start2,
        (4 | 5 | 8 | 9 | 10 | 11, 3) => Start4,
        (4 | 5 | 8 | 9 | 10 | 11, 6) => Unbounded,
        _ => Empty,
    };
    (primary, secondary)
}
fn recipe_steps(recipe: Recipe) -> &'static [PlanStep] {
    match recipe {
        Empty | Unbounded => &[], Start2 => &START[..2], Start4 => &START, Stop => &STOP,
        Disable2 => &DISABLE[..2], Disable4 => &DISABLE, AudioOff3 => &AUDIO_OFF[..3], AudioOff4 => &AUDIO_OFF,
        AudioOn1 => &AUDIO_ON[..1], AudioOn2 => &AUDIO_ON[1..], AudioOn3 => &AUDIO_ON,
        Brightness => &BRIGHTNESS, DigitalOn2 => &DIGITAL_ON[1..], DigitalOn3 => &DIGITAL_ON,
        DigitalOff1 => &DIGITAL_ON[..1], DigitalOff3 => &DIGITAL_OFF[..3], DigitalOff4 => &DIGITAL_OFF,
        AudioPrimary => &AUDIO_OFF[3..], PrimaryEnable => &DIGITAL_OFF[3..],
    }
}
pub fn plan(current: u32, target: u32) -> SequencePlan {
    let mut result = SequencePlan::empty();
    if current >= 14 || target >= 14 { result.fault = Some(PlanFault::InvalidTransition); return result; }
    let (first, second) = recipe(current, target);
    if matches!(first, Unbounded) || matches!(second, Unbounded) { result.fault = Some(PlanFault::UnboundedStockCopy); return result; }
    for segment in [recipe_steps(first), recipe_steps(second)] {
        for step in segment { result.steps[result.count] = *step; result.count += 1; }
    }
    if result.count != 0 { result.steps[result.count] = op(0, Some(Complete(target as u8)), false); result.count += 1; }
    result
}

const INITIAL_REGISTERS: [u8; 81] = [1, 0, 2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 17, 18];
const fn initial_program() -> [ProgrammingStep; 81] {
    let mut result = [ProgrammingStep::Delay(ValueSource::Constant(1)); 81];
    let mut index = 0;
    while index < 81 {
        let register = INITIAL_REGISTERS[index];
        if index != 1 {
            let key = if index == 0 { 0xeb6 } else if index == 2 { 0xeb7 } else if index == 3 { 0xeb8 } else if index < 16 { 0xeba + index as u32 - 4 } else if index < 79 { 0xece + index as u32 - 16 } else if index == 79 { 0xec6 } else { 0xec8 };
            let source = if register == 3 { ValueSource::PanelPolarity } else { ValueSource::Calibration(CalibrationKey(key)) };
            result[index] = ProgrammingStep::Write(PanelRegister(register), source);
        }
        index += 1;
    }
    result
}
const INITIAL_PROGRAM: &[ProgrammingStep] = &initial_program();
const RESET_TAIL: [ProgrammingStep; 9] = [
    ProgrammingStep::Delay(ValueSource::Constant(1)),
    ProgrammingStep::Write(PanelRegister(19), ValueSource::Calibration(CalibrationKey(0xeca))),
    ProgrammingStep::Write(PanelRegister(20), ValueSource::Calibration(CalibrationKey(0xecb))),
    ProgrammingStep::Write(PanelRegister(21), ValueSource::Calibration(CalibrationKey(0xecc))),
    ProgrammingStep::Write(PanelRegister(22), ValueSource::Calibration(CalibrationKey(0xecd))),
    ProgrammingStep::Write(PanelRegister(17), ValueSource::Calibration(CalibrationKey(0xec7))),
    ProgrammingStep::Write(PanelRegister(18), ValueSource::Calibration(CalibrationKey(0xec9))),
    ProgrammingStep::Delay(ValueSource::Constant(1)),
    ProgrammingStep::Write(PanelRegister(1), ValueSource::Calibration(CalibrationKey(0xeb5))),
];
const fn reset_program() -> [ProgrammingStep; 89] {
    let initial = initial_program();
    let mut result = [ProgrammingStep::Delay(ValueSource::Constant(1)); 89];
    let mut index = 0;
    while index < 80 { result[index] = initial[index + 1]; index += 1; }
    index = 0;
    while index < 9 { result[index + 80] = RESET_TAIL[index]; index += 1; }
    result
}
const RESET_PROGRAM: &[ProgrammingStep] = &reset_program();
pub fn reset_pin(runtime: &mut impl DeviceRuntime) { program(RESET_PROGRAM, runtime); }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdmiFeature { Video, Audio }
pub const fn feature_word(current: u32, feature: HdmiFeature, enabled: bool) -> u32 {
    let mask = match feature { HdmiFeature::Video => 5, HdmiFeature::Audio => 2 };
    (current & !mask) | if enabled { mask } else { 0 }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RouteState { pub initialized: bool, pub updating: bool, pub pending: bool }
pub fn route_selected(state: &mut RouteState) { if state.initialized && !state.updating { state.pending = true; } }
pub const fn pack_serial(register: PanelRegister, value: u16) -> u16 { value | ((register.0 as u16) << 9) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerialStatus { pub transferred: bool, pub failed: bool }
impl SerialStatus { pub const fn stopped(self) -> bool { self.transferred || self.failed } }

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub(crate) mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    extern "C" {
        fn xm1_power_serial_open(port: u32, mode: u32, out: *mut u32) -> i32;
        fn xm1_power_serial_configure(handle: u32, delay: u32) -> i32;
        fn xm1_power_serial_write_with_residue(handle: u32, word: u32, residue: u32) -> i32;
        fn xm1_power_serial_status(handle: u32, out: *mut u32) -> i32;
        fn xm1_power_serial_stop(handle: u32) -> i32;
        fn xm1_power_serial_close(handle: u32) -> i32;
        fn xm1_power_audio_panel_control(value: u32) -> i32;
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(state: u32);
        fn xm1_power_get_task_phase(task: u32, priority: *mut u32) -> i32;
        fn xm1_power_set_task_phase(task: u32, priority: u32) -> i32;
        fn xm1_power_command_diagnostic(category: u32, code: u32) -> i32;
        fn xm1_power_software_delay_start(ticks: u32) -> i32;
        fn xm1_power_software_timer_start(period: u32, mode: u32, callback: u32, token: *mut u32) -> i32;
        fn xm1_power_software_timer_cancel(token: u32) -> i32;
    }
    pub struct NativeDeviceRuntime;
    unsafe fn selected_engine() -> usize {
        usize::from(read_volatile(0x955b20 as *const u32) == 0 && read_volatile(0x955b08 as *const u32) == 1)
    }
    unsafe fn hdmi_feature(feature: HdmiFeature, enabled: bool) {
        if read_volatile(0x93a774 as *const u32) == 7 {
            let word = read_volatile(0xfff76114 as *const u32);
            write_volatile(0xfff76114 as *mut u32, feature_word(word, feature, enabled));
        }
    }
    impl DeviceRuntime for NativeDeviceRuntime {
        fn calibration(&mut self, key: CalibrationKey) -> u8 { unsafe { crate::panel::configuration_byte(calibration_liveview::CalibrationOffset::parse(key.0 as usize).unwrap()).0 } }
        fn alternate_serial_value(&mut self) -> bool { unsafe { read_volatile(0x96bfa4 as *const u32) != 0 } }
        fn active_target(&mut self) -> ActiveTarget { unsafe { if read_volatile(0x96bd74 as *const u32) == 3 { ActiveTarget::Panel } else { ActiveTarget::Other } } }
        fn delay(&mut self, ticks: DelayTicks) { unsafe { let _ = xm1_power_software_delay_start(ticks.0); } }
        fn serial_write(&mut self, register: PanelRegister, value: u16) -> i32 {
            unsafe {
                let mut saved = 0;
                let mut priority_result = xm1_power_get_task_phase(0, &mut saved);
                if priority_result == 0 { priority_result = xm1_power_set_task_phase(0, 5); }
                let mut handle = 0;
                let result = if xm1_power_serial_open(1, 0, &mut handle) == 0 {
                    // 0x64a6cc leaves semaphore 0x16 at caller SP-20; successful 0x64c0c0 replaces it with GPIO select zero.
                    let residue = if xm1_power_serial_configure(handle, 1) == 0 { 0 } else { 0x16 };
                    let _ = xm1_power_serial_write_with_residue(handle, u32::from(pack_serial(register, value)), residue);
                    loop {
                        let mut status = [0; 7];
                        let _ = xm1_power_serial_status(handle, status.as_mut_ptr());
                        if (SerialStatus { transferred: status[0] == 1, failed: status[1] == 1 }).stopped() { break; }
                    }
                    let _ = xm1_power_serial_stop(handle);
                    xm1_power_serial_close(handle)
                } else { let _ = xm1_power_command_diagnostic(2, 0x33); -30 };
                if priority_result == 0 { let _ = xm1_power_set_task_phase(0, saved); }
                result
            }
        }
        fn level_write(&mut self, value: u32) { unsafe { write_volatile(0x96bf48 as *mut u16, (value >> 2) as u16); if read_volatile(0x9743e8 as *const u32) == 1 { write_volatile(0xfff86008 as *mut u32, value & 0x3ff); } } }
        fn panel_control_write(&mut self, value: u32) { unsafe { write_volatile(0xfff890c0 as *mut u32, value); } }
        fn read_colour(&mut self) {
            unsafe {
                let engine = selected_engine();
                let _ = read_volatile(0x916610 as *const u8);
                let base = 0x96b9bc + engine * 420;
                let _ = read_volatile((base + 0x180) as *const u8);
                let _ = read_volatile((base + 0x181) as *const u8);
                let _ = read_volatile((base + 0x182) as *const u8);
            }
        }
        fn route(&mut self) {
            unsafe {
                let base = 0x9553cc + selected_engine() * 88;
                if read_volatile((base + 0x54) as *const u8) == 0 { return; }
                let saved = xm1_power_interrupt_lock();
                let mut state = RouteState { initialized: true, updating: read_volatile((base + 0x10) as *const u32) != 0, pending: read_volatile((base + 0xc) as *const u32) != 0 };
                route_selected(&mut state);
                if !state.updating { write_volatile((base + 0xc) as *mut u32, 1); }
                xm1_power_interrupt_unlock(saved);
            }
        }
        fn output_enable(&mut self, enabled: bool) { unsafe { hdmi_feature(HdmiFeature::Video, enabled); if !enabled { hdmi_feature(HdmiFeature::Audio, false); } } }
        // The pinned 0x6d20fc body has no effects.
        fn graphic_enable(&mut self, _: bool) {}
        // The pinned 0x6e4c94 body has no effects.
        fn audio_gate(&mut self, _: bool) {}
        fn audio_output(&mut self, enabled: bool) { unsafe { let _ = xm1_power_audio_panel_control(u32::from(enabled)); } }
        fn signal_event2(&mut self) { unsafe { crate::panel_actor::xm1_scanout_panel_signal_event2(); } }
        fn signal_event4(&mut self) { unsafe { crate::panel_actor::xm1_scanout_panel_signal_event4(); } }
        fn signal_complete(&mut self, target: u8) { unsafe { crate::panel_actor::xm1_scanout_panel_signal_complete(u32::from(target)); } }
        fn timer_start(&mut self, ticks: DelayTicks) -> Result<u32, i32> { unsafe { let mut token = 0; let status = xm1_power_software_timer_start(ticks.0, 1, crate::panel_actor::xm1_scanout_panel_post_timer_command as *const () as u32, &mut token); if status == 0 { Ok(token) } else { Err(status) } } }
        fn timer_cancel(&mut self, token: u32) { unsafe { let _ = xm1_power_software_timer_cancel(token); } }
        fn timer_store(&mut self, token: TimerToken) { unsafe { write_volatile(0x96bf40 as *mut u32, token.0); } }
        fn notify_error(&mut self) { unsafe { crate::panel_actor::xm1_scanout_panel_notify_error(); } }
    }
    pub unsafe fn read_stock_device_state(state: &mut DeviceState) {
        state.timing = SourceClass::from_stock(read_volatile(0x96bd78 as *const u32));
        state.source = SourceClass::from_stock(read_volatile(0x96bd7c as *const u32));
        state.format = FormatCode(read_volatile(0x96bd88 as *const u32));
        state.first_flag = FeatureValue(read_volatile(0x96bd8c as *const u32));
        state.second_flag = FeatureValue(read_volatile(0x96bd90 as *const u32));
        state.channel_first = ChannelValue(read_volatile(0x96bd98 as *const u32));
        state.channel_second = ChannelValue(read_volatile(0x96bd9c as *const u32));
        state.transition_enabled = FeatureValue(read_volatile(0x96bf30 as *const u32));
        state.parameter_second = PhaseCode(read_volatile(0x96bf34 as *const u32));
        state.parameter_third = OverrideValue(read_volatile(0x96bf38 as *const u32));
        state.timer = TimerToken(read_volatile(0x96bf40 as *const u32));
        state.programmed_level = PanelLevel(read_volatile(0x96bf48 as *const u16));
        state.alternate_serial_value = read_volatile(0x96bfa4 as *const u32) != 0;
    }
    unsafe fn backend_state() -> DeviceState { let mut state = DeviceState::new(); read_stock_device_state(&mut state); state }
    unsafe fn finish_setter(result: SetterOutcome) {
        if result == SetterOutcome::Held { loop { core::hint::spin_loop(); } }
    }
    /// Pinned 0x6d2d80/0x6d43d0: 0x96bd74 is the panel device selector and 0x96bd90 its second flag.
    pub(crate) unsafe fn apply_mode(value: u32) -> i32 {
        if value > 1 { return -30; }
        let Some(device) = BackendDevice::parse(read_volatile(0x96bd74 as *const u32)) else { return -30; };
        let mut state = backend_state();
        if matches!(device.0, 2 | 3) && state.second_flag != FeatureValue(value) { write_volatile(0x96bd90 as *mut u32, value); }
        let outcome = set_second_flag(&mut state, device, FeatureValue(value), &mut NativeDeviceRuntime);
        finish_setter(outcome);
        write_volatile(0x96bd90 as *mut u32, value);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_transfer(register: i16, value: u16) -> i32 { NativeDeviceRuntime.serial_write(PanelRegister(register as u8), value) }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_initialize_program() {
        program(INITIAL_PROGRAM, &mut NativeDeviceRuntime);
        program(&RESET_TAIL, &mut NativeDeviceRuntime);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_reset_program() { reset_pin(&mut NativeDeviceRuntime); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_source(device: u32, value: u32) -> i32 {
        if BackendDevice::parse(device).is_none() { return -30; }
        write_volatile(0x96bd7c as *mut u32, value);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_channel(device: u32, channel: u32, value: u32) -> i32 {
        let Some(device) = BackendDevice::parse(device) else { return -30; };
        let channel = match channel { 0 => Channel::Primary, 1 => Channel::Secondary, _ => return -30 };
        let mut state = backend_state();
        if channel == Channel::Primary && matches!(device.0, 2 | 3) && state.channel_first != ChannelValue(value) { write_volatile(0x96bd98 as *mut u32, value); }
        let outcome = set_channel(&mut state, device, channel, ChannelValue(value), &mut NativeDeviceRuntime);
        finish_setter(outcome);
        write_volatile((if channel == Channel::Primary { 0x96bd98 } else { 0x96bd9c }) as *mut u32, value);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_timing(device: u32, timing: u32) -> i32 {
        let Some(device) = BackendDevice::parse(device) else { return -30; };
        let mut state = backend_state();
        if matches!(device.0, 2 | 3) && timing_class(SourceClass(timing)) != 0 && timing_class(SourceClass(timing)) != timing_class(state.timing) { write_volatile(0x96bd78 as *mut u32, timing); }
        let outcome = set_timing(&mut state, device, SourceClass(timing), &mut NativeDeviceRuntime);
        finish_setter(outcome);
        write_volatile(0x96bd78 as *mut u32, timing);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_parameters(device: u32, format: u32, second: u32, third: u32) -> i32 {
        let Some(device) = BackendDevice::parse(device) else { return -30; };
        let mut state = backend_state();
        if matches!(device.0, 2 | 3) && (state.format != FormatCode(format) || state.parameter_second != PhaseCode(second) || state.parameter_third != OverrideValue(third)) {
            write_volatile(0x96bd88 as *mut u32, format);
            write_volatile(0x96bf34 as *mut u32, second);
            write_volatile(0x96bf38 as *mut u32, third);
        }
        let outcome = set_parameters(&mut state, device, HardwareParameters { format: FormatCode(format), second: PhaseCode(second), third: OverrideValue(third) }, &mut NativeDeviceRuntime);
        finish_setter(outcome);
        write_volatile(0x96bd88 as *mut u32, format);
        write_volatile(0x96bf34 as *mut u32, second);
        write_volatile(0x96bf38 as *mut u32, third);
        0
    }
    // 0x6d42b8 consumes r0-r3 and never loads its three incoming stack arguments.
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_configuration(device: u32, format: u32, second: u32, third: u32, _: u32, _: u32, _: u32) -> i32 { xm1_scanout_panel_device_set_parameters(device, format, second, third) }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_first_flag(device: u32, value: u32) -> i32 {
        if BackendDevice::parse(device).is_none() { return -30; }
        write_volatile(0x96bd8c as *mut u32, value);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_second_flag(device: u32, value: u32) -> i32 {
        let Some(device) = BackendDevice::parse(device) else { return -30; };
        let mut state = backend_state();
        if matches!(device.0, 2 | 3) && state.second_flag != FeatureValue(value) { write_volatile(0x96bd90 as *mut u32, value); }
        let outcome = set_second_flag(&mut state, device, FeatureValue(value), &mut NativeDeviceRuntime);
        finish_setter(outcome);
        write_volatile(0x96bd90 as *mut u32, value);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_panel_device_set_transition(device: u32, value: u32) -> i32 {
        if BackendDevice::parse(device).is_none() { return -30; }
        write_volatile(0x96bf30 as *mut u32, value);
        0
    }
    fn callback_step(address: u32) -> Option<SequenceStep> {
        if (0x6d368c..=0x6d3790).contains(&address) && (0x6d3790 - address) % 20 == 0 { return Some(Complete(((0x6d3790 - address) / 20) as u8)); }
        Some(match address {
            0x6d30d0 | 0x6d38cc => SignalEvent2, 0x6d30ac | 0x6d38e8 => SignalEvent4,
            0x6d3904 | 0x6d392c => GraphicDisable, 0x6d3918 | 0x6d3940 => GraphicEnable,
            0x6d3954 => DisablePanel, 0x6d399c => EnableHdmi,
            0x6d39dc => DisableHdmi, 0x6d3a18 => AudioGateOn, 0x6d3a2c => AudioGateOff,
            0x6d3a40 => EnableAudio, 0x6d3a84 => DisableAudio,
            0x6d467c => InitializePanel, 0x6d4700 => StopPanel, 0x6d3c50 => ProgramLevel,
            0x6d37a4 | 0x6d37b8 | 0x6d3d10 => Noop, _ => return None,
        })
    }
    pub unsafe fn native_read_callback(index: u32) -> Result<Option<SequenceStep>, i32> {
        let address = 0x96bda4_u32.wrapping_add(index.wrapping_mul(12)).wrapping_add(4);
        let callback = read_volatile(address as *const u32);
        if callback == 0 { Ok(None) } else { callback_step(callback).map(Some).ok_or(-255) }
    }
    pub fn native_submit_existing(start: usize) -> u32 { 0x96bda4_u32.wrapping_add((start as u32).wrapping_mul(12)) }
    unsafe fn copy_guest_bytes(mut destination: u32, mut source: u32, mut count: u32) {
        if count > 1024 && (source ^ destination) & 3 == 0 {
            let prefix = 4 - (destination & 3);
            for _ in 0..prefix {
                let byte = read_volatile(source as *const u8);
                write_volatile(destination as *mut u8, byte);
                source = source.wrapping_add(1);
                destination = destination.wrapping_add(1);
            }
            count = count.wrapping_sub(prefix);
            let words = count >> 2;
            for _ in 0..words {
                let word = read_volatile(source as *const u32);
                write_volatile(destination as *mut u32, word);
                source = source.wrapping_add(4);
                destination = destination.wrapping_add(4);
            }
            count &= 3;
        } else if (source | destination) & 1 == 0 {
            let halves = count >> 1;
            for _ in 0..halves {
                let half = read_volatile(source as *const u16);
                write_volatile(destination as *mut u16, half);
                source = source.wrapping_add(2);
                destination = destination.wrapping_add(2);
            }
            count &= 1;
        }
        for _ in 0..count {
            let byte = read_volatile(source as *const u8);
            write_volatile(destination as *mut u8, byte);
            source = source.wrapping_add(1);
            destination = destination.wrapping_add(1);
        }
    }
    pub unsafe fn native_build_plan(current: u32, target: u32) -> Result<i32, i32> {
        if current >= 14 || target >= 14 { return Err(-30); }
        let entry = 0x7cbea0_u32.wrapping_add(current.wrapping_mul(14).wrapping_add(target).wrapping_mul(16));
        let terminal = read_volatile(0x7cf25c_u32.wrapping_add(target.wrapping_mul(12)) as *const u32);
        let mut count = read_volatile(entry.wrapping_add(4) as *const u32);
        if count != 0 {
            let source = read_volatile(entry as *const u32);
            copy_guest_bytes(0x96bda4, source, count.wrapping_mul(12));
        }
        let second_count = read_volatile(entry.wrapping_add(12) as *const u32);
        if second_count != 0 {
            let source = read_volatile(entry.wrapping_add(8) as *const u32);
            copy_guest_bytes(0x96bda4_u32.wrapping_add(count.wrapping_mul(12)), source, second_count.wrapping_mul(12));
            count = count.wrapping_add(second_count);
        }
        if count != 0 {
            let mut node = 0x96bda4_u32;
            for _ in 0..count {
                let boundary = read_volatile(node.wrapping_add(8) as *const u32) == 1;
                write_volatile(node.wrapping_add(8) as *mut u32, if boundary { 0 } else { node.wrapping_add(12) });
                node = node.wrapping_add(12);
            }
            write_volatile(node as *mut u32, 0);
            write_volatile(node.wrapping_add(4) as *mut u32, terminal);
            write_volatile(node.wrapping_add(8) as *mut u32, 0);
            write_volatile(0x95dc00 as *mut u32, 0x6d30ac);
            write_volatile(0x95dc04 as *mut u32, 0x6d30d0);
            count = count.wrapping_add(1);
        }
        write_volatile(0x96bf24 as *mut u32, count);
        Ok(0)
    }
    pub unsafe fn native_execute(state: &mut DeviceState, step: SequenceStep) -> ProgrammingOutcome {
        match execute(state, step, &mut NativeDeviceRuntime) {
            ProgrammingOutcome::Finished => ProgrammingOutcome::Finished,
            ProgrammingOutcome::Held => loop { core::hint::spin_loop(); },
        }
    }
    pub unsafe fn native_reset_pin(_: &mut DeviceState) { reset_pin(&mut NativeDeviceRuntime); }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_lcd_operation_action(address: u32) -> i32 {
        if address == 0x6d4960 || address == 0x6d4978 { hdmi_feature(HdmiFeature::Audio, address == 0x6d4960); return 0; }
        let Some(step) = callback_step(address) else { return -255; };
        match step {
            SignalEvent2 => { crate::panel_actor::xm1_scanout_panel_signal_event2(); return 0; }
            SignalEvent4 => { crate::panel_actor::xm1_scanout_panel_signal_event4(); return 0; }
            Complete(target) => { crate::panel_actor::xm1_scanout_panel_signal_complete(u32::from(target)); return 0; }
            _ => (),
        }
        let outcome = {
            let mut state = DeviceState::new();
            read_stock_device_state(&mut state);
            native_execute(&mut state, step)
        };
        match outcome { ProgrammingOutcome::Finished => 0, ProgrammingOutcome::Held => loop { core::hint::spin_loop(); } }
    }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{native_execute, native_reset_pin, read_stock_device_state, NativeDeviceRuntime, xm1_power_lcd_operation_action};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackendDevice(u8);
impl BackendDevice { pub fn parse(value: u32) -> Option<Self> { if value < 12 { Some(Self(value as u8)) } else { None } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel { Primary, Secondary }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelValue(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardwareParameters { pub format: FormatCode, pub second: PhaseCode, pub third: OverrideValue }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetterOutcome { Stored, Programmed, Held }
fn update_level(state: &mut DeviceState, runtime: &mut impl DeviceRuntime) -> SetterOutcome {
    match program_level(state, runtime) { ProgrammingOutcome::Finished => SetterOutcome::Programmed, ProgrammingOutcome::Held => SetterOutcome::Held }
}
pub fn set_channel(state: &mut DeviceState, device: BackendDevice, channel: Channel, value: ChannelValue, runtime: &mut impl DeviceRuntime) -> SetterOutcome {
    if channel == Channel::Secondary { state.channel_second = value; return SetterOutcome::Stored; }
    let changed = state.channel_first != value;
    state.channel_first = value;
    if matches!(device.0, 2 | 3) && changed && value.0 < 11 && runtime.active_target() == ActiveTarget::Panel { update_level(state, runtime) } else { SetterOutcome::Stored }
}
fn timing_class(value: SourceClass) -> u8 {
    match value.code() { 0 | 1 | 3 | 5 | 6 | 7 | 8 | 10 => 1, 2 | 4 | 9 => 2, _ => 0 }
}
pub fn set_timing(state: &mut DeviceState, device: BackendDevice, timing: SourceClass, runtime: &mut impl DeviceRuntime) -> SetterOutcome {
    let changed_class = timing_class(timing) != 0 && timing_class(timing) != timing_class(state.timing);
    state.timing = timing;
    if matches!(device.0, 2 | 3) && changed_class && runtime.active_target() == ActiveTarget::Panel { update_level(state, runtime) } else { SetterOutcome::Stored }
}
pub fn set_parameters(state: &mut DeviceState, device: BackendDevice, parameters: HardwareParameters, runtime: &mut impl DeviceRuntime) -> SetterOutcome {
    let changed = state.format != parameters.format || state.parameter_second != parameters.second || state.parameter_third != parameters.third;
    state.format = parameters.format;
    state.parameter_second = parameters.second;
    state.parameter_third = parameters.third;
    if device.0 == 3 && changed { update_level(state, runtime) } else { SetterOutcome::Stored }
}
pub fn set_second_flag(state: &mut DeviceState, device: BackendDevice, flag: FeatureValue, runtime: &mut impl DeviceRuntime) -> SetterOutcome {
    let changed = state.second_flag != flag;
    state.second_flag = flag;
    if device.0 == 3 && changed { update_level(state, runtime) } else { SetterOutcome::Stored }
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{xm1_scanout_panel_device_configuration, xm1_scanout_panel_device_set_source, xm1_scanout_panel_device_set_channel, xm1_scanout_panel_device_set_timing, xm1_scanout_panel_device_set_parameters, xm1_scanout_panel_device_set_first_flag, xm1_scanout_panel_device_set_second_flag, xm1_scanout_panel_device_set_transition};

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{xm1_scanout_panel_device_transfer, xm1_scanout_panel_device_initialize_program, xm1_scanout_panel_device_reset_program};

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{native_read_callback, native_submit_existing};

#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::native_build_plan;
