use crate::children::LensChildren;
use crate::ports::{DriveDetail, DriveFourth, DrivePolicy, DriveReply, Probe, Probes, Reply, Status};
use crate::test_ports::{completed, record, Fake, Seen, AUTOFOCUS_MODE, PRIMARY_MODE, SECONDARY_MODE};
use std::vec;
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, LensControl};
use xm1_saclac_interface::lens::{DriveOwner, LensPayload, LensMode, PositionStep, ProfileSlotReference, SelectionCode, Setting};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completion as C, Message, ParameterValue as V};

const SLOT: ProfileSlotReference = ProfileSlotReference(0x8100_0042);

fn control(control: LensControl) -> ChildEvent<'static, LensControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}

fn command(kind: K) -> ChildEvent<'static, LensControl> {
    ChildEvent::Command { command: Command { kind, parameters: stock::parameters(&[]) }, record: record() }
}

fn done(completion: C) -> ChildEvent<'static, LensControl> {
    ChildEvent::Completed { completed: completed(completion, &[]), record: record() }
}

fn drive(owner: DriveOwner) -> Seen {
    Seen::Send(LensPayload::Drive { owner, parameters: [V(11), V(12), V(13), V(14)], slot: SLOT })
}

fn setting(setting: Setting, mode: LensMode) -> Seen {
    Seen::Send(LensPayload::Setting { setting, slot: SLOT, mode })
}

#[test]
fn every_live_control_reaches_its_child_entry() {
    let select = LensPayload::Select { value: V(0x2c2c), code: SelectionCode::Unconfigured, record: record() };
    for (event, outcome, effects) in [
        (control(LensControl::PrepareDrive), ChildOutcome::Pending, vec![]),
        (control(LensControl::CancelDrive), ChildOutcome::Pending, vec![drive(DriveOwner::Live)]),
        (control(LensControl::Setting(Setting::Request04)), ChildOutcome::Pending, vec![setting(Setting::Request04, AUTOFOCUS_MODE)]),
        (control(LensControl::Setting(Setting::Request06)), ChildOutcome::Pending, vec![setting(Setting::Request06, AUTOFOCUS_MODE)]),
        (control(LensControl::Setting(Setting::Request05)), ChildOutcome::Pending, vec![setting(Setting::Request05, AUTOFOCUS_MODE)]),
        (control(LensControl::Step), ChildOutcome::Pending, vec![Seen::Send(LensPayload::Step(PositionStep(-3)))]),
        (command(K::Command02), ChildOutcome::Pending, vec![setting(Setting::Request05, PRIMARY_MODE)]),
        (command(K::Command03), ChildOutcome::Pending, vec![setting(Setting::Request05, SECONDARY_MODE)]),
        (command(K::LensDrive), ChildOutcome::Pending, vec![drive(DriveOwner::Live)]),
        (command(K::Cancel), ChildOutcome::Complete, vec![Seen::Reply(Reply::Cancel(Status::Unneeded))]),
        (command(K::Command10), ChildOutcome::Complete, vec![Seen::Reply(Reply::Stop(Status::Unneeded))]),
        (command(K::Command2c), ChildOutcome::Pending, vec![Seen::Send(select), Seen::PrepareSelection(record())]),
        (command(K::Command2d), ChildOutcome::Complete, vec![Seen::Reply(Reply::Release(Status::Unneeded))]),
        (command(K::Command11), ChildOutcome::Inactive, vec![]),
        (done(C::Lens33), ChildOutcome::Inactive, vec![]),
    ] {
        let mut children = LensChildren::new();
        let mut ports = Fake::default();
        assert_eq!(children.live(event, &mut ports), outcome, "{event:?}");
        assert_eq!(ports.take(), effects, "{event:?}");
    }
}

#[test]
fn every_s1_control_and_command_reaches_its_child_entry() {
    let report = Seen::Reply(Reply::Drive(DriveReply {
        status: Status::Done,
        value: V(0x750c),
        fourth: DriveFourth::Carried(V(14)),
        detail: DriveDetail::Lens(Probes { bit0: Probe::Clear, bit1: Probe::Clear, bit2: Probe::Clear }),
    }));
    for (event, outcome, effects) in [
        (control(LensControl::PrepareDrive), ChildOutcome::Inactive, vec![]),
        (control(LensControl::CancelDrive), ChildOutcome::Pending, vec![drive(DriveOwner::S1)]),
        (control(LensControl::Setting(Setting::Request06)), ChildOutcome::Inactive, vec![]),
        (control(LensControl::Step), ChildOutcome::Inactive, vec![]),
        (command(K::Cancel), ChildOutcome::Pending, vec![Seen::Reply(Reply::Cancel(Status::Accepted))]),
        (command(K::Command10), ChildOutcome::Complete, vec![Seen::Reply(Reply::Stop(Status::Accepted)), report]),
        (command(K::LensDrive), ChildOutcome::Inactive, vec![]),
        (command(K::Command2c), ChildOutcome::Inactive, vec![]),
    ] {
        let mut children = LensChildren::new();
        let mut ports = Fake::default();
        assert_eq!(children.s1(event, &mut ports), outcome, "{event:?}");
        assert_eq!(ports.take(), effects, "{event:?}");
    }
}

#[test]
fn every_bring_up_message_reaches_its_child_entry() {
    for (kind, outcome, effects) in [
        (K::LensBringUp, ChildOutcome::Pending, vec![Seen::Send(LensPayload::BringUp { parameters: [1, 2, 3, 4, 5].map(V), record: record() })]),
        (K::LensStop, ChildOutcome::Pending, vec![Seen::Send(LensPayload::Stop { parameters: [6, 7].map(V), record: record() })]),
        (K::Command10, ChildOutcome::Inactive, vec![]),
    ] {
        let mut children = LensChildren::new();
        let mut ports = Fake::default();
        let message = Message::Command(Command { kind, parameters: stock::parameters(&[]) });
        assert_eq!(children.bring_up(message, record(), &mut ports), outcome, "{kind:?}");
        assert_eq!(ports.take(), effects, "{kind:?}");
    }
}

#[test]
fn the_live_child_takes_controls_commands_and_completions() {
    let mut children = LensChildren::new();
    let mut ports = Fake { policy: DrivePolicy { always: true, resource_one: false, resource_zero: false }, shared_bit: true, ..Fake::default() };
    assert_eq!(children.live(control(LensControl::PrepareDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(command(K::LensDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(command(K::Command11), &mut ports), ChildOutcome::Inactive);
    assert_eq!(children.live(done(C::Lens37), &mut ports), ChildOutcome::Complete);
    assert_eq!(children.live(control(LensControl::CancelDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(control(LensControl::Setting(Setting::Request06)), &mut ports), ChildOutcome::Fault);
    let sends: vec::Vec<_> = ports.take().into_iter().filter(|seen| matches!(seen, Seen::Send(_))).collect();
    assert_eq!(sends, vec![
        Seen::Send(LensPayload::Drive { owner: DriveOwner::Live, parameters: [V(11), V(12), V(13), V(1)], slot: ports.slot }),
        Seen::Send(LensPayload::Drive { owner: DriveOwner::Live, parameters: [V(11), V(12), V(13), V(14)], slot: ports.slot }),
    ]);
    children.live(done(C::Lens34), &mut ports);
    ports.take();
    assert_eq!(children.live(control(LensControl::Setting(Setting::Request06)), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(control(LensControl::Step), &mut ports), ChildOutcome::Fault);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Setting { setting: Setting::Request06, slot: ports.slot, mode: AUTOFOCUS_MODE }),
        Seen::Fault,
    ]);
}
