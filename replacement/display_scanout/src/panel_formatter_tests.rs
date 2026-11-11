use crate::panel_formatter::*;
use crate::timing::{CalibrationRead, PendingUpdate};
use crate::{Bank, Engine, Hardware, Register};
use calibration_liveview::{CalibrationByte, CalibrationOffset};
use std::{vec, vec::Vec};

#[derive(Default)]
struct Calibration {
    values: Vec<(usize, u8)>,
    reads: Vec<usize>,
}
impl Calibration {
    fn pair(&mut self, high: usize, value: u16) {
        let [upper, lower] = value.to_be_bytes();
        self.values.extend([(high, upper), (high + 1, lower)]);
    }
}
impl CalibrationRead for Calibration {
    fn read_byte(&mut self, offset: CalibrationOffset) -> CalibrationByte {
        let key = offset.byte_offset();
        self.reads.push(key);
        CalibrationByte(self.values.iter().find(|value| value.0 == key).map_or(0, |value| value.1))
    }
}
#[derive(Default)]
struct Device { writes: Vec<(Register, u32)> }
impl Hardware for Device {
    fn read(&mut self, _: Register) -> u32 { panic!("unexpected register read"); }
    fn write(&mut self, register: Register, value: u32) { self.writes.push((register, value)); }
    fn enter_critical(&mut self) -> u32 { panic!("unexpected critical section"); }
    fn leave_critical(&mut self, _: u32) { panic!("unexpected critical section"); }
    fn yield_zero(&mut self) { panic!("unexpected yield"); }
}
fn write(engine: Engine, offset: u16, value: u32) -> (Register, u32) {
    (Register { engine, bank: Bank::Formatter, offset }, value)
}
fn mode(value: u32) -> BufferMode { BufferMode::parse(value).unwrap() }
fn configuration(engine: Engine, value: u32) -> FormatterConfiguration {
    FormatterConfiguration {
        mode: FormatterMode(value), primary_key: PrimaryConfigurationKey(0xa51234ff),
        source: match engine { Engine::Zero => BufferSlot::Two, Engine::One => BufferSlot::One },
        secondary_key: SecondaryConfigurationKey(0xfedcba98),
    }
}

#[test]
fn source_selection_publishes_both_headers_without_live_hardware() {
    let mut states = [PanelFormatterState::zero(); 2];
    states[0].source_mode = RecordedSourceMode(1);
    states[1].source_mode = RecordedSourceMode(3);
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    initial_live_configuration(false, &mut states, SourceConfiguration { mode: SourceMode::Zero, selector: SourceSelector::Two, flip: Flip::Flipped }, &mut calibration, &mut hardware).unwrap();
    assert_eq!(states[0].previous_source_mode, RecordedSourceMode(1));
    assert_eq!(states[1].previous_source_mode, RecordedSourceMode(3));
    for state in states {
        assert_eq!(state.source_mode, RecordedSourceMode(0));
        assert_eq!(state.selector, RecordedSelector(2));
        assert_eq!(state.flip, RecordedFlip(1));
    }
    assert!(calibration.reads.is_empty());
    assert!(hardware.writes.is_empty());
}

#[test]
fn source_category_selects_all_five_categories() {
    for (buffer, enabled, source, selector, flip, expected) in [
        (1, false, SourceMode::Three, SourceSelector::Three, Flip::Flipped, SourceCategory::Disabled),
        (1, true, SourceMode::One, SourceSelector::Zero, Flip::Normal, SourceCategory::Three),
        (4, true, SourceMode::Three, SourceSelector::Three, Flip::Flipped, SourceCategory::Four),
        (5, true, SourceMode::Zero, SourceSelector::Three, Flip::Normal, SourceCategory::One),
        (7, true, SourceMode::Zero, SourceSelector::Zero, Flip::Flipped, SourceCategory::One),
        (8, true, SourceMode::Zero, SourceSelector::Zero, Flip::Normal, SourceCategory::Two),
        (0, true, SourceMode::Three, SourceSelector::Zero, Flip::Flipped, SourceCategory::Two),
        (3, true, SourceMode::One, SourceSelector::Three, Flip::Normal, SourceCategory::One),
    ] {
        let mut state = PanelFormatterState::zero();
        state.mode = FormatterMode(buffer);
        state.enabled = EnabledState(u32::from(enabled));
        state.source_mode = RecordedSourceMode(source.raw());
        state.selector = RecordedSelector(selector.raw());
        state.flip = RecordedFlip(flip.raw());
        assert_eq!(source_category(&state), expected);
    }
}

#[test]
fn slot_changes_publish_only_the_active_one_or_two_slot() {
    for active in [BufferSlot::Zero, BufferSlot::One, BufferSlot::Two, BufferSlot::Three] {
        for slot in [BufferSlot::Zero, BufferSlot::One, BufferSlot::Two, BufferSlot::Three] {
            let mut state = PanelFormatterState::zero();
            state.active_slot = active;
            let mut calibration = Calibration::default();
            let mut hardware = Device::default();
            set_slot_mode(Engine::Zero, &mut state, slot, mode(7), SourceCategory::Two, &mut calibration, &mut hardware).unwrap();
            assert_eq!(state.slot_modes[slot.index()], mode(7));
            assert_eq!(hardware.writes.len(), if active == slot && matches!(slot, BufferSlot::One | BufferSlot::Two) { 15 } else { 0 });
        }
    }
}

#[test]
fn geometry_saturates_and_channel_fields_preserve_reserved_bits() {
    let mut state = PanelFormatterState::zero();
    state.active_slot = BufferSlot::One;
    state.registers.channels = [ChannelTransform { coefficients: 0xe000e000, offset: 0xa5a5e000 }; 3];
    state.registers.alternate_channels = [ChannelTransform { coefficients: 0xe000e000, offset: 0x5a5ae000 }; 3];
    let mut calibration = Calibration::default();
    for (offset, value) in [(0x4c4,0xffff),(0x4d0,0x2000),(0x4dc,0x1234),(0x4e8,0xffff),(0x4f4,0x1111),(0x8f1,0x3abc),(0x8f3,0x5678),(0x8f5,0x9abc)] { calibration.pair(offset, value); }
    let mut hardware = Device::default();
    set_slot_mode(Engine::One, &mut state, BufferSlot::One, mode(10), SourceCategory::Disabled, &mut calibration, &mut hardware).unwrap();
    assert_eq!(&hardware.writes[..5], &[
        write(Engine::One,0x24,0xfff), write(Engine::One,0x28,0x1fff1234), write(Engine::One,0x2c,0x1fff1111),
        write(Engine::One,0x4c,0xfabcf678), write(Engine::One,0x50,0xa5a5fabc),
    ]);
    assert_eq!(&hardware.writes[9..], &[
        write(Engine::One,0x100,0xe400e000), write(Engine::One,0x104,0x5a5ae000),
        write(Engine::One,0x108,0xe000e400), write(Engine::One,0x10c,0x5a5ae000),
        write(Engine::One,0x110,0xe000e000), write(Engine::One,0x114,0x5a5ae400),
    ]);
}

#[test]
fn geometry_calibration_respects_category_previous_mode_selector_and_flip() {
    for (category, previous, selector, flip, buffer, expected_extent) in [
        (SourceCategory::Two, SourceMode::Zero, SourceSelector::Zero, Flip::Normal, 0, 101),
        (SourceCategory::Disabled, SourceMode::Zero, SourceSelector::Three, Flip::Flipped, 0, 100),
        (SourceCategory::Four, SourceMode::Zero, SourceSelector::Three, Flip::Flipped, 0, 102),
        (SourceCategory::Three, SourceMode::Three, SourceSelector::Zero, Flip::Normal, 0, 102),
        (SourceCategory::Three, SourceMode::Three, SourceSelector::Three, Flip::Normal, 0, 103),
        (SourceCategory::Two, SourceMode::Zero, SourceSelector::Zero, Flip::Flipped, 3, 101),
        (SourceCategory::Two, SourceMode::Zero, SourceSelector::Zero, Flip::Flipped, 0, 103),
    ] {
        let mut state = PanelFormatterState::zero();
        state.active_slot = BufferSlot::One;
        state.previous_source_mode = RecordedSourceMode(previous.raw());
        state.selector = RecordedSelector(selector.raw());
        state.flip = RecordedFlip(flip.raw());
        state.mode = FormatterMode(buffer);
        let mut calibration = Calibration::default();
        for (offset, value) in [(0x4c4,100),(0x4c6,101),(0x53e,102),(0x502,103)] { calibration.pair(offset, value); }
        let mut hardware = Device::default();
        set_slot_mode(Engine::One, &mut state, BufferSlot::One, mode(6), category, &mut calibration, &mut hardware).unwrap();
        assert_eq!(hardware.writes[0], write(Engine::One,0x24,expected_extent));
    }
}

#[test]
fn indexed_channel_calibration_uses_slot_mode_and_category() {
    for (category, high, low, offset) in [(SourceCategory::One,0x769,0x77f,0x795),(SourceCategory::Two,0x6a3,0x6b9,0x6cf),(SourceCategory::Four,0x82f,0x845,0x85b)] {
        let mut state = PanelFormatterState::zero();
        state.active_slot = BufferSlot::One;
        let mut calibration = Calibration::default();
        calibration.pair(high, 0x1234);
        calibration.pair(low, 0x5678);
        calibration.pair(offset, 0x9abc);
        let mut hardware = Device::default();
        set_slot_mode(Engine::One, &mut state, BufferSlot::One, mode(2), category, &mut calibration, &mut hardware).unwrap();
        assert_eq!(hardware.writes[3], write(Engine::One,0x4c,0x12341678));
        assert_eq!(hardware.writes[4], write(Engine::One,0x50,0x1abc));
    }
}

#[test]
fn buffer_mode_ten_replaces_all_transform_words_with_identity() {
    for engine in [Engine::Zero, Engine::One] {
        let mut state = PanelFormatterState::zero();
        state.mode = FormatterMode(10);
        state.active_slot = BufferSlot::Two;
        state.registers.channels = [ChannelTransform { coefficients: u32::MAX, offset: u32::MAX }; 3];
        state.registers.alternate_channels = state.registers.channels;
        let mut calibration = Calibration::default();
        let mut hardware = Device::default();
        set_slot_mode(engine, &mut state, BufferSlot::Two, mode(0), SourceCategory::Two, &mut calibration, &mut hardware).unwrap();
        assert_eq!(hardware.writes, vec![
            write(engine,0x24,0),write(engine,0x28,0),write(engine,0x2c,0),
            write(engine,0x4c,0x04000000),write(engine,0x50,0),write(engine,0x54,0x400),write(engine,0x58,0),write(engine,0x5c,0),write(engine,0x60,0x400),
            write(engine,0x100,0x04000000),write(engine,0x104,0),write(engine,0x108,0x400),write(engine,0x10c,0),write(engine,0x110,0),write(engine,0x114,0x400),
        ]);
        assert!(calibration.reads.is_empty());
    }
}

#[test]
fn configure_suppresses_equal_packets_and_preserves_uninterpreted_identity_keys() {
    let mut state = PanelFormatterState::zero();
    let packet = configuration(Engine::Zero, 0);
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    configure(Engine::Zero, &mut state, packet, &mut calibration, &mut hardware).unwrap();
    assert_eq!(state.configuration(), packet);
    assert_eq!(state.formatter.pending, PendingUpdate(0));
    assert_eq!(hardware.writes.len(), 50);
    calibration.reads.clear();
    hardware.writes.clear();
    configure(Engine::Zero, &mut state, packet, &mut calibration, &mut hardware).unwrap();
    assert!(calibration.reads.is_empty());
    assert!(hardware.writes.is_empty());
}

#[test]
fn base_stage_selects_enabled_disabled_mode_three_and_external_calibration() {
    for (engine, buffer, enabled, primary, channel, expected_primary, expected_channel) in [
        (Engine::One,0,true,0xb8c,0xba3,0x12345678,0x11223344),
        (Engine::One,3,true,0xc4a,0xc61,0x12345678,0x11223344),
        (Engine::One,0,false,0xd08,0xd1f,0x12345678,0x11223344),
        (Engine::Zero,0,true,0xc4a,0xc61,0x12345678,0x11223344),
        (Engine::One,10,true,0xb8c,0xba3,0x12345678,0x40404040),
    ] {
        let mut state = PanelFormatterState::zero();
        state.enabled = EnabledState(u32::from(enabled));
        state.formatter.registers.channel_control = 0xab000000;
        let mut calibration = Calibration::default();
        calibration.values.extend([(primary,0x12),(primary+1,0x34),(primary+2,0x56),(primary+3,0x78),(primary+20,0x9a),(primary+21,0xbc),(primary+22,0xde),(channel,0x11),(channel+1,0x22),(channel+2,0x33),(channel+3,0x44)]);
        let mut hardware = Device::default();
        configure(engine, &mut state, configuration(engine,buffer), &mut calibration, &mut hardware).unwrap();
        assert!(hardware.writes.contains(&write(engine,0x64,expected_primary)));
        assert!(hardware.writes.contains(&write(engine,0x78,0xab9abcde)));
        assert!(hardware.writes.contains(&write(engine,0x7c,expected_channel)));
        assert_eq!(hardware.writes.len(),50);
    }
}

#[test]
fn base_stage_packs_control_dimensions_coefficients_and_plane_parameters() {
    let mut state = PanelFormatterState::zero();
    state.enabled = EnabledState(1);
    state.formatter.registers.parameters_38 = [0xfc00f800;5];
    let mut calibration = Calibration::default();
    calibration.values.extend([
        (0x494,0xff),(0x493,0xff),(0x492,0xff),(0x491,0xff),(0x490,0xff),(0x48f,0xff),(0x48e,0xff),(0x48d,0xff),(0x48c,0xff),(0x48b,0xff),(0x48a,0xff),(0x489,0xff),(0x488,0xff),(0x487,0xff),
        (0x62d,3),(0x62f,0xff),(0x631,0x12),(0x633,0x34),
        (0x63e,0x56),(0x63f,0x78),(0x640,0x9a),(0x641,0xbc),
        (0x4a6,1),(0x4a5,3),(0x4a4,1),(0x4a3,1),
    ]);
    for (offset,value) in [(0x4b8,0xffff),(0x4be,0xfabc),(0x4bc,0xcdef),(0x644,0xffff),(0x642,0xffff),(0xdcc,0xffff),(0xdc8,0xffff),(0x4a9,0xffff),(0x4a7,0x2345),(0x4ab,0xffff),(0x4ad,0x1234),(0x4af,0x4567),(0x4b1,0x89ab),(0x4b3,0xffff),(0x4b5,0x3456)] { calibration.pair(offset,value); }
    calibration.values.extend([(0xdcb,3),(0xdca,3),(0xdc7,0xff)]);
    stage_configuration(Engine::One,&mut state,configuration(Engine::One,0),&mut calibration);
    assert_eq!(state.formatter.pending,PendingUpdate(1));
    assert_eq!(state.pipeline_control,0x013f1234);
    assert_eq!(state.plane,PlaneRegisters { control:0x1131,dimensions:0x03451fff,start:0x3fff1234,end:0x056789ab,offset:0x1fff,limit:0x1456 });
    let mut hardware = Device::default();
    crate::timing::formatter_flush(Engine::One,&mut state.formatter,&mut hardware);
    for (offset,value) in [(0,0xeffff7f7),(4,0x1fff),(0x20,0x4def0abc),(0x34,0x56789abc),(0x38,0xffffffff),(0xc4,0xf1ffdfff)] {
        assert!(hardware.writes.contains(&write(Engine::One,offset,value)), "register {offset:#x}");
    }
}

#[test]
fn disabled_plane_preserves_unwritten_plane_parameters_and_external_mode_preserves_live_only_fields() {
    for engine in [Engine::Zero,Engine::One] {
        let mut state = PanelFormatterState::zero();
        state.pipeline_control = u32::MAX;
        state.plane = PlaneRegisters { control:1,dimensions:2,start:3,end:4,offset:5,limit:6 };
        state.formatter.registers.mode = 0x12345678;
        state.formatter.registers.parameters_38 = [0x87654321;5];
        let mut calibration = Calibration::default();
        stage_configuration(engine,&mut state,configuration(engine,0),&mut calibration);
        assert_eq!(state.pipeline_control,0);
        assert_eq!(state.plane,PlaneRegisters { control:if engine == Engine::One {0} else {1},dimensions:2,start:3,end:4,offset:5,limit:6 });
        if engine == Engine::Zero {
            assert_eq!(state.formatter.registers.mode,0x12345678);
            assert_eq!(state.formatter.registers.parameters_38,[0x87654321;5]);
            assert!(!calibration.reads.contains(&0x494));
        }
    }
}

#[test]
fn enabling_updates_both_engines_and_only_live_changes_restage() {
    let mut states = [PanelFormatterState::zero();2];
    states[1].active_slot = BufferSlot::One;
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    set_enabled(false,&mut states,true,&mut calibration,&mut hardware).unwrap();
    assert_eq!([states[0].enabled,states[1].enabled],[EnabledState(1);2]);
    assert!(calibration.reads.is_empty());
    assert!(hardware.writes.is_empty());
    set_enabled(true,&mut states,false,&mut calibration,&mut hardware).unwrap();
    assert_eq!([states[0].enabled,states[1].enabled],[EnabledState(0);2]);
    assert_eq!(hardware.writes.len(),50);
    calibration.reads.clear();
    hardware.writes.clear();
    set_enabled(true,&mut states,false,&mut calibration,&mut hardware).unwrap();
    assert!(calibration.reads.is_empty());
    assert!(hardware.writes.is_empty());
}

#[test]
fn live_initial_configuration_uses_previous_mode_and_publishes_engine_one() {
    let mut states = [PanelFormatterState::zero();2];
    states[1].mode = FormatterMode(1);
    states[1].enabled = EnabledState(1);
    states[1].active_slot = BufferSlot::One;
    states[1].slot_modes[1] = mode(6);
    states[1].source_mode = RecordedSourceMode(3);
    let mut calibration = Calibration::default();
    calibration.pair(0x53e,0x123);
    calibration.pair(0x903,0x456);
    calibration.pair(0x905,0x789);
    calibration.pair(0x907,0xabc);
    let mut hardware = Device::default();
    initial_live_configuration(true,&mut states,SourceConfiguration { mode:SourceMode::One,selector:SourceSelector::Zero,flip:Flip::Normal },&mut calibration,&mut hardware).unwrap();
    assert_eq!(hardware.writes.len(),15);
    assert_eq!(hardware.writes[0],write(Engine::One,0x24,0x123));
    assert_eq!(hardware.writes[3],write(Engine::One,0x4c,0x04560789));
    assert_eq!(hardware.writes[4],write(Engine::One,0x50,0xabc));
    assert_eq!(states[1].previous_source_mode,RecordedSourceMode(3));
    assert_eq!(states[1].slot_modes[1],mode(6));
    assert_eq!(states[0].source_mode,RecordedSourceMode(1));
}

#[test]
fn mode_three_uses_its_pipeline_coefficient_and_final_control_calibration() {
    let mut state = PanelFormatterState::zero();
    state.enabled = EnabledState(1);
    let mut calibration = Calibration::default();
    calibration.values.extend([(0x62e,1),(0x630,2),(0x632,3),(0x634,4),(0x66e,0x12),(0x66f,0x34),(0x670,0x56),(0x671,0x78),(0xdd2,1),(0xdd1,0),(0xdce,5)]);
    calibration.pair(0x674,0x321);
    calibration.pair(0x672,0x123);
    calibration.pair(0xdd3,0x567);
    calibration.pair(0xdcf,0x89);
    stage_configuration(Engine::One,&mut state,configuration(Engine::One,3),&mut calibration);
    assert_eq!(state.pipeline_control,0x01020304);
    let mut hardware = Device::default();
    crate::timing::formatter_flush(Engine::One,&mut state.formatter,&mut hardware);
    for (offset,value) in [(0x34,0x12345678),(0x38,0x01230321),(0xc4,0x50894567)] {
        assert!(hardware.writes.contains(&write(Engine::One,offset,value)));
    }
    assert!(!calibration.reads.contains(&0x62d));
    assert!(!calibration.reads.contains(&0x644));
    assert!(!calibration.reads.contains(&0xdcc));
}

#[test]
fn noncanonical_nonzero_enable_still_republishes_canonical_enabled_state() {
    let mut states = [PanelFormatterState::zero();2];
    for state in &mut states { state.enabled = EnabledState(2); }
    states[1].active_slot = BufferSlot::One;
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    set_enabled(true,&mut states,true,&mut calibration,&mut hardware).unwrap();
    assert_eq!([states[0].enabled,states[1].enabled],[EnabledState(1);2]);
    assert_eq!(hardware.writes.len(),50);
}

#[test]
fn formatter_mode_retains_all_bits_without_limiting_slot_mode_domain() {
    let mut state = PanelFormatterState::zero();
    let packet = configuration(Engine::Zero,u32::MAX);
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    configure(Engine::Zero,&mut state,packet,&mut calibration,&mut hardware).unwrap();
    assert_eq!(state.configuration(),packet);
    assert_eq!(hardware.writes.len(),50);
    assert_eq!(state.slot_modes[BufferSlot::Two.index()],mode(0));
    assert!(BufferMode::parse(11).is_none());
}

#[test]
fn configuration_history_retains_full_recorded_words() {
    let mut states = [PanelFormatterState::zero();2];
    states[0].source_mode = RecordedSourceMode(0xfedcba98);
    states[1].source_mode = RecordedSourceMode(u32::MAX);
    let mut calibration = Calibration::default();
    let mut hardware = Device::default();
    initial_live_configuration(false,&mut states,SourceConfiguration { mode:SourceMode::One,selector:SourceSelector::Three,flip:Flip::Normal },&mut calibration,&mut hardware).unwrap();
    assert_eq!(states[0].previous_source_mode,RecordedSourceMode(0xfedcba98));
    assert_eq!(states[1].previous_source_mode,RecordedSourceMode(u32::MAX));
    assert!(hardware.writes.is_empty());
}
