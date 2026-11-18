#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::motion::state::MotionState;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::pipeline_work::{CaptureBlock, LensBlock, PlanBlock, PlanMemory, ScanBlock, ScanStore, SubjectBlock};
use crate::{motion_projection as projection, shot_services::Libraries};

// fa364c's output addresses name views of the one owned MotionState, not another native state.
pub(crate) const SUBJECT: u32 = 0x16d_74d8;
pub(crate) const CAPTURE: u32 = 0x16d_7390;
pub(crate) const LENS: u32 = 0x16d_7d28;
pub(crate) const SCAN: u32 = 0x16d_829c;
pub(crate) const RAW_SELECTION: u32 = 0x16d_8424;
pub(crate) const RAW_SUBJECT: u32 = 0x16d_8430;
pub(crate) const RAW_FIRST: u32 = 0x16d_84a0;
pub(crate) const RAW_RESULT: u32 = 0x16d_84f4;

fn prepared(state: &Option<MotionState>) -> &MotionState { state.as_ref().expect("motion block before preparation") }
fn prepared_mut(state: &mut Option<MotionState>) -> &mut MotionState { state.as_mut().expect("motion block before preparation") }

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q>
    PlanMemory for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn subject(&mut self, block: PlanBlock) -> SubjectBlock {
        let state = self.motion.borrow(); let exposures = &prepared(&state).exposures();
        projection::subject(match block.0 {
            SUBJECT => &exposures.second,
            RAW_SUBJECT => &exposures.first,
            _ => panic!("unknown motion subject address"),
        })
    }
    fn capture(&mut self, block: PlanBlock) -> CaptureBlock {
        assert_eq!(block.0, CAPTURE);
        projection::capture(prepared(&self.motion.borrow()))
    }
    fn lens(&mut self, block: PlanBlock) -> LensBlock {
        assert_eq!(block.0, LENS);
        projection::lens(prepared(&self.motion.borrow()))
    }
    fn scan(&mut self, block: PlanBlock) -> ScanBlock {
        assert_eq!(block.0, SCAN);
        projection::scan(prepared(&self.motion.borrow()).plan())
    }
    fn raw_selection(&mut self, block: PlanBlock) -> u32 {
        assert_eq!(block.0, RAW_SELECTION);
        u32::from(prepared(&self.motion.borrow()).channel_status())
    }
    fn raw_first_word(&mut self, block: PlanBlock) -> u32 {
        assert_eq!(block.0, RAW_FIRST);
        let [height, width] = prepared(&self.motion.borrow()).statistics().cell_size;
        u32::from(height) | u32::from(width) << 16
    }
    fn store_scan(&mut self, block: PlanBlock, store: ScanStore) {
        assert_eq!(block.0, SCAN);
        let mut state = self.motion.borrow_mut(); let plan = prepared_mut(&mut state).plan_mut();
        match store {
            ScanStore::ScanRequested => plan.scan_requested = 1,
            ScanStore::FirstSample(position) => plan.profile.start = position as i16,
            ScanStore::Decision(decision) => plan.decision = decision,
            ScanStore::GradationReached => plan.result.fast = 1,
            ScanStore::Selection(selection) => plan.selection = selection,
            ScanStore::FirstToneLimit(limit) => plan.tone_limits[0] = limit,
            ScanStore::SecondToneLimit(limit) => plan.tone_limits[1] = limit,
            ScanStore::Tone(result) => for (word, bytes) in plan.tone.words.iter_mut().zip(result.0.chunks_exact(4)) { *word = u32::from_le_bytes(bytes.try_into().unwrap()); },
            ScanStore::Gradation(result) => plan.result = projection::focus_result(&result.0),
        }
    }
    fn mark_lens_driven(&mut self, block: PlanBlock) {
        assert_eq!(block.0, LENS);
        prepared_mut(&mut self.motion.borrow_mut()).mark_lens_driven();
    }
    fn store_raw_calculation(&mut self, block: PlanBlock, result: &xm1_saclac_interface::pipeline::RawCalculation) {
        assert_eq!(block.0, RAW_RESULT);
        let mut machine = self.services.borrow().machine.clone();
        for (i, bytes) in result.0.chunks_exact(4).enumerate() { machine.write_u32(RAW_RESULT + 4 * i as u32, u32::from_le_bytes(bytes.try_into().unwrap())); }
    }
}
