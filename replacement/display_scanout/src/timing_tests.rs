use crate::timing::*;
use crate::{Bank, Engine, Hardware, Register};
use calibration_liveview::{CalibrationByte, CalibrationOffset};
use std::{collections::VecDeque, vec, vec::Vec};

// Synthetic register scripts and calibration bytes are authored from the instruction contract.
#[derive(Debug, PartialEq, Eq)]
enum Effect { Read(Bank, u16, u32), Write(Bank, u16, u32), Enter, Leave(u32), Yield }
struct Device { engine: Engine, reads: VecDeque<u32>, effects: Vec<Effect> }
impl Device {
    fn new(engine: Engine, reads: &[u32]) -> Self { Self { engine, reads: reads.iter().copied().collect(), effects: Vec::new() } }
}
impl Hardware for Device {
    fn read(&mut self, register: Register) -> u32 {
        assert_eq!(register.engine, self.engine);
        let value = self.reads.pop_front().expect("held handshake");
        self.effects.push(Effect::Read(register.bank, register.offset, value));
        value
    }
    fn write(&mut self, register: Register, value: u32) {
        assert_eq!(register.engine, self.engine);
        self.effects.push(Effect::Write(register.bank, register.offset, value));
    }
    fn enter_critical(&mut self) -> u32 { self.effects.push(Effect::Enter); 0x42 }
    fn leave_critical(&mut self, token: u32) { self.effects.push(Effect::Leave(token)); }
    fn yield_zero(&mut self) { self.effects.push(Effect::Yield); }
}
struct CalibrationFixture { values: Vec<(usize, u8)>, reads: Vec<usize> }
impl CalibrationFixture {
    fn defaults() -> Self { Self { values: vec![(0x1083,0xfe),(0x1081,0xab),(0x1082,0xcd),(0x1080,3)], reads: Vec::new() } }
}
impl CalibrationRead for CalibrationFixture {
    fn read_byte(&mut self, offset: CalibrationOffset) -> CalibrationByte {
        let offset = offset.byte_offset();
        self.reads.push(offset);
        CalibrationByte(self.values.iter().find(|entry| entry.0 == offset).map_or(0, |entry| entry.1))
    }
}

#[test]
fn immediate_uses_defaults_only_for_unselected_fields() {
    for engine in [Engine::Zero, Engine::One] {
        for selection in 0..8 {
            let mut calibration = CalibrationFixture::defaults();
            let mut state = TimingState::zero();
            state.registers.defaults = 0xe000fff8;
            state.pending = PendingUpdate(7);
            state.defaults = DefaultSelection(0);
            let mut device = Device::new(engine, &[]);
            let result = immediate(engine, &mut state, ImmediatePacket { interval: 0xf234, selector: 0xfb, flag: 0xfe, selection: DefaultSelection(selection) }, &mut calibration, &mut device);
            let expected = 0xe000fff8 | if selection & 1 == 0 { 0x0bcd0000 } else { 0x02340000 }
                | if selection & 2 == 0 { 6 } else { 3 }
                | if selection & 4 == 0 { 0x10000000 } else { 0 };
            assert_eq!(result, 0);
            assert_eq!(state.registers.defaults, expected);
            assert_eq!(state.pending, PendingUpdate(7));
            assert_eq!(state.defaults, DefaultSelection(selection));
            assert_eq!(device.effects, vec![Effect::Write(Bank::Timing,0x34,expected)]);
            let mut expected_reads = Vec::new();
            if selection & 2 == 0 { expected_reads.push(0x1083); }
            if selection & 1 == 0 { expected_reads.extend([0x1081,0x1082]); }
            if selection & 4 == 0 { expected_reads.push(0x1080); }
            assert_eq!(calibration.reads, expected_reads);
        }
    }
}

#[test]
fn flush_writes_parameters_even_when_control_is_equal() {
    for engine in [Engine::Zero, Engine::One] {
        let mut state = TimingState::zero();
        let mut device = Device::new(engine, &[]);
        assert_eq!(flush(engine, &mut state, &mut device), 0);
        assert!(device.effects.is_empty());
        state.pending = PendingUpdate(2);
        state.force = ForceUpdate(2);
        state.registers = TimingRegisters {
            control: 0x120, dimensions: 0xabcdef,
            mode_pulse: 0x1028, mode_interval: 0x102c,
            route_pulse: 0x1030, route_interval: 0x1034,
            mode_delay: 0x1038, output_pulse: 0x103c, output_interval: 0x1040,
            secondary_output_pulse: 0x1044, secondary_output_interval: 0x1048,
            threshold: 0x104c, defaults: 0x1050, route_adjustment: 0x1054,
        };
        device.reads.extend([0x20,0xabcdef]);
        assert_eq!(flush(engine, &mut state, &mut device), 0);
        assert_eq!(state.pending, PendingUpdate(0));
        assert_eq!(state.force, ForceUpdate(2));
        let mut expected = vec![
            Effect::Write(Bank::Timing,8,0x1028), Effect::Write(Bank::Timing,12,0x102c),
            Effect::Write(Bank::Timing,16,0x1030), Effect::Write(Bank::Timing,20,0x1034),
            Effect::Write(Bank::Timing,24,0x1038), Effect::Write(Bank::Timing,28,0x103c),
            Effect::Write(Bank::Timing,32,0x1040),
        ];
        if engine == Engine::One { expected.extend([Effect::Write(Bank::Timing,36,0x1044),Effect::Write(Bank::Timing,40,0x1048)]); }
        expected.extend([Effect::Write(Bank::Timing,44,0x104c),Effect::Write(Bank::Timing,52,0x1050),Effect::Write(Bank::Timing,56,0x1054),Effect::Read(Bank::Timing,0,0x20),Effect::Read(Bank::Timing,4,0xabcdef)]);
        assert_eq!(device.effects, expected);
    }
}

#[test]
fn force_exactly_one_and_zero_secondary_control() {
    for force in [0,1,2] {
        let mut state = TimingState::zero();
        state.pending = PendingUpdate(1);
        state.force = ForceUpdate(force);
        let mut device = Device::new(Engine::Zero, &[0,0]);
        assert_eq!(flush(Engine::Zero,&mut state,&mut device), u32::from(force == 1));
        if force == 1 {
            assert_eq!(state.force, ForceUpdate(0));
            assert_eq!(device.effects.last(),Some(&Effect::Write(Bank::Timing,0,0)));
        }
        assert!(!device.effects.iter().any(|effect| matches!(effect, Effect::Write(Bank::Timing,4,_))));
    }
}

#[test]
fn reset_polls_set_and_restores_saved_control() {
    for engine in [Engine::Zero,Engine::One] {
        let mut device = Device::new(engine,&[0xabcd,0x10055,0,0,0x10000]);
        reset(engine,&mut device);
        assert_eq!(device.effects,vec![Effect::Read(Bank::Scanout,0,0xabcd),Effect::Write(Bank::Scanout,0,0x10000),Effect::Read(Bank::Scanout,0,0x10055),Effect::Write(Bank::Scanout,0,0x55),Effect::Read(Bank::Scanout,0,0),Effect::Read(Bank::Scanout,0,0),Effect::Read(Bank::Scanout,0,0x10000),Effect::Write(Bank::Scanout,0,0x1abcd)]);
    }
}

#[test]
fn wait_yields_after_each_failed_status_read() {
    let mut device = Device::new(Engine::One,&[0x12345678,0,1,2]);
    wait_input_ready(Engine::One,&mut device);
    assert_eq!(device.effects,vec![Effect::Read(Bank::Timing,0,0x12345678),Effect::Write(Bank::Timing,0,0x12345678 & !0x10000),Effect::Read(Bank::Timing,0x30,0),Effect::Yield,Effect::Read(Bank::Timing,0x30,1),Effect::Yield,Effect::Read(Bank::Timing,0x30,2)]);
}

#[test]
fn formatter_strobe_releases_critical_before_polling_clear() {
    let mut device = Device::new(Engine::Zero,&[0x45,0x10000045,0x10000045,0x45]);
    formatter_strobe(Engine::Zero,&mut device);
    assert_eq!(device.effects,vec![Effect::Enter,Effect::Read(Bank::Formatter,0,0x45),Effect::Write(Bank::Formatter,0,0x10000045),Effect::Leave(0x42),Effect::Read(Bank::Formatter,0,0x10000045),Effect::Read(Bank::Formatter,0,0x10000045),Effect::Read(Bank::Formatter,0,0x45)]);
}

#[test]
fn formatter_configuration_publishes_only_ordered_selected_offsets() {
    let offsets = [0,4,0x20,0xc8,0x34,0x38,0x3c,0x40,0x44,0x48,0x64,0x68,0x6c,0x70,0x74,0x78,0x7c,0x80,0x84,0x88,0x8c,0x90,0x94,0x98,0x9c,0xa0,0xa4,0xa8,0xac,0xb0,0xb4,0xb8,0xbc,0xc0,0xc4];
    for engine in [Engine::Zero,Engine::One] {
        let mut state = FormatterState::zero();
        let mut device = Device::new(engine,&[]);
        formatter_flush(engine,&mut state,&mut device);
        assert!(device.effects.is_empty());
        state.registers = FormatterRegisters {
            control: 0x4000, input_control: 0x4004, dimensions: 0x4020,
            output_control: 0x40c8, mode: 0x4034,
            parameters_38: [0x4038,0x403c,0x4040,0x4044,0x4048],
            parameters_64: [0x4064,0x4068,0x406c,0x4070,0x4074],
            channel_control: 0x4078,
            channel_parameters: [
                [0x407c,0x4080,0x4084,0x4088,0x408c,0x4090],
                [0x4094,0x4098,0x409c,0x40a0,0x40a4,0x40a8],
                [0x40ac,0x40b0,0x40b4,0x40b8,0x40bc,0x40c0],
            ],
            final_control: 0x40c4,
        };
        state.pending = PendingUpdate(2);
        formatter_flush(engine,&mut state,&mut device);
        assert_eq!(state.pending, PendingUpdate(0));
        assert_eq!(device.effects,offsets.into_iter().map(|offset| Effect::Write(Bank::Formatter,offset,0x4000+u32::from(offset))).collect::<Vec<_>>());
    }
}

#[test]
fn held_handshakes_do_not_return_or_restore_early() {
    for operation in 0..3 {
        let mut device = Device::new(Engine::Zero,match operation { 0 => &[0x44,0,0,0][..], 1 => &[0,0,0,0][..], _ => &[0,0x10000000,0x10000000,0x10000000][..] });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match operation {
            0 => reset(Engine::Zero,&mut device),
            1 => wait_input_ready(Engine::Zero,&mut device),
            _ => formatter_strobe(Engine::Zero,&mut device),
        }));
        assert!(result.is_err());
        if operation == 0 { assert!(!device.effects.contains(&Effect::Write(Bank::Scanout,0,0x10044))); }
        if operation == 1 { assert_eq!(device.effects.iter().filter(|effect| **effect == Effect::Yield).count(),3); }
        if operation == 2 { assert!(device.effects.contains(&Effect::Leave(0x42))); }
    }
}

#[test]
fn staging_normalizes_caller_rate_after_recording_original_and_keeps_selected_defaults() {
    let mut states = [TimingState::zero(),TimingState::zero()];
    states[1].registers.threshold = 0xf0000000;
    states[1].defaults = DefaultSelection(7);
    states[1].registers.defaults = 0x87654321;
    let mut calibration = CalibrationFixture { values: vec![(0x1052,0x12),(0x1053,0x34),(0x1050,0x56),(0x1051,0x78),(0x104e,3),(0x106c,0x9a)], reads: Vec::new() };
    let mut packet = TimingConfiguration { mode: TimingMode::parse(0).unwrap(),route:TimingRoute::Direct,rate:TimingRate::parse(3).unwrap() };
    stage(Engine::One,&mut states,&mut packet,&mut calibration);
    assert_eq!(packet.rate.raw(),2);
    assert_eq!(states[0].selected_engine, Engine::One);
    assert_eq!(states[1].selected_engine, Engine::One);
    assert_eq!(states[1].original.rate.raw(), 3);
    assert_eq!(states[1].pending, PendingUpdate(1));
    assert_eq!(states[1].registers.control, 0x10000);
    assert_eq!(states[1].registers.dimensions, 0x06780234);
    assert_eq!(states[1].registers.mode_delay, 0x9a);
    assert_eq!(states[1].registers.threshold, 0xf0000108);
    assert_eq!(states[1].registers.defaults, 0x87654321);
    assert_eq!(states[1].registers.route_adjustment, 0);
    assert!(!calibration.reads.iter().any(|offset| (0x1080..=0x1083).contains(offset)));
}

#[test]
fn staging_selects_route_rate_rows_and_preserves_unselected_word_bits() {
    for (route, route_row) in [(TimingRoute::Zero,0),(TimingRoute::One,0),(TimingRoute::Two,2)] {
        for rate in 0..6 {
            let expected_row = route_row + usize::from(rate != 1);
            let mut states = [TimingState::zero(),TimingState::zero()];
            states[0].registers.threshold = 0xa5a5e000;
            states[0].registers.defaults = 0xe000fff8;
            let mut calibration = CalibrationFixture::defaults();
            calibration.values.extend([(0x413,100),(0x40f,101),(0x41b,102),(0x417,103)]);
            let mut packet = TimingConfiguration { mode:TimingMode::parse(2).unwrap(),route,rate:TimingRate::parse(rate).unwrap() };
            stage(Engine::Zero,&mut states,&mut packet,&mut calibration);
            assert_eq!(states[0].original.rate.raw(), rate);
            assert_eq!(packet.rate.raw(),if rate == 0 || rate == 3 { 2 } else { rate });
            assert_eq!(states[0].registers.dimensions,100 + expected_row as u32);
            assert_eq!(states[0].registers.threshold & !0x1fff,0xa5a5e000);
            assert_eq!(states[0].registers.defaults,0xfbcdfffe);
            assert_eq!(states[0].registers.mode_delay,0);
            assert_eq!(states[1].pending,PendingUpdate(0));
        }
    }
}

#[test]
fn direct_mode_staging_selects_mode_parameters_and_threshold_margin() {
    let cases = [
        (0, 0x1058, 0x1064, 0x1052, 0x1053, 0x9a, 0x108),
        (1, 0x105e, 0x1068, 0x1056, 0x1057, 0xbc, 0x10a),
        (2, 0x116e, 0x1174, 0x116c, 0x116d, 0x9a, 0x108),
        (3, 0x110a, 0x1110, 0x1108, 0x1109, 0, 0x108),
        (4, 0x110a, 0x1110, 0x1108, 0x1109, 0, 0x10a),
        (5, 0x110a, 0x1110, 0x1108, 0x1109, 0, 0x108),
        (6, 0x113c, 0x1142, 0x113a, 0x113b, 0, 0x10a),
        (7, 0x110a, 0x1110, 0x1108, 0x1109, 0, 0x108),
        (8, 0x10d8, 0x10de, 0x10d6, 0x10d7, 0, 0x108),
        (9, 0x10a6, 0x10ac, 0x10a4, 0x10a5, 0, 0x108),
    ];
    for (mode, pulse, interval, width_high, width_low, delay, threshold) in cases {
        let mut states = [TimingState::zero(), TimingState::zero()];
        let mut calibration = CalibrationFixture {
            values: vec![
                (pulse, 3), (pulse + 1, 0xab), (pulse + 2, 0xcd),
                (pulse + 3, 3), (pulse + 4, 0xe2), (pulse + 5, 0x34),
                (interval, 0xf5), (interval + 1, 0x67), (interval + 2, 0xa8), (interval + 3, 0x9a),
                (width_high, 0x12), (width_low, 0x34), (0x106c, 0x9a), (0x106d, 0xbc),
            ],
            reads: Vec::new(),
        };
        let mut packet = TimingConfiguration {
            mode: TimingMode::parse(mode).unwrap(), route: TimingRoute::Direct, rate: TimingRate::parse(1).unwrap(),
        };
        let original = packet;
        stage(Engine::Zero, &mut states, &mut packet, &mut calibration);
        assert_eq!(states[0].original, original);
        let mut device = Device::new(Engine::Zero, &[0, 0]);
        assert_eq!(flush(Engine::Zero, &mut states[0], &mut device), 1);
        for (offset, value) in [
            (8, 0x1bcd1234), (12, 0x0567089a), (16, 0), (20, 0),
            (24, delay), (44, threshold), (56, 0),
        ] {
            assert!(device.effects.contains(&Effect::Write(Bank::Timing, offset, value)), "mode {mode}, register {offset:#x}");
        }
    }
}
