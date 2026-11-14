#![forbid(unsafe_code)]

use xm1_saclac_calc::exposure::ports::Placement;
use xm1_saclac_controllers::{children::{ChildEvent, ChildOutcome, ChildRequest, Children, LensControl, ShotControl, ShotEvent as ChildShot, ShotTarget}, root::{Control, Input, Outcome}, s1::{CaptureEvent, Effect, LensEvent, LiveEvent, Port, Published, ShotEvent, StatisticsEvent}, live_view::Signal};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{stock::{command_code, parameters}, Calc, Calc1, CommandKind, CompletionStatus, ImageProcessor, ImgDc, LensDc, Message, RecordHandle, Statistics};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{stock::shared, CaptureBuffer, Memory, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::{settings::{self, AdjustmentKey, Adjustments}, stock::Stock};
use xm1_shot_sequence::state::StateMemory;
use crate::{command_source::CommandSource, controller_layers::Layers, controller_profiles::Profiles, controller_replies::{autofocus_reply, parameter_word}, statistics_results::Delivery};

pub struct S1Ports<'s, 'entry, M, R, E, P, I, T, S, X, A, C, D> {
    pub profiles: &'entry Profiles,
    pub layers: &'entry mut Layers<'s, M, R, E, P, I, T, S, X, A, C, D>,
}
fn outcome(child: ChildOutcome) -> Outcome {
    match child {
        ChildOutcome::Complete => Outcome::Complete,
        ChildOutcome::Pending => Outcome::Pending,
        ChildOutcome::Inactive => Outcome::Inactive,
        ChildOutcome::Fault | ChildOutcome::Measurement(_) | ChildOutcome::Search(_) => Outcome::Fault,
    }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
    I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
    S1Ports<'_, '_, M, R, E, P, I, T, S, X, A, C, D> {
    fn machine(&self) -> M { self.layers.services.borrow().machine.clone() }
    fn shot_input(&mut self, target: ShotTarget, input: Input<'_>, record: RecordHandle) -> Outcome {
        let event = match input {
            Input::Message(Message::Command(command)) => ChildShot::Command { target, command, record },
            Input::Message(Message::Completed(completed)) => ChildShot::Completed { target, completed, record },
            _ => { self.fault(); return Outcome::Fault; }
        };
        outcome(self.layers.children.handle(ChildRequest::Shot(event)))
    }
    fn lens_input(&mut self, input: Input<'_>, record: RecordHandle) -> Outcome {
        let event = match input {
            Input::Message(Message::Command(command)) => ChildEvent::Command { command, record },
            Input::Message(Message::Completed(completed)) => ChildEvent::Completed { completed, record },
            Input::Control(Control::Cancel) => ChildEvent::Control { control: LensControl::CancelDrive, parameters: parameters(&[]), record },
            _ => { self.fault(); return Outcome::Fault; }
        };
        outcome(self.layers.children.handle(ChildRequest::LensS1(event)))
    }
    fn live_input(&mut self, input: Input<'_>, record: RecordHandle) -> Outcome {
        let signal = match input {
            Input::Message(Message::Command(command)) => Signal::Command(command),
            Input::Message(Message::Completed(done)) => Signal::Completed(done),
            Input::Control(Control::Cancel) => Signal::Cancel,
            Input::Control(Control::SuspendLive) => Signal::SuspendLive,
            _ => { self.fault(); return Outcome::Fault; }
        };
        let previous = core::mem::replace(&mut self.layers.extent, crate::controller_services::WindowExtent::Live);
        let outcome = self.profiles.view(signal, record, self.layers);
        self.layers.extent = previous;
        outcome
    }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
    I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
    Port for S1Ports<'_, '_, M, R, E, P, I, T, S, X, A, C, D> {
    fn shot(&mut self, event: ShotEvent<'_>, record: RecordHandle) -> Outcome {
        let control = match event {
            ShotEvent::Begin => ShotControl::StartS1,
            ShotEvent::Release => ShotControl::AdvanceS1,
            ShotEvent::Forward(input) => return self.shot_input(ShotTarget::HalfPress, input, record),
        };
        outcome(self.layers.children.handle(ChildRequest::Shot(ChildShot::Control { control, record })))
    }
    fn statistics(&mut self, event: StatisticsEvent<'_>, record: RecordHandle) -> Outcome { self.profiles.s1_statistics(event, record, self.layers) }
    fn capture(&mut self, event: CaptureEvent<'_>, record: RecordHandle) -> Outcome {
        match event {
            CaptureEvent::Begin => outcome(self.layers.children.handle(ChildRequest::Shot(ChildShot::Control { control: ShotControl::InitializeCapture, record }))),
            CaptureEvent::Forward(input) => self.shot_input(ShotTarget::Capture, input, record),
        }
    }
    fn lens(&mut self, event: LensEvent<'_>, record: RecordHandle) -> Outcome {
        match event {
            LensEvent::Forward(input) => self.lens_input(input, record),
            LensEvent::Reissue(kind) => self.machine().with_command(kind, |command| self.lens_input(Input::Message(Message::Command(command)), record)),
        }
    }
    fn live(&mut self, event: LiveEvent<'_>, record: RecordHandle) -> Outcome {
        match event {
            LiveEvent::Forward(input) => self.live_input(input, record),
            LiveEvent::Reissue(kind) => self.machine().with_command(kind, |command| self.live_input(Input::Message(Message::Command(command)), record)),
        }
    }
    fn publish(&mut self, published: Published) {
        let mut machine = self.machine();
        match published {
            Published::Waiting(waiting) => {
                let retained = machine.read_u8(crate::stock::S1_PROGRESS) & 0xf0;
                machine.write_u8(crate::stock::S1_PROGRESS, retained | xm1_saclac_controllers::stock::s1_progress(waiting));
            }
            Published::Deferred(count) => machine.write_u8(crate::stock::S1_PROGRESS + 1, count),
            Published::LensStopRequested(on) => machine.write_u8(crate::stock::S1_PROGRESS + 2, u8::from(on)),
            Published::ShotIdle(idle) => self.profiles.statistics.borrow_mut().set_shot_idle(idle),
        }
    }
    fn effect(&mut self, effect: Effect, record: RecordHandle) {
        let mut machine = self.machine();
        match effect {
            Effect::ResetShotProgress => machine.write_u32(xm1_shot_sequence::stock::pipeline::RESTART_FLAG, 0),
            Effect::ApplyHalfPressOption => {
                // f1550c..20 dereferences the client's slot before testing its frame word.
                let slot = parameter_word(&machine, CommandKind::HalfPress, 11);
                if let Some(buffer) = CaptureBuffer::new(machine.read_u32(slot)) {
                    let _ = self.layers.services.borrow_mut().records.write_shared(record, &shared::group_20::CAPTURE_BUFFER, Some(buffer));
                }
            }
            Effect::ResetCaptureRecord => { let _ = self.layers.services.borrow_mut().records.write_shared(record, &shared::group_38::AT_0894, 0); }
            Effect::BeginBusy => { machine.enter(0x74_046c, &[3]); }
            Effect::EndBusy => { machine.enter(0x74_0404, &[3]); }
            Effect::NotifyBridge(kind) => { machine.enter(0x74_2a6c, &[usize::from(command_code(kind)), 0]); }
            Effect::ReplyAutofocus => autofocus_reply(machine, CompletionStatus(0)),
        }
    }
    fn second_shot(&mut self, record: RecordHandle) -> bool { self.layers.services.borrow_mut().entries.header_byte(record, SharedHeaderByte::At63) != 0 }
    fn lens_cancellable(&mut self, record: RecordHandle) -> bool {
        let stock = Stock::new(self.machine());
        settings::unselected_mode(record, &stock, &mut *self.layers.services.borrow_mut())
    }
    fn release_allowed(&mut self) -> bool {
        let machine = self.machine();
        Stock::new(machine.clone()).byte(AdjustmentKey(0xe0)) & 1 == 0
            && machine.enter(0x74_2054, &[]) == 0
            && parameter_word(&machine, CommandKind::HalfPress, 2) != 0
            && parameter_word(&machine, CommandKind::HalfPress, 1) != 6
    }
    fn fault(&mut self) { self.machine().enter(crate::stock::FAULT, &[]); }
}
