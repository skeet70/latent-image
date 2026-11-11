use crate::{bootstrap::*, timing::{CalibrationRead, TimingMode, TimingRoute}, Engine, Bank, Hardware, Register};
use calibration_liveview::{CalibrationByte, CalibrationOffset};
use std::vec::Vec;

struct Calibration(Vec<(usize,u8)>);
impl CalibrationRead for Calibration {
    fn read_byte(&mut self, offset: CalibrationOffset) -> CalibrationByte {
        CalibrationByte(self.0.iter().find(|(key,_)| *key == offset.byte_offset()).map_or(0,|(_,value)| *value))
    }
}
struct ClockMemory { registers: [ClockWord;7], pending: bool, active: bool, mode: u32 }
impl ClockMemory { fn new() -> Self { Self { registers:[ClockWord(0xa5a55a5a);7],pending:true,active:true,mode:9 } } }
impl ClockState for ClockMemory {
    fn select_mode(&mut self,mode:TimingMode) { self.mode=mode.raw(); }
    fn set_pending(&mut self,value:bool) { self.pending=value; }
    fn pending(&self)->bool { self.pending }
    fn clear_activity(&mut self) { self.active=false; }
    fn word(&self,register:ClockRegister)->ClockWord { self.registers[register.index()] }
    fn put_word(&mut self,register:ClockRegister,value:ClockWord) { self.registers[register.index()]=value; }
    fn put_byte(&mut self,register:ClockRegister,byte:usize,value:u8) { let word=&mut self.registers[register.index()].0; *word=(*word & !(0xff << (byte*8))) | (u32::from(value) << (byte*8)); }
    fn put_half(&mut self,register:ClockRegister,upper:bool,value:u16) { let shift=if upper {16}else{0}; let word=&mut self.registers[register.index()].0; *word=(*word & !(0xffff << shift)) | (u32::from(value)<<shift); }
}
struct ClockDevice { writes:Vec<(ClockRegister,ClockWord)> }
impl ClockHardware for ClockDevice {
    fn control(&mut self)->ClockWord { ClockWord(0x89abcdef) }
    fn write(&mut self,register:ClockRegister,value:ClockWord) { self.writes.push((register,value)); }
}
#[test]
fn boot_default_preserves_input_reserved_bits_and_publishes_all_seven_registers() {
    let mut calibration=Calibration(Vec::from([(0x108b,0),(0x108a,0),(4236,5),(4249,7),(4248,9),(4238,11),(4239,13),(4240,17),(4245,19),(4244,23)]));
    let mut state=ClockMemory::new();
    let mut hardware=ClockDevice { writes:Vec::new() };
    bootstrap(TimingMode::parse(0).unwrap(),&mut state,&mut calibration,&mut hardware);
    assert_eq!(hardware.writes,Vec::from([
        (ClockRegister::Control,ClockWord(0x89aacded)),
        (ClockRegister::Selection,ClockWord(0x00140000)),
        (ClockRegister::Input,ClockWord(0xa424581c)),
        (ClockRegister::FirstPulse,ClockWord(44)),
        (ClockRegister::FirstInterval,ClockWord(0x00340044)),
        (ClockRegister::SecondPulse,ClockWord(0x005c004c)),
        (ClockRegister::SecondInterval,ClockWord(0)),
    ]));
    assert!(!state.pending);
    assert!(!state.active);
    assert_eq!(state.mode,0);
}
#[test]
fn extended_boot_uses_distinct_mode_keys_and_ten_bit_pair_truncation() {
    let mut calibration=Calibration(Vec::from([(4479,0x12),(4480,0x34),(4481,0x56),(4482,0x78),(4483,7),
        (4484,5),(4485,0x23),(4486,6),(4487,0x45),(4488,7),(4489,0x67),
        (4494,8),(4495,0x89),(4492,9),(4493,0xab),(4490,10),(4491,0xcd)]));
    let mut state=ClockMemory::new(); let mut hardware=ClockDevice { writes:Vec::new() };
    bootstrap(TimingMode::parse(2).unwrap(),&mut state,&mut calibration,&mut hardware);
    assert_eq!(hardware.writes.iter().map(|(_,word)|word.0).collect::<Vec<_>>(),Vec::from([0x12345678,0x001c0000,0xa5a55a5a,0x123,0x02450367,0x02cd01ab,0x89]));
}
#[test]
fn unchanged_clock_flush_clears_activity_without_mmio() {
    let mut state=ClockMemory::new(); state.pending=false;
    let mut hardware=ClockDevice { writes:Vec::new() };
    flush_clock(&mut state,&mut hardware);
    assert!(!state.active); assert!(hardware.writes.is_empty());
}
struct Initialization { extents:[u16;4], interlaced:bool, output:(u32,u32), copied:(u16,u16), origins:(u32,u32) }
impl Initialization { fn new()->Self { Self {extents:[0xa111,0xb222,0xc333,0xd444],interlaced:true,output:(11,13),copied:(0xefff,0xffff),origins:(17,19)} } }
fn index(field:ExtentField)->usize { match field {ExtentField::Width=>0,ExtentField::Height=>1,ExtentField::HorizontalScale=>2,ExtentField::VerticalScale=>3} }
impl EngineInitialization for Initialization {
    fn interlaced(&mut self,value:bool) { self.interlaced=value; }
    fn extent(&mut self,field:ExtentField)->u16 { self.extents[index(field)] }
    fn set_extent(&mut self,field:ExtentField,value:u16) { self.extents[index(field)]=value; }
    fn output_width(&mut self,value:Dimension) { self.output.0=value.0; }
    fn output_height(&mut self,value:Dimension) { self.output.1=value.0; }
    fn clear_origins(&mut self) { self.origins=(0,0); }
    fn copy_extents(&mut self) { self.copied=((self.copied.0&0xf000)|(self.extents[1]&0xfff),(self.copied.1&0xf000)|(self.extents[0]&0xfff)); }
}
#[test]
fn direct_dimensions_and_interlace_follow_source_mode_switch() {
    for (mode,width,height,horizontal,output,interlaced) in [(2,719,479,0xe33,(640,480),true),(3,1279,719,4096,(1280,720),true),(5,1919,1079,4096,(1920,1080),false),(8,1919,1079,4096,(1920,1080),true)] {
        let mut state=Initialization::new();
        engine_initialize(TimingRoute::Direct,TimingMode::parse(mode).unwrap(),&mut state,&mut Calibration(Vec::new()));
        assert_eq!(state.extents,[0xa000|width,0xb000|height,horizontal,4096]);
        assert_eq!(state.output,output); assert_eq!(state.interlaced,interlaced);
        assert_eq!(state.copied,(0xe000|height,0xf000|width)); assert_eq!(state.origins,(0,0));
    }
}
#[test]
fn calibrated_routes_distinguish_output_dimensions_from_hardware_extents() {
    let mut calibration=Calibration(Vec::from([(0xe2e,0x12),(0xe2f,0x34),(0xe30,0x56),(0xe31,0x78),(0xe2c,0),
        (0xe32,0x9a),(0xe33,0xbc),(0xe34,0xde),(0xe35,0xf0),(0xe2d,3)]));
    let mut state=Initialization::new();
    engine_initialize(TimingRoute::One,TimingMode::parse(7).unwrap(),&mut state,&mut calibration);
    assert_eq!(state.extents,[0xa234,0xb678,4096,4096]); assert_eq!(state.output,(720,480)); assert!(!state.interlaced);
    engine_initialize(TimingRoute::Two,TimingMode::parse(1).unwrap(),&mut state,&mut calibration);
    assert_eq!(state.extents,[0xaabc,0xbef0,4096,4096]); assert_eq!(state.output,(2749,3825)); assert!(state.interlaced);
}
#[test]
fn native_timing_packet_codec_rejects_each_invalid_domain() {
    assert!(decode_timing_configuration([9,3,5]).is_some());
    for words in [[10,3,5],[9,4,5],[9,3,6],[u32::MAX,0,0]] { assert!(decode_timing_configuration(words).is_none()); }
}
#[derive(Default)]
struct Device { writes:Vec<(Register,u32)> }
impl Hardware for Device {
    fn read(&mut self,_:Register)->u32 { 0 }
    fn write(&mut self,register:Register,value:u32) { self.writes.push((register,value)); }
    fn enter_critical(&mut self)->u32 { 0 }
    fn leave_critical(&mut self,_:u32) {}
    fn yield_zero(&mut self) {}
}
#[test]
fn timing_frontend_resets_previous_defaults_and_forces_control_publication() {
    let mut states=[crate::timing::TimingState::zero();2];
    states[1].defaults=crate::timing::DefaultSelection(7);
    states[1].registers.defaults=0xffff_ffff;
    let mut packet=decode_timing_configuration([2,3,2]).unwrap();
    let mut hardware=Device::default();
    timing_configuration(Engine::One,&mut states,&mut packet,&mut Calibration(Vec::new()),&mut hardware);
    assert_eq!(states[1].defaults.0,0);
    assert_eq!(states[1].registers.defaults,0);
    assert_eq!(states[0].selected_engine,Engine::One);
    assert!(hardware.writes.contains(&(Register{engine:Engine::One,bank:Bank::Timing,offset:0},states[1].registers.control)));
    assert_eq!(states[1].pending.0,0);
}
#[test]
fn formatter_frontend_publishes_even_when_configuration_is_unchanged() {
    use crate::panel_formatter::*;
    let mut state=PanelFormatterState::zero();
    state.active_slot=BufferSlot::Two;
    let configuration=state.configuration();
    let mut hardware=Device::default();
    assert_eq!(formatter_slot(Engine::Zero,&mut state,configuration,&mut Calibration(Vec::new()),&mut hardware),Ok(()));
    let first=hardware.writes.clone();
    assert!(!first.is_empty());
    hardware.writes.clear();
    assert_eq!(formatter_slot(Engine::Zero,&mut state,configuration,&mut Calibration(Vec::new()),&mut hardware),Ok(()));
    assert_eq!(hardware.writes,first);
}

#[test]
fn live_formatter_frontend_decodes_distinct_control_input_and_dimension_bytes() {
    use crate::panel_formatter::*;
    let mut state=PanelFormatterState::zero();
    let configuration=FormatterConfiguration { mode:FormatterMode(3),primary_key:PrimaryConfigurationKey(17),source:BufferSlot::One,secondary_key:SecondaryConfigurationKey(19) };
    let mut calibration=Calibration(Vec::from([(0x494,5),(0x493,3),(0x4b8,0x12),(0x4b9,0x34),(0x4be,0x56),(0x4bf,0x78),(0x4bc,0x23),(0x4bd,0x45)]));
    let mut hardware=Device::default();
    assert_eq!(formatter_slot(Engine::One,&mut state,configuration,&mut calibration,&mut hardware),Ok(()));
    assert!(hardware.writes.contains(&(Register{engine:Engine::One,bank:Bank::Formatter,offset:0},0x35)));
    assert!(hardware.writes.contains(&(Register{engine:Engine::One,bank:Bank::Formatter,offset:4},0x1234)));
    assert!(hardware.writes.contains(&(Register{engine:Engine::One,bank:Bank::Formatter,offset:0x20},0x23450678)));
    assert_eq!(state.configuration(),configuration);
}

#[test]
fn direct_calibrated_mode_one_uses_its_own_height_and_vertical_scale() {
    let mut calibration=Calibration(Vec::from([(0x1041,0x13),(0x1040,0x57),(0x1048,0x24),(0x1047,0x68),(0x1043,0x9a),(0x1042,0xbc),(0x104c,0xde),(0x104b,0xf0)]));
    let mut state=Initialization::new();
    engine_initialize(TimingRoute::Direct,TimingMode::parse(1).unwrap(),&mut state,&mut calibration);
    assert_eq!(state.extents,[0xa357,0xb468,0x9abc,0xdef0]);
    assert_eq!(state.output,(856,1129));
    assert!(!state.interlaced);
}

#[test]
fn live_timing_stages_without_forcing_or_zeroing_the_record() {
    let mut states=[crate::timing::TimingState::zero();2];
    states[1].force=crate::timing::ForceUpdate(0);
    states[1].defaults=crate::timing::DefaultSelection(7);
    let mut packet=decode_timing_configuration([2,3,2]).unwrap();
    let mut hardware=Device::default();
    let result=live_timing(Engine::One,&mut states,&mut packet,&mut Calibration(Vec::new()),&mut hardware);
    assert_eq!(result,0);
    assert_eq!(states[1].force.0,0);
    assert_eq!(states[1].defaults.0,7);
    assert_eq!(states[1].pending.0,0);
    assert!(hardware.writes.contains(&(Register{engine:Engine::One,bank:Bank::Timing,offset:8},states[1].registers.mode_pulse)));
    assert!(!hardware.writes.iter().any(|(register,_)| *register==Register{engine:Engine::One,bank:Bank::Timing,offset:0}));
}
