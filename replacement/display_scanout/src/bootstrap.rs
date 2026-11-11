use crate::{Engine, timing::{CalibrationRead, TimingMode, TimingRoute}};
use calibration_liveview::CalibrationOffset;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockRegister { Control, Selection, Input, FirstPulse, FirstInterval, SecondPulse, SecondInterval }
impl ClockRegister {
    const ALL: [Self; 7] = [Self::Control, Self::Selection, Self::Input, Self::FirstPulse, Self::FirstInterval, Self::SecondPulse, Self::SecondInterval];
    pub const fn index(self) -> usize { match self { Self::Control => 0, Self::Selection => 1, Self::Input => 2, Self::FirstPulse => 3, Self::FirstInterval => 4, Self::SecondPulse => 5, Self::SecondInterval => 6 } }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClockWord(pub u32);
pub trait ClockState {
    fn select_mode(&mut self, mode: TimingMode);
    fn set_pending(&mut self, pending: bool);
    fn pending(&self) -> bool;
    fn clear_activity(&mut self);
    fn word(&self, register: ClockRegister) -> ClockWord;
    fn put_word(&mut self, register: ClockRegister, value: ClockWord);
    fn put_byte(&mut self, register: ClockRegister, byte: usize, value: u8);
    fn put_half(&mut self, register: ClockRegister, upper: bool, value: u16);
}
pub trait ClockHardware {
    fn control(&mut self) -> ClockWord;
    fn write(&mut self, register: ClockRegister, value: ClockWord);
}
fn byte(calibration: &mut impl CalibrationRead, offset: usize) -> u32 {
    u32::from(calibration.read_byte(CalibrationOffset::parse(offset).unwrap()).0)
}
const SELECTION: [[usize; 3]; 10] = [
    [4236,4249,4248],[4236,4251,4250],[4483,4249,4248],[4383,4249,4248],[4433,4249,4248],
    [4383,4249,4248],[4433,4251,4250],[4383,4251,4250],[4333,4251,4250],[4283,4251,4250],
];
const CONTROL: [usize; 10] = [4379,4379,4479,4379,4429,4379,4429,4379,4329,4279];
const FIRST: [usize; 10] = [4238,4241,4484,4384,4434,4384,4434,4384,4334,4284];
const SECOND: [usize; 10] = [0,0,4494,4394,4444,4394,4444,4394,4344,4294];

// 0x6c77a0..0x6c7b1c.
pub fn stage_clock(mode: TimingMode, state: &mut impl ClockState, calibration: &mut impl CalibrationRead, hardware: &mut impl ClockHardware) {
    let index = mode.raw() as usize;
    let extended = index >= 2;
    state.set_pending(false);
    state.select_mode(mode);
    state.put_word(ClockRegister::Control, hardware.control());
    if extended {
        let key = CONTROL[index];
        let high = byte(calibration, key);
        let middle = byte(calibration, key + 1);
        let low = byte(calibration, key + 2);
        let last = byte(calibration, key + 3);
        state.put_word(ClockRegister::Control, ClockWord((high << 24) | (middle << 16) | (low << 8) | last));
    } else {
        let value = byte(calibration, 0x108b);
        let old = state.word(ClockRegister::Control).0 as u8;
        state.put_byte(ClockRegister::Control, 0, (old & !2) | ((value as u8 & 1) << 1));
        let value = byte(calibration, 0x108a);
        let old = (state.word(ClockRegister::Control).0 >> 16) as u8;
        state.put_byte(ClockRegister::Control, 2, (old & !1) | (value as u8 & 1));
    }
    state.put_word(ClockRegister::Selection, ClockWord(0));
    let selection = byte(calibration, SELECTION[index][0]);
    state.put_byte(ClockRegister::Selection, 2, ((selection & 7) << 2) as u8);
    if !extended {
        for (upper, key) in [(false, SELECTION[index][1]), (true, SELECTION[index][2])] {
            let value = byte(calibration, key);
            state.put_half(ClockRegister::Input, upper, ((value << 2) & 0x3ff) as u16 | (half(state, ClockRegister::Input, upper) & !0x3ff));
        }
    }
    for (second, pulse, interval, key) in [
        (false, ClockRegister::FirstPulse, ClockRegister::FirstInterval, FIRST[index]),
        (true, ClockRegister::SecondPulse, ClockRegister::SecondInterval, SECOND[index]),
    ] {
        state.put_word(pulse, ClockWord(0));
        state.put_word(interval, ClockWord(0));
        if extended {
            for (register, upper, pair_key) in if second { [(interval,false,key),(pulse,false,key-2),(pulse,true,key-4)] } else { [(pulse,false,key),(interval,true,key+2),(interval,false,key+4)] } {
                let high = byte(calibration, pair_key);
                let low = byte(calibration, pair_key + 1);
                let value = ((high << 8) | low) as u16;
                let old = half(state, register, upper);
                state.put_half(register, upper, (old & !0x3ff) | (value & 0x3ff));
            }
        } else {
            if second {
                state.put_half(interval, false, half(state, interval, false) & !0x3ff);
                for (upper, actual_key) in [(false,[4245,4247][index]),(true,[4244,4246][index])] {
                    let value = byte(calibration, actual_key) << 2;
                    state.put_half(pulse, upper, (half(state, pulse, upper) & !0x3ff) | (value as u16 & 0x3ff));
                }
            } else {
                for (register, upper, actual_key) in [(pulse,false,key),(interval,true,key+1),(interval,false,key+2)] {
                    let value = byte(calibration, actual_key) << 2;
                    state.put_half(register, upper, (half(state, register, upper) & !0x3ff) | (value as u16 & 0x3ff));
                }
            }
        }
    }
    state.set_pending(true);
}
fn half(state: &impl ClockState, register: ClockRegister, upper: bool) -> u16 { (state.word(register).0 >> if upper { 16 } else { 0 }) as u16 }
// 0x6c7728..0x6c77a0.
pub fn flush_clock(state: &mut impl ClockState, hardware: &mut impl ClockHardware) {
    state.clear_activity();
    if !state.pending() { return; }
    state.set_pending(false);
    for register in ClockRegister::ALL { hardware.write(register, state.word(register)); }
}
pub fn bootstrap(mode: TimingMode, state: &mut impl ClockState, calibration: &mut impl CalibrationRead, hardware: &mut impl ClockHardware) {
    stage_clock(mode, state, calibration, hardware);
    flush_clock(state, hardware);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimension(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtentField { Width, Height, HorizontalScale, VerticalScale }
pub trait EngineInitialization {
    fn interlaced(&mut self, value: bool);
    fn extent(&mut self, field: ExtentField) -> u16;
    fn set_extent(&mut self, field: ExtentField, value: u16);
    fn output_width(&mut self, value: Dimension);
    fn output_height(&mut self, value: Dimension);
    fn clear_origins(&mut self);
    fn copy_extents(&mut self);
}
fn pair(calibration: &mut impl CalibrationRead, high: usize, low: usize) -> u16 {
    let high = byte(calibration, high);
    (byte(calibration, low) | (high << 8)) as u16
}
// 0x6cbb88..0x6cbf24.
pub fn engine_initialize(route: TimingRoute, mode: TimingMode, state: &mut impl EngineInitialization, calibration: &mut impl CalibrationRead) {
    let mode = mode.raw();
    let fixed = if route == TimingRoute::Direct {
        state.interlaced(matches!(mode, 2 | 3 | 4 | 7 | 8 | 9));
        match mode {
            5..=9 => Some((1919,1079,4096,4096,1920,1080)),
            3 | 4 => Some((1279,719,4096,4096,1280,720)),
            2 => Some((719,479,0xe33,4096,640,480)),
            _ => None,
        }
    } else { None };
    if let Some((width,height,horizontal,vertical,output_width,output_height)) = fixed {
        set_dimension(state, ExtentField::Width, width);
        set_dimension(state, ExtentField::Height, height);
        state.set_extent(ExtentField::HorizontalScale,horizontal);
        state.set_extent(ExtentField::VerticalScale,vertical);
        state.output_width(Dimension(output_width));
        state.output_height(Dimension(output_height));
    } else {
        let (width_keys,height_keys) = match route {
            TimingRoute::Direct => ((0x1041,0x1040),if mode == 1 { (0x1048,0x1047) } else { (0x1046,0x1045) }),
            TimingRoute::Two => ((0xe32,0xe33),(0xe34,0xe35)),
            _ => ((0xe2e,0xe2f),(0xe30,0xe31)),
        };
        let width = pair(calibration,width_keys.0,width_keys.1);
        set_dimension(state,ExtentField::Width,width);
        let height = pair(calibration,height_keys.0,height_keys.1);
        set_dimension(state,ExtentField::Height,height);
        if route != TimingRoute::Direct {
            state.interlaced(byte(calibration,if route == TimingRoute::Two { 0xe2d } else { 0xe2c }) & 1 != 0);
        }
        let horizontal = if route == TimingRoute::Direct { pair(calibration,0x1043,0x1042) } else { 4096 };
        state.set_extent(ExtentField::HorizontalScale,horizontal);
        let vertical = if route == TimingRoute::Direct {
            if mode == 1 { pair(calibration,0x104c,0x104b) } else { pair(calibration,0x104a,0x1049) }
        } else { 4096 };
        state.set_extent(ExtentField::VerticalScale,vertical);
        if matches!(route,TimingRoute::Zero | TimingRoute::One) {
            state.output_width(Dimension(720));
            state.output_height(Dimension(480));
        } else {
            let width = u32::from(state.extent(ExtentField::Width) & 0xfff)+1;
            state.output_width(Dimension(width));
            let height = u32::from(state.extent(ExtentField::Height) & 0xfff)+1;
            state.output_height(Dimension(height));
        }
    }
    state.clear_origins();
    state.copy_extents();
}
fn set_dimension(state: &mut impl EngineInitialization, field: ExtentField, value: u16) {
    let old = state.extent(field);
    state.set_extent(field,(old & !0xfff) | (value & 0xfff));
}

pub fn decode_timing_configuration(words: [u32; 3]) -> Option<crate::timing::TimingConfiguration> {
    Some(crate::timing::TimingConfiguration {
        mode: TimingMode::parse(words[0])?,
        route: TimingRoute::parse(words[1])?,
        rate: crate::timing::TimingRate::parse(words[2])?,
    })
}
pub fn timing_configuration(engine: Engine, states: &mut [crate::timing::TimingState; 2], configuration: &mut crate::timing::TimingConfiguration, calibration: &mut impl CalibrationRead, hardware: &mut impl crate::Hardware) {
    states[engine.index()] = crate::timing::TimingState::zero();
    crate::timing::stage(engine, states, configuration, calibration);
    states[engine.index()].force = crate::timing::ForceUpdate(1);
    crate::timing::flush(engine, &mut states[engine.index()], hardware);
}
/// Pinned 0x6c86e8.
pub fn live_timing(engine: Engine, states: &mut [crate::timing::TimingState; 2], configuration: &mut crate::timing::TimingConfiguration, calibration: &mut impl CalibrationRead, hardware: &mut impl crate::Hardware) -> u32 {
    crate::timing::stage(engine, states, configuration, calibration);
    crate::timing::flush(engine, &mut states[engine.index()], hardware)
}
pub fn formatter_slot(engine: Engine, state: &mut crate::panel_formatter::PanelFormatterState, configuration: crate::panel_formatter::FormatterConfiguration, calibration: &mut impl CalibrationRead, hardware: &mut impl crate::Hardware) -> Result<(), crate::Error> {
    crate::panel_formatter::stage_configuration(engine, state, configuration, calibration);
    crate::timing::formatter_flush(engine, &mut state.formatter, hardware);
    let category = crate::panel_formatter::source_category(state);
    let slot = state.active_slot;
    let mode = state.slot_modes[slot.index()];
    crate::panel_formatter::set_slot_mode(engine, state, slot, mode, category, calibration, hardware)
}

#[cfg(all(target_arch = "arm", feature = "panel"))]
mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    unsafe fn word(address: usize) -> u32 { read_volatile(address as *const u32) }
    unsafe fn put(address: usize, value: u32) { write_volatile(address as *mut u32,value); }
    struct Calibration;
    impl CalibrationRead for Calibration {
        fn read_byte(&mut self, offset: CalibrationOffset) -> calibration_liveview::CalibrationByte { unsafe { crate::panel::configuration_byte(offset) } }
    }
    struct Clock;
    impl ClockState for Clock {
        fn select_mode(&mut self, mode: TimingMode) { unsafe { put(0x974310,mode.raw()); } }
        fn set_pending(&mut self, pending: bool) { unsafe { put(0x974314,u32::from(pending)); } }
        fn pending(&self) -> bool { unsafe { word(0x974314) != 0 } }
        fn clear_activity(&mut self) { unsafe { put(0x974334,0); } }
        fn word(&self, register: ClockRegister) -> ClockWord { unsafe { ClockWord(word(0x974318 + register.index()*4)) } }
        fn put_word(&mut self, register: ClockRegister, value: ClockWord) { unsafe { put(0x974318 + register.index()*4,value.0); } }
        fn put_byte(&mut self, register: ClockRegister, byte: usize, value: u8) { unsafe { write_volatile((0x974318 + register.index()*4 + byte) as *mut u8,value); } }
        fn put_half(&mut self, register: ClockRegister, upper: bool, value: u16) { unsafe { write_volatile((0x974318 + register.index()*4 + usize::from(upper)*2) as *mut u16,value); } }
    }
    struct ClockDevice;
    impl ClockHardware for ClockDevice {
        fn control(&mut self) -> ClockWord { unsafe { ClockWord(word(0xfff68000)) } }
        fn write(&mut self, register: ClockRegister, value: ClockWord) { unsafe { put(0xfff68000 + register.index()*4,value.0); } }
    }
    struct Initialization(Engine);
    impl Initialization {
        fn base(&self) -> usize { 0x96b9bc + self.0.index()*420 }
        fn pipeline(&self) -> usize { 0x9554b8 + self.0.index()*572 }
        fn field(&self, field: ExtentField) -> usize { self.base() + match field { ExtentField::Width=>18, ExtentField::Height=>16, ExtentField::HorizontalScale=>0x186, ExtentField::VerticalScale=>0x184 } }
    }
    impl EngineInitialization for Initialization {
        fn interlaced(&mut self, value: bool) { unsafe { let at=(self.base()+1) as *mut u8; write_volatile(at,(read_volatile(at)&!1)|u8::from(value)); } }
        fn extent(&mut self, field: ExtentField) -> u16 { unsafe { read_volatile(self.field(field) as *const u16) } }
        fn set_extent(&mut self, field: ExtentField, value: u16) { unsafe { write_volatile(self.field(field) as *mut u16,value); } }
        fn output_width(&mut self, value: Dimension) { unsafe { put(self.pipeline()+0x234,value.0); } }
        fn output_height(&mut self, value: Dimension) { unsafe { put(self.pipeline()+0x238,value.0); } }
        fn clear_origins(&mut self) { unsafe { put(self.pipeline()+0x34,0); put(self.pipeline()+0x38,0); } }
        fn copy_extents(&mut self) { unsafe {
            for (source,destination) in [(16,20),(18,22)] {
                let source=read_volatile((self.base()+source) as *const u16);
                let destination=(self.base()+destination) as *mut u16;
                write_volatile(destination,(read_volatile(destination)&!0xfff)|(source&0xfff));
            }
        } }
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_bootstrap(configuration: *const u32) -> i32 {
        let Some(mode)=TimingMode::parse(read_volatile(configuration)) else { return -30; };
        bootstrap(mode,&mut Clock,&mut Calibration,&mut ClockDevice);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_timing_configuration(engine: u32, configuration: *mut u32) -> i32 {
        let Some(selected)=Engine::parse(engine) else { return -30; };
        if decode_timing_configuration([read_volatile(configuration),read_volatile(configuration.add(1)),read_volatile(configuration.add(2))]).is_none() { return -30; }
        let address=0x9429f4 + selected.index()*88;
        for offset in (0..88).step_by(4) { put(address+offset,0); }
        let status=crate::abi::xm1_scanout_timing_stage(engine,configuration);
        if status != 0 { return status; }
        put(address+0x14,1);
        let _=crate::abi::xm1_scanout_timing_flush(engine);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_timing_live(engine: u32, configuration: *mut u32, flushed: *mut u32) -> i32 {
        let status=crate::abi::xm1_scanout_timing_stage(engine,configuration);
        if status != 0 { return status; }
        crate::layout::put_word(flushed as usize,crate::abi::xm1_scanout_timing_flush(engine) as u32);
        0
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_engine_initialize(engine: u32, route: u32, mode: u32) {
        let (Some(engine),Some(route),Some(mode))=(Engine::parse(engine),TimingRoute::parse(route),TimingMode::parse(mode)) else { return; };
        engine_initialize(route,mode,&mut Initialization(engine),&mut Calibration);
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_scanout_formatter_slot(engine: u32, configuration: *const u32) -> i32 {
        let Some(engine)=Engine::parse(engine) else { return -30; };
        crate::panel_formatter::abi::initialize(engine,configuration)
    }
}
#[cfg(all(target_arch = "arm", feature = "panel"))]
pub use native::{xm1_scanout_bootstrap,xm1_scanout_timing_configuration,xm1_scanout_timing_live,xm1_scanout_engine_initialize,xm1_scanout_formatter_slot};
