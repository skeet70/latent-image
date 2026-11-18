#![forbid(unsafe_code)]

use core::cell::RefCell;
use xm1_saclac_calc::exposure::ports::Placement;
use xm1_saclac_controllers::{children::{ChildEvent, ChildOutcome, ChildRequest, Children, MeasurementControl, ShotEvent}, live_statistics::LiveStatistics, live_statistics_profile::Leaf, live_view::{Application, Condition, Effect, MotionSetting, Port, Preparation, ReplyStatus, StatisticsCall, StatisticsOutcome, ViewRequest}, root::Outcome};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{Calc, Calc1, Command, CommandKind, ImageProcessor, ImgDc, LensDc, RecordHandle, Statistics};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::state::StateMemory;
use crate::{controller_layers::Layers, statistics_results::Delivery};

/// A view entry borrows the persistent profile, statistics and children independently.
pub struct View<'a, 'entry, M, R, E, P, I, T, S, X, A, C, D> {
    pub layers: &'entry mut Layers<'a, M, R, E, P, I, T, S, X, A, C, D>,
    pub statistics: &'entry RefCell<LiveStatistics>,
}

impl<M: Machine + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
    I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
    Port for View<'_, '_, M, R, E, P, I, T, S, X, A, C, D> {
    fn statistics(&mut self, call: StatisticsCall<'_>, record: RecordHandle) -> StatisticsOutcome {
        self.statistics.borrow_mut().send(call, record, &mut Leaf(&mut *self.layers))
    }
    fn measurement(&mut self, event: ChildEvent<'_, MeasurementControl>) -> ChildOutcome {
        self.layers.children.handle(ChildRequest::Measurement(event))
    }
    fn sequence(&mut self, event: ShotEvent<'_>) -> Outcome {
        match self.layers.children.handle(ChildRequest::Shot(event)) {
            ChildOutcome::Complete => Outcome::Complete,
            ChildOutcome::Pending => Outcome::Pending,
            ChildOutcome::Inactive => Outcome::Inactive,
            ChildOutcome::Fault | ChildOutcome::Measurement(_) | ChildOutcome::Search(_) => Outcome::Fault,
        }
    }
    fn prepare(&mut self, command: Option<Command<'_>>, record: RecordHandle) -> Preparation {
        let profile = self.layers.live_profile;
        profile.borrow_mut().prepare(command, record, self.layers)
    }
    fn apply(&mut self, command: Command<'_>, record: RecordHandle) -> Application {
        let profile = self.layers.live_profile;
        profile.borrow_mut().apply(command, record, self.layers)
    }
    fn view(&mut self, request: ViewRequest, record: RecordHandle) {
        let profile = self.layers.live_profile;
        profile.borrow_mut().view(request, record, self.layers);
    }
    fn effect(&mut self, effect: Effect, record: RecordHandle) {
        let profile = self.layers.live_profile;
        profile.borrow_mut().effect(effect, record, self.layers);
    }
    fn condition(&mut self, condition: Condition, command: Option<Command<'_>>, record: RecordHandle) -> bool {
        let profile = self.layers.live_profile;
        profile.borrow_mut().condition(condition, command, record, self.layers)
    }
    fn motion_setting(&mut self) -> MotionSetting {
        let profile = self.layers.live_profile;
        profile.borrow_mut().motion_setting(self.layers)
    }
    fn reply(&mut self, command: Option<CommandKind>, status: ReplyStatus, record: RecordHandle) {
        let profile = self.layers.live_profile;
        profile.borrow_mut().reply(command, status, record, self.layers);
    }
    fn fault(&mut self) { self.layers.services.borrow().machine.enter(crate::stock::FAULT, &[]); }
}
