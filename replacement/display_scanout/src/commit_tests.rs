use crate::commit::*;
use crate::{Bank, Engine, Hardware, Register};
use std::vec::Vec;

#[derive(Default)]
struct Device {
    reads: Vec<Register>,
    writes: Vec<(Register, u32)>,
    reset_writes: usize,
}
impl Hardware for Device {
    fn read(&mut self, register: Register) -> u32 {
        self.reads.push(register);
        if register.offset == 0 && self.reset_writes >= 2 { 0x12350000 } else { 0x12340000 | register.offset as u32 }
    }
    fn write(&mut self, register: Register, value: u32) {
        if register.offset == 0 { self.reset_writes += 1; }
        self.writes.push((register, value));
    }
    fn enter_critical(&mut self) -> u32 { 7 }
    fn leave_critical(&mut self, token: u32) { assert_eq!(token, 7); }
    fn yield_zero(&mut self) { panic!("unexpected yield"); }
}
#[derive(Default)]
struct Callbacks { seen: Vec<u32>, prepared: usize, mutate: bool }
impl CompletionCallbacks for Callbacks {
    fn prepared(&mut self, _engine: Engine, state: &EngineState) {
        assert_eq!(state.runtime.pending, CommitPending(0));
        self.prepared += 1;
    }
    fn call(&mut self, id: CallbackId, slot: CompletionSlot, _engine: Engine, state: &mut EngineState) {
        let callbacks = match slot.list {
            CompletionList::First => &state.runtime.completion_callbacks,
            CompletionList::Second => &state.runtime.secondary_callbacks,
        };
        assert_eq!(callbacks[slot.index()], None);
        self.seen.push(id.0);
        if self.mutate && id == CallbackId(1) {
            state.runtime.completion_callbacks[0] = Some(CallbackId(5));
            state.runtime.completion_callbacks[2] = Some(CallbackId(3));
            state.runtime.secondary_callbacks[0] = Some(CallbackId(4));
            state.runtime.completion_callbacks[1] = None;
        }
    }
}
#[test]
fn callbacks_run_without_pending_and_observe_live_requeue() {
    for engine in [Engine::Zero, Engine::One] {
        for flag in [0, 2, u32::MAX] {
            let mut state = EngineState::zero();
            state.runtime.pending = CommitPending(flag);
            state.runtime.completion_callbacks[0] = Some(CallbackId(1));
            state.runtime.completion_callbacks[1] = Some(CallbackId(2));
            let mut device = Device::default();
            let mut callbacks = Callbacks { mutate: true, ..Callbacks::default() };
            commit(engine, &mut state, CommitOptions::default(), &mut device, &mut callbacks);
            assert_eq!(callbacks.seen, [1, 3, 4]);
            assert_eq!(state.runtime.completion_callbacks[0], Some(CallbackId(5)));
            assert_eq!(state.runtime.pending, CommitPending(flag));
            assert!(device.writes.is_empty());
            assert_eq!(callbacks.prepared, 0);
            commit(engine, &mut state, CommitOptions::default(), &mut device, &mut callbacks);
            assert_eq!(callbacks.seen, [1, 3, 4, 5]);
        }
    }
}
#[test]
fn full_callback_lists_drain_in_order() {
    let mut state = EngineState::zero();
    for index in 0..6 {
        state.runtime.completion_callbacks[index] = Some(CallbackId(index as u32 + 1));
        state.runtime.secondary_callbacks[index] = Some(CallbackId(index as u32 + 7));
    }
    let mut callbacks = Callbacks::default();
    commit_zero(&mut state, CommitOptions::default(), &mut Device::default(), &mut callbacks);
    assert_eq!(callbacks.seen, (1..=12).collect::<Vec<_>>());
    assert_eq!(state.runtime.completion_callbacks, [None; 6]);
    assert_eq!(state.runtime.secondary_callbacks, [None; 6]);
}
fn write_offsets(engine: Engine) -> Vec<u16> {
    let mut offsets = std::vec![0, 4, 8, 12, 16, 20];
    if engine == Engine::One { offsets.push(0x18); }
    offsets.extend([
        0x1c,0x20,0x38,0x3c,0x88,0x8c,0x24,0x28,0x2c,0x30,0xd4,0xd8,0xdc,0x34,0x40,0xe0,0x84,0x90,0xe4,
        0x44,0x54,0x64,0x74,0x94,0xa4,0xb4,0xc4,
        0x48,0x58,0x68,0x78,0x98,0xa8,0xb8,0xc8,
        0x4c,0x5c,0x6c,0x7c,0x9c,0xac,0xbc,0xcc,
        0x50,0x60,0x70,0x80,0xa0,0xb0,0xc0,0xd0,
        0x10c,0x110,0x194,0x114,0x118,0x198,
        0x11c,0x120,0x124,0x128,0x12c,0x130,0x134,0x138,0x13c,0x140,
        0x144,0x148,0x14c,0x150,0x154,0x158,0x15c,0x160,0x164,0x168,
        0x16c,0x170,0x174,0x178,0x17c,0x180,0x184,0x188,0x18c,0x190,0x108,
        0xe8,0xec,0xf0,0xf4,0xf8,0xfc,0x100,0x1bc,0x9d0,0x9d4,
    ]);
    offsets
}
#[test]
fn both_engines_emit_complete_register_sequence_and_preserve_captured_words() {
    for engine in [Engine::Zero, Engine::One] {
        let mut state = EngineState::zero();
        let mut device = Device::default();
        let mut registrar = Registrar { calls: Vec::new(), result: 0 };
        capture(engine, &mut state, config(), &mut device, &mut registrar);
        device.writes.clear();
        state.runtime.pending = CommitPending(1);
        state.input.geometry_control = RegisterWord(0x98760000);
        state.input.origin = RegisterWord(0xc1238123);
        state.input.primary_origins = [RegisterWord(0x40001000); 4];
        state.input.secondary_origins = [RegisterWord(0x50002000); 4];
        for record in state.input.ten_records.iter_mut().chain(state.input.two_records.iter_mut()) {
            record.coordinates = [11.75f32,22.5,33.25,44.875].map(|value| FloatBits(value.to_bits()));
        }
        let mut callbacks = Callbacks::default();
        commit(engine, &mut state, CommitOptions::default(), &mut device, &mut callbacks);
        let expected: Vec<_> = write_offsets(engine).into_iter().map(|offset| {
            let value = match offset {
                0 => 0x12350006,
                4 => 0x98760000,
                0x24 => 0xc1238123,
                0x44..=0x50 if offset % 4 == 0 => 0x40001000,
                0x94..=0xa0 if offset % 4 == 0 => 0x50002000,
                0x10c | 0x114 => 0x100b0016,
                0x110 | 0x118 => 0x1021002c,
                0x11c..=0x168 if offset % 8 == 4 => 0x100b0016,
                0x120..=0x168 if offset % 8 == 0 => 0x1021002c,
                0x194 | 0x198 => 0x10000000,
                0x16c | 0x170 => 0x10000000,
                0x174 => 0x12340174 & !0x0f0fffff,
                0x178..=0x184 => 0,
                0x188 => 0x12340188 & !0x0f0fffff,
                0x18c => 0x12340000,
                0x190 => 0x12340190 & !0x303ff,
                0x108 => 0,
                _ => 0x12340000 | offset as u32,
            };
            (Register { engine, bank: Bank::Scanout, offset }, value)
        }).collect();
        assert_eq!(device.writes, expected);
        assert_eq!(callbacks.prepared, 1);
    }
}
#[test]
fn reversed_coordinates_scale_and_origin_additions_preserve_reserved_bits() {
    let mut state = EngineState::zero();
    state.runtime.pending = CommitPending(1);
    state.runtime.mode = CaptureMode(2);
    state.runtime.auxiliary = ConfigurationAuxiliary(1);
    state.saved.extent = RegisterWord(99 << 16);
    state.saved.format = RegisterWord(0x120);
    state.saved.two_rectangles[0] = RectangleRegisters { horizontal: RegisterWord(0xa000b000), vertical: RegisterWord(0xc000d000) };
    state.saved.two_extents[0] = RegisterWord(0xf800f800);
    state.input.geometry_control = RegisterWord(0x3e);
    state.input.origin = RegisterWord(0xfffffffe);
    state.input.vertical_offset = CoordinateOffset(5);
    state.input.horizontal_offset = CoordinateOffset(12);
    state.input.primary_origins[0] = RegisterWord(0xc0018002);
    state.input.secondary_origins[0] = RegisterWord(0x8003c004);
    state.input.two_records[0].coordinates = [11.75f32,22.5,33.25,44.875].map(|value| FloatBits(value.to_bits()));
    state.input.two_records[0].extent = RectangleExtent { width: -3, height: 7 };
    state.input.two_records[0].reverse = 1;
    commit_one(&mut state, CommitOptions { alternate_masks: false, reverse_vertical: true }, &mut Device::default(), &mut Callbacks::default());
    assert_eq!(state.saved.geometry_control, RegisterWord(0x3f));
    assert_eq!(state.saved.two_rectangles[0], RectangleRegisters { horizontal: RegisterWord(0xa043b016), vertical: RegisterWord(0xc059d02c) });
    assert_eq!(state.saved.two_extents[0], RegisterWord(0xfed4fabc));
    assert_eq!(state.saved.origin, RegisterWord(0xc004c00a));
    assert_eq!(state.saved.primary_planes[0].origin, RegisterWord(0xc0068005));
    assert_eq!(state.saved.secondary_planes[0].origin, RegisterWord(0x8008c00a));
}
#[test]
fn conversions_cover_truncation_zeros_nan_infinities_and_limits() {
    for (bits, signed, unsigned) in [(0,0,0),(0x80000000,0,0),(1,0,0),(0x3ff33333,1,1),(0xbff33333,-1,0),(0x7fc00001,0,0),(0xffc00001,0,0),(0x7f800000,i32::MAX,u32::MAX),(0xff800000,i32::MIN,0),(0x4f000000,i32::MAX,0x80000000),(0x4f7fffff,i32::MAX,0xffffff00),(0xcf000000,i32::MIN,0)] {
        assert_eq!(float_to_signed(FloatBits(bits)), signed);
        assert_eq!(float_to_unsigned(FloatBits(bits)), unsigned);
    }
}
#[test]
fn defaults_float_capacity_retains_prior_categories_and_uses_selected_mask() {
    let mut state = EngineState::zero();
    state.saved.rectangle_mask = RegisterWord(0x1000f);
    state.input.rectangle_mask = RegisterWord(1);
    for (item, value) in [1.0f32,2.0,3.0,4.0,5.0].into_iter().enumerate() {
        let record = if item < 4 { &mut state.input.ten_records[item] } else { &mut state.input.two_records[0] };
        record.level = FloatBits(value.to_bits());
        record.color = ColorWord(17);
        record.opacity = Opacity(128);
    }
    let mut packet = DefaultsCollection::default();
    assert_eq!(collect_defaults(&state, false, &mut packet), -30);
    assert_eq!(packet.levels, [FloatBits(0x3f800000),FloatBits(0x40000000),FloatBits(0x40400000),FloatBits(0x40800000)]);
    assert_eq!(packet.colors, [ColorWord(17),ColorWord(0),ColorWord(0),ColorWord(0)]);
    assert_eq!(packet.opacities, [Opacity(128),Opacity(0)]);
    let mut alternate = DefaultsCollection::default();
    assert_eq!(collect_defaults(&state, true, &mut alternate), 0);
    assert_eq!(alternate.levels, [FloatBits(0x3f800000),FloatBits(0),FloatBits(0),FloatBits(0)]);
}
#[test]
fn integer_capacity_is_independent_and_stops_before_collecting_failing_opacity() {
    let mut state = EngineState::zero();
    state.saved.rectangle_mask = RegisterWord(31);
    for (index, record) in state.input.ten_records[..5].iter_mut().enumerate() {
        record.level = FloatBits(0x3f800000);
        record.color = ColorWord(100 + index as u32);
        record.opacity = Opacity(if index == 4 { 9 } else { 7 });
    }
    let mut packet = DefaultsCollection::default();
    assert_eq!(collect_defaults(&state, false, &mut packet), -30);
    assert_eq!(packet.colors, [ColorWord(100),ColorWord(101),ColorWord(102),ColorWord(103)]);
    assert_eq!(packet.opacities, [Opacity(7),Opacity(0)]);
    assert_eq!(packet.levels, [FloatBits(0x3f800000),FloatBits(0),FloatBits(0),FloatBits(0)]);
}
#[test]
fn byte_capacity_is_independent_and_keeps_level_and_color_from_failing_record() {
    let mut state = EngineState::zero();
    state.saved.rectangle_mask = RegisterWord(7);
    for (index, record) in state.input.ten_records[..3].iter_mut().enumerate() {
        record.level = FloatBits((index as f32 + 1.0).to_bits());
        record.color = ColorWord(10 + index as u32);
        record.opacity = Opacity(20 + index as u8);
    }
    let mut packet = DefaultsCollection::default();
    assert_eq!(collect_defaults(&state, false, &mut packet), -30);
    assert_eq!(packet.levels, [FloatBits(0x3f800000),FloatBits(0x40000000),FloatBits(0x40400000),FloatBits(0)]);
    assert_eq!(packet.colors, [ColorWord(10),ColorWord(11),ColorWord(12),ColorWord(0)]);
    assert_eq!(packet.opacities, [Opacity(20),Opacity(21)]);
}
#[test]
fn signed_zeros_deduplicate_but_identical_nan_payloads_exhaust_capacity() {
    let mut state = EngineState::zero();
    state.saved.rectangle_mask = RegisterWord(3);
    state.input.ten_records[1].level = FloatBits(0x80000000);
    let mut packet = DefaultsCollection { levels: [FloatBits(0xdeadbeef); 4], ..DefaultsCollection::default() };
    assert_eq!(collect_defaults(&state, false, &mut packet), 0);
    assert_eq!(packet.levels, [FloatBits(0),FloatBits(0xdeadbeef),FloatBits(0xdeadbeef),FloatBits(0xdeadbeef)]);
    state.saved.rectangle_mask = RegisterWord(31);
    for record in &mut state.input.ten_records[..5] { record.level = FloatBits(0x7fc00001); }
    let mut packet = DefaultsCollection::default();
    assert_eq!(collect_defaults(&state, false, &mut packet), -30);
    assert_eq!(packet.levels, [FloatBits(0x7fc00001); 4]);
}
struct Registrar { calls: Vec<Engine>, result: i32 }
impl Registration for Registrar {
    fn register_commit(&mut self, engine: Engine, state: &mut EngineState) -> i32 {
        assert_eq!(state.runtime.capture_count, 0);
        assert_eq!(state.saved.control.0 & 6, 6);
        self.calls.push(engine);
        state.runtime.registration = RegistrationId(if self.result == 0 { 71 } else { 0 });
        self.result
    }
}
fn config() -> CaptureConfiguration {
    CaptureConfiguration { mode: CaptureMode(0), selector: ConfigurationSelector(8), auxiliary: ConfigurationAuxiliary(9) }
}
#[test]
fn capture_recaptures_only_first_configuration_and_keeps_registration_errors() {
    for engine in [Engine::Zero, Engine::One] {
        for result in [0,-20,-77] {
            let mut state = EngineState::zero();
            state.saved.rectangle_mask = RegisterWord(0xdeadbeef);
            state.saved.engine_one_control = RegisterWord(0xcafebabe);
            state.runtime.completion_callbacks[0] = Some(CallbackId(99));
            let mut device = Device::default();
            let mut registrar = Registrar { calls: Vec::new(), result };
            assert_eq!(capture(engine, &mut state, config(), &mut device, &mut registrar), result);
            assert_eq!(state.runtime.capture_count, 1);
            assert_eq!(state.runtime.mode, CaptureMode(if engine == Engine::Zero { 3 } else { 1 }));
            assert_eq!(state.runtime.completion_callbacks, [None; 6]);
            assert_eq!(state.saved.rectangle_mask, RegisterWord(0xdeadbeef));
            assert_eq!(state.saved.interrupt_end, RegisterWord(0x123409d4));
            assert_eq!(registrar.calls, [engine]);
            let expected: Vec<_> = write_offsets(engine).into_iter().filter(|&offset| offset != 0x108).collect();
            let captured: Vec<_> = device.reads[3..].iter().map(|register| register.offset).collect();
            assert_eq!(captured, expected);
            if engine == Engine::Zero { assert_eq!(state.saved.engine_one_control, RegisterWord(0xcafebabe)); }
            let reads = device.reads.len();
            let same = CaptureConfiguration { mode: state.runtime.mode, ..config() };
            assert_eq!(capture(engine, &mut state, same, &mut device, &mut registrar), 0);
            assert_eq!(state.runtime.capture_count, 1);
            assert_eq!(capture(engine, &mut state, config(), &mut device, &mut registrar), 0);
            assert_eq!(state.runtime.capture_count, 2);
            assert_eq!(state.runtime.mode, CaptureMode(0));
            assert_eq!(device.reads.len(), reads);
            assert_eq!(registrar.calls.len(), 1);
            state.runtime.capture_count = 255;
            assert_eq!(capture(engine, &mut state, same, &mut device, &mut registrar), 0);
            assert_eq!(state.runtime.capture_count, 0);
        }
    }
}
#[test]
fn rectangle_masks_selector_spills_and_invalid_items_are_observable() {
    let mut state = EngineState::zero();
    state.saved.level_selectors = RegisterWord(0x80000000);
    let packet = RectanglePacket {
        coordinates: [0x1234,0x2345,0x3456,0x4567],
        level: DefaultSelector(4), color: DefaultSelector(4), opacity: DefaultSelector(2),
        ..RectanglePacket::default()
    };
    assert_eq!(pack_rectangle(&mut state, RectangleGroup::Ten, 9, &packet), 0);
    assert_eq!(state.saved.level_selectors, RegisterWord(0x80100000));
    assert_eq!(state.saved.color_selectors, RegisterWord(0x00100000));
    assert_eq!(state.saved.opacity_selectors, RegisterWord(0x400));
    assert_eq!(state.input.ten_rectangles[9].horizontal, RegisterWord(0x02340345));
    let before = state.clone();
    assert_eq!(pack_rectangle(&mut state, RectangleGroup::Ten, 10, &packet), -30);
    assert_eq!(pack_rectangle(&mut state, RectangleGroup::Two, 2, &packet), -30);
    assert_eq!(state, before);
}
#[test]
fn defaults_pack_clamps_opacity_and_preserves_reserved_register_bits() {
    let mut state = EngineState::zero();
    state.saved.default_opacities = RegisterWord(0xabcdffff);
    state.saved.default_levels = [RegisterWord(0xf800f800); 2];
    pack_defaults(&mut state, &PackedDefaults {
        levels: [RegisterWord(0x1234),RegisterWord(0xffff),RegisterWord(0),RegisterWord(0x800)],
        colors: [ColorWord(1),ColorWord(2),ColorWord(3),ColorWord(u32::MAX)],
        opacities: [Opacity(129),Opacity(127)],
    });
    assert_eq!(state.saved.default_opacities, RegisterWord(0xabcd7f80));
    assert_eq!(state.saved.default_levels, [RegisterWord(0xfffffa34),RegisterWord(0xf800f800)]);
    assert_eq!(state.saved.default_colors, [RegisterWord(1),RegisterWord(2),RegisterWord(3),RegisterWord(u32::MAX)]);
}
