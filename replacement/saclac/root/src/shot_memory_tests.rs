use core::cell::RefCell;
use xm1_saclac_calc::exposure::context::ExposureContext;
use xm1_saclac_interface::exposure::CalculationInput;
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::calculation_work::{CalculationContext, ExposureWord};
use xm1_shot_sequence::grid_boundary::{GridSetting, GridSettings};
use xm1_shot_sequence::state::{ShotState, StateMemory};
use xm1_shot_sequence::stock::{calculation, state as layout};
use crate::fake::Fake;
use crate::stock::shot::ShotMemory;

struct Settings;
impl GridSettings for Settings {
    fn setting(&mut self, _: GridSetting) -> i32 { unreachable!() }
    fn lens_threshold(&mut self, _: u8) -> i16 { unreachable!() }
    fn lens_band(&mut self, _: i16) -> u32 { unreachable!() }
    fn reference_level(&mut self, _: u32, _: u32) -> i16 { unreachable!() }
    fn reference_offset(&mut self, reference: u32) -> i16 { reference as i16 }
}

#[test]
fn calculation_reads_and_clears_the_same_exposure_as_the_engine() {
    let machine = Fake::default();
    machine.set_word(calculation::EXPOSURE_CONTEXT + 0x30, 99);
    machine.set_word(calculation::EXPOSURE_CONTEXT + 0x1268, 99);
    let context = RefCell::new(ExposureContext::default());
    let mut state = ShotState::new(ShotMemory(machine.clone(), &context));
    assert_eq!(state.exposure(ExposureWord::Evaluation), 0);
    assert_eq!(state.exposure(ExposureWord::Repeat), 0);
    {
        let mut context = context.borrow_mut();
        context.levels.unmeasured = true;
        context.outcome.focus_reused = true;
        context.levels.curve = 0x1234;
    }
    machine.set_word(calculation::INPUT + 0x3c, u32::from_le_bytes([7, 9, 6, 8]));
    assert_eq!(state.exposure(ExposureWord::Evaluation), 1);
    assert_eq!(state.exposure(ExposureWord::Repeat), 1);
    assert_eq!(state.exposure_target_word(), 0x1234);
    assert_eq!((state.exposure_columns(), state.exposure_rows()), (7, 9));
    <_ as CalculationContext<Settings>>::clear_repeat(&mut state);
    assert!(!context.borrow().outcome.focus_reused);
    assert!(context.borrow().levels.unmeasured);
}

#[test]
fn resumed_input_rebases_history_into_the_same_buffer_the_calc_actor_reads() {
    let context = RefCell::new(ExposureContext::default());
    let mut machine = Fake::default();
    crate::shot_input::store(&mut machine, &CalculationInput {
        lens_reference: 30, record_lens_reference: 30, metering_level: 32760,
        levels: [32760, -32760, 3, 4, 5, 6, 7, 8, 9], target_word: 0xfeed_beef,
        target_half: 0x9876, selection: true, refinement_pass: true, record_26: 0xabcd,
        record_33: 0x76, selection_flags: 0x8123, plan_exposure: [1, 2, 3, 4, 5],
        offset: -123, ..CalculationInput::default()
    });
    let mut state = ShotState::new(ShotMemory(machine.clone(), &context));
    <_ as CalculationContext<Settings>>::save_input(&mut state);
    state.memory().write(layout::INPUT, &[0; calculation::INPUT_BYTES]);
    <_ as CalculationContext<Settings>>::resume_input(&mut state, 20, &mut Settings);
    let word = |offset| machine.read_u32(calculation::INPUT + offset);
    let half = |offset| machine.read_u16(calculation::INPUT + offset);
    assert_eq!((word(8), word(0x100)), (20, 20));
    assert_eq!(half(0x38) as i16, -32766);
    assert_eq!(core::array::from_fn::<_, 9, _>(|index| half(0x104 + index as u32 * 2) as i16), [-32766, -32750, 13, 14, 15, 16, 17, 18, 19]);
    assert_eq!((word(0xc), half(0x10)), (0xfeed_beef, 0x9876));
    assert_eq!((word(4), word(0x30), word(0x128)), (4, 1, 1));
    assert_eq!((word(0x60), half(0x58)), (0xabcd_0076, 0x8123));
    assert_eq!(core::array::from_fn::<_, 5, _>(|index| half(0x44 + index as u32 * 2)), [1, 2, 3, 4, 5]);
    assert_eq!(half(0x80) as i16, -123);
    assert_eq!(state.saved_input().reference(), 30);
    state.memory().write(layout::INPUT_TARGET_HALF, &0x1357u16.to_le_bytes());
    assert_eq!(machine.read_u16(calculation::INPUT + 0x10), 0x1357);
    assert_eq!(machine.read_u32(calculation::INPUT + 0xc), 0xfeed_beef);
    machine.set_word(calculation::INPUT + 0x3c, u32::from_le_bytes([5, 8, 4, 7]));
    assert_eq!((state.exposure_columns(), state.exposure_rows()), (5, 8));
}

fn pattern(machine: &Fake, at: u32, bytes: u32) {
    for offset in 0..bytes { machine.set_byte(at + offset, (offset * 7 + 3) as u8); }
}

fn bytes(machine: &Fake, at: u32, count: u32) -> Vec<u8> { (0..count).map(|offset| machine.byte(at + offset)).collect() }

/// The bytes an ascending one-byte-at-a-time copy leaves at the destination.
fn ascending_copy(machine: &Fake, to: u32, from: u32, count: u32) -> Vec<u8> {
    let mut memory: std::collections::BTreeMap<u32, u8> = (0..count).map(|offset| (from + offset, machine.byte(from + offset))).collect();
    for offset in 0..count {
        let byte = memory.get(&(from + offset)).copied().unwrap_or_else(|| machine.byte(from + offset));
        memory.insert(to + offset, byte);
    }
    (0..count).map(|offset| memory[&(to + offset)]).collect()
}

#[test]
fn copy_between_plain_words_moves_every_byte() {
    let machine = Fake::default();
    let context = RefCell::new(ExposureContext::default());
    let mut memory = ShotMemory(machine.clone(), &context);
    pattern(&machine, layout::INPUT.get(), calculation::INPUT_BYTES as u32);
    let expected = bytes(&machine, layout::INPUT.get(), calculation::INPUT_BYTES as u32);
    memory.copy(layout::SAVED_INPUT, layout::INPUT, calculation::INPUT_BYTES);
    assert_eq!(bytes(&machine, layout::SAVED_INPUT.get(), calculation::INPUT_BYTES as u32), expected);
    memory.copy(layout::RETURN_ACTIVE, layout::SAVED_INPUT, 7);
    let mut tail = expected[..7].to_vec();
    tail.push(0);
    assert_eq!(bytes(&machine, layout::RETURN_ACTIVE.get(), 8), tail);
}

#[test]
fn copy_reads_and_writes_mapped_context_words() {
    let machine = Fake::default();
    let context = RefCell::new(ExposureContext::default());
    context.borrow_mut().levels.curve = 0x1234_5678;
    context.borrow_mut().levels.unmeasured = true;
    let mut memory = ShotMemory(machine.clone(), &context);
    memory.copy(layout::SAVED_INPUT, layout::EXPOSURE_TARGET_WORD, 8);
    assert_eq!((machine.word(layout::SAVED_INPUT.get()), machine.word(layout::SAVED_INPUT.get() + 4)), (0x1234_5678, 1));
    machine.set_word(layout::SAVED_INPUT.get(), 0x0bad_cafe);
    machine.set_word(layout::SAVED_INPUT.get() + 4, 0);
    memory.copy(layout::EXPOSURE_TARGET_WORD, layout::SAVED_INPUT, 8);
    assert_eq!((context.borrow().levels.curve, context.borrow().levels.unmeasured), (0x0bad_cafe, false));
    assert_eq!(machine.word(layout::EXPOSURE_TARGET_WORD.get()), 0);
}

#[test]
fn overlapping_and_unaligned_copies_keep_ascending_byte_order() {
    let machine = Fake::default();
    let context = RefCell::new(ExposureContext::default());
    let mut memory = ShotMemory(machine.clone(), &context);
    let input = layout::INPUT.get();
    for (to, from, count) in [(layout::EXPOSURE_COLUMNS, layout::INPUT, 0x40), (layout::INPUT, layout::EXPOSURE_COLUMNS, 0x40), (layout::SAVED_INPUT, layout::EXPOSURE_ROWS, 5)] {
        pattern(&machine, input, calculation::INPUT_BYTES as u32);
        let expected = ascending_copy(&machine, to.get(), from.get(), count);
        memory.copy(to, from, count as usize);
        assert_eq!(bytes(&machine, to.get(), count), expected, "{:#x} <- {:#x}", to.get(), from.get());
    }
}
