#![forbid(unsafe_code)]

use xm1_saclac_calc::exposure::ports::Placement;
use xm1_saclac_controllers::{children::{ChildEvent, ChildOutcome, ChildRequest, Children, LensControl}, live::{ControllerEvent, DriveOrigin, Effect, LensEvent, Port, Profile}, live_replies, live_view::Signal, root::{Control, Input, Outcome}, s1_autofocus};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{stock::{parameters, record_handle_from_a2, StockCallbackWord}, Calc, Calc1, CommandKind, Completed, Completion, CompletionStatus, ImageProcessor, ImgDc, LensDc, Message, RecordHandle, Statistics};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{stock::{record, shared}, ChangeSet, Memory, MeasurementSelection, SharedHeaderByte, StockEntries};
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::Stock};
use xm1_shot_sequence::state::StateMemory;
use crate::{command_source::CommandSource, controller_layers::Layers, controller_profiles::Profiles, controller_replies::{autofocus_reply, parameter_word}, statistics_results::Delivery};

#[derive(Clone, Copy)]
pub struct Callbacks { pub lens_corrected: StockCallbackWord }

pub struct LivePorts<'s, 'entry, M, R, E, P, I, T, S, X, A, C, D> {
    pub profiles: &'entry Profiles,
    pub layers: &'entry mut Layers<'s, M, R, E, P, I, T, S, X, A, C, D>,
    pub callbacks: Callbacks,
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
    LivePorts<'_, '_, M, R, E, P, I, T, S, X, A, C, D> {
    fn machine(&self) -> M { self.layers.services.borrow().machine.clone() }
    fn view(&mut self, signal: Signal<'_>, record: RecordHandle) -> Outcome { self.profiles.view(signal, record, self.layers) }
    fn forward_view(&mut self, input: Input<'_>, record: RecordHandle) -> Outcome {
        match input {
            Input::Message(Message::Command(command)) => self.view(Signal::Command(command), record),
            Input::Message(Message::Completed(done)) => self.view(Signal::Completed(done), record),
            Input::Control(Control::ResumeLive) => self.view(Signal::ResumeLive, record),
            Input::Control(Control::SuspendLive) => self.view(Signal::SuspendLive, record),
            Input::Control(Control::Cancel) => self.view(Signal::Cancel, record),
            _ => { self.fault(); Outcome::Fault }
        }
    }
    fn forward_lens(&mut self, input: Input<'_>, record: RecordHandle) -> Outcome {
        let event = match input {
            Input::Message(Message::Command(command)) => ChildEvent::Command { command, record },
            Input::Message(Message::Completed(completed)) => ChildEvent::Completed { completed, record },
            Input::Control(Control::Cancel) => ChildEvent::Control { control: LensControl::CancelDrive, parameters: parameters(&[]), record },
            _ => { self.fault(); return Outcome::Fault; }
        };
        outcome(self.layers.children.handle(ChildRequest::LensLive(event)))
    }
}
impl<M: CommandSource + Clone, R: Memory, E: StockEntries,
    P: ImageProcessor + ImgDc + Statistics + LensDc + Calc + Calc1,
    I: Isp, T: Tables, S: StateMemory, X, A: Placement, C: Children, D: Delivery>
    Port for LivePorts<'_, '_, M, R, E, P, I, T, S, X, A, C, D> {
    fn controller(&mut self, event: ControllerEvent<'_>, record: RecordHandle) -> Outcome {
        match event {
            ControllerEvent::Forward(input) => self.forward_view(input, record),
            ControllerEvent::Reissue(kind) => self.machine().with_command(kind, |command| self.view(Signal::Command(command), record)),
            ControllerEvent::Suspend => self.view(Signal::Suspend, record),
            ControllerEvent::Resume => self.view(Signal::Resume, record),
        }
    }
    fn lens(&mut self, event: LensEvent<'_>, record: RecordHandle) -> Outcome {
        match event {
            LensEvent::Forward(input) => self.forward_lens(input, record),
            LensEvent::Reissue(kind) => self.machine().with_command(kind, |command| self.forward_lens(Input::Message(Message::Command(command)), record)),
            LensEvent::Prepare => outcome(self.layers.children.handle(ChildRequest::LensLive(ChildEvent::Control { control: LensControl::PrepareDrive, parameters: parameters(&[]), record }))),
            // The drive receipt was recorded at intake; the deferred leaf only reads its kind.
            LensEvent::DriveStopped => outcome(self.layers.children.handle(ChildRequest::LensLive(ChildEvent::Completed { completed: Completed { completion: Completion::Lens34, parameters: parameters(&[]) }, record }))),
        }
    }
    fn lens_follows(&mut self, profile: Profile, record: RecordHandle) -> bool {
        if self.layers.services.borrow_mut().entries.header_byte(record, SharedHeaderByte::At63) != 0 { return false; }
        let (kind, index) = match profile { Profile::Primary => (CommandKind::Command02, 1), Profile::Secondary => (CommandKind::Command03, 2) };
        parameter_word(&self.machine(), kind, index) < 2
    }
    fn effect(&mut self, effect: Effect<'_>, handle: RecordHandle) {
        let mut machine = self.machine();
        match effect {
            Effect::ReplyProfile(profile) => live_replies::reply_profile(profile, handle, self.layers),
            Effect::ReplyAutofocus => autofocus_reply(machine, CompletionStatus(0)),
            Effect::PrepareAutofocusLens => {
                let changed = Stock::new(machine.clone()).byte(AdjustmentKey(0x88ef)) & 0xe != 0
                    && self.layers.services.borrow().records.has_changes(handle, ChangeSet::of(record::group_00::MEASUREMENT_SELECTION.change().expect("measurement selection change"))).unwrap_or(false);
                if changed {
                    let resource = machine.enter(0x76_d9b8, &[]);
                    let value = machine.enter(0x74_a740, &[resource as usize, 0]);
                    machine.enter(0x76_f394, &[resource as usize, value as usize, 1]);
                }
            }
            Effect::ReportLensStatus(parameters) => live_replies::report_drive_progress(parameters, self.layers),
            Effect::CaptureLensPosition => {
                let record = record_handle_from_a2(machine.read_u32(crate::stock::FOCUS_CONTROLLER_RECORD));
                let mut target = {
                    let mut services = self.layers.services.borrow_mut();
                    let mut options = services.shared_bytes(record, &shared::group_19::AT_0120);
                    options[24..28].fill(0);
                    let _ = services.records.write_shared(record, &shared::group_19::AT_0120, options);
                    services.shared_bytes(record, &shared::group_26::AT_0240)
                };
                target[..2].copy_from_slice(&(crate::calibration_inputs::lens_position(&machine) as u16).to_le_bytes());
                let _ = self.layers.services.borrow_mut().records.write_shared(record, &shared::group_26::AT_0240, target);
            }
            Effect::RecordDriveOrigin(origin) => machine.write_u32(crate::stock::LIVE_DRIVE_ORIGIN, u32::from(origin == DriveOrigin::Cancel)),
            Effect::RecordDriveResult(parameters) => machine.write_u32(crate::stock::LIVE_DRIVE_RESULT, u32::from_le_bytes(parameters.bytes()[4..8].try_into().expect("drive result word"))),
        }
    }
    fn drive_origin(&mut self) -> DriveOrigin {
        if self.machine().read_u32(crate::stock::LIVE_DRIVE_ORIGIN) == 1 { DriveOrigin::Cancel } else { DriveOrigin::Command }
    }
    fn drive_check_applies(&mut self, record: RecordHandle) -> bool {
        if self.drive_origin() != DriveOrigin::Cancel { return false; }
        let mut services = self.layers.services.borrow_mut();
        services.entries.measurement_selection(record) == MeasurementSelection::Unselected
            && !matches!(services.entries.header_byte(record, SharedHeaderByte::At31), 46 | 54 | 47 | 56)
    }
    fn correct_drive(&mut self, record: RecordHandle) -> Outcome {
        let base = self.machine().enter(0x76_d9c8, &[]);
        if s1_autofocus::correct_drive(record, base, self.callbacks.lens_corrected, self.layers) { Outcome::Complete } else { Outcome::Pending }
    }
    fn fault(&mut self) { self.machine().enter(crate::stock::FAULT, &[]); }
}
