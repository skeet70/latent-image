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
