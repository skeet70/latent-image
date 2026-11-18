#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::motion::{prepare::Preparation, readings::PreparationInput, state::MotionState}, exposure::ports::Placement};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, ImageProcessor};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{pipeline_work::PlanInput, subjects::Candidates};
use crate::{calculation_services::Calculation, motion_memory::{focus_request, Selection}, shot_services::Libraries};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q>
    Libraries<'_, M, R, E, P, I, T, A, Q> {
    pub fn prepare_motion(&mut self, input: &PlanInput, candidates: &Candidates, selection_reference: u32) -> Preparation {
        let machine = self.services.borrow().machine.clone();
        let frame = SharedAddress(machine.read_u32(input.frame_slot));
        let input = PreparationInput {
            record: input.record, lens: input.lens_reference, load_test: input.load_test_window,
            request: focus_request(&machine, input.option),
        };
        let mut selection = Selection { candidates, reference: selection_reference };
        let (state, prepared) = MotionState::prepare(&mut Calculation {
            services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(),
            placement: &mut *self.placement.borrow_mut(),
        }, &mut selection, &input, frame);
        *self.motion.borrow_mut() = Some(state);
        prepared
    }
    pub fn lay_out_motion(&mut self) {
        self.motion.borrow_mut().as_mut().expect("motion layout before preparation").lay_out_samples(&mut Calculation {
            services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(),
            placement: &mut *self.placement.borrow_mut(),
        });
    }
    pub fn next_motion_plan(&mut self) -> xm1_saclac_calc::adjust::motion::plan::PlanKind {
        use xm1_saclac_calc::adjust::motion::plan::PlanKind;
        let mut motion = self.motion.borrow_mut();
        let state = motion.as_mut().expect("motion planning before preparation");
        let kind = state.plan_next(&mut Calculation {
            services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(),
            placement: &mut *self.placement.borrow_mut(),
        });
        if kind != PlanKind::SETTLED {
            let plan = state.plan_mut();
            plan.selection = u32::from(plan.speed_choice);
        }
        kind
    }
}
