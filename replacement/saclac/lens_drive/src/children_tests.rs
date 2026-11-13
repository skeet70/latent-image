use crate::children::LensChildren;
use crate::ports::{DrivePolicy, Reply, Status};
use crate::test_ports::{completed, record, Fake, Seen, AUTOFOCUS_MODE};
use std::vec;
use xm1_saclac_controllers::children::{ChildEvent, ChildOutcome, LensControl, LensSetting};
use xm1_saclac_interface::lens::{DriveOwner, LensPayload, Setting};
use xm1_saclac_interface::{stock, Command, CommandKind as K, Completion as C, Message, ParameterValue as V};

fn control(control: LensControl) -> ChildEvent<'static, LensControl> {
    ChildEvent::Control { control, parameters: stock::parameters(&[]), record: record() }
}

fn command(kind: K) -> ChildEvent<'static, LensControl> {
    ChildEvent::Command { command: Command { kind, parameters: stock::parameters(&[]) }, record: record() }
}

fn done(completion: C) -> ChildEvent<'static, LensControl> {
    ChildEvent::Completed { completed: completed(completion, &[]), record: record() }
}

#[test]
fn the_live_child_takes_controls_commands_and_completions() {
    let mut children = LensChildren::new();
    let mut ports = Fake { policy: DrivePolicy(8), shared_bit: true, ..Fake::default() };
    assert_eq!(children.live(control(LensControl::PrepareDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(command(K::LensDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(command(K::Command11), &mut ports), ChildOutcome::Inactive);
    assert_eq!(children.live(done(C::Lens37), &mut ports), ChildOutcome::Complete);
    assert_eq!(children.live(control(LensControl::CancelDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(control(LensControl::Setting(LensSetting::Request06)), &mut ports), ChildOutcome::Fault);
    let sends: vec::Vec<_> = ports.take().into_iter().filter(|seen| matches!(seen, Seen::Send(_))).collect();
    assert_eq!(sends, vec![
        Seen::Send(LensPayload::Drive { owner: DriveOwner::Live, parameters: [V(11), V(12), V(13), V(1)], slot: ports.slot }),
        Seen::Send(LensPayload::Drive { owner: DriveOwner::Live, parameters: [V(11), V(12), V(13), V(14)], slot: ports.slot }),
    ]);
    children.live(done(C::Lens34), &mut ports);
    ports.take();
    assert_eq!(children.live(control(LensControl::Setting(LensSetting::Request06)), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.live(control(LensControl::Step), &mut ports), ChildOutcome::Fault);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Setting { setting: Setting::Request06, slot: ports.slot, mode: AUTOFOCUS_MODE }),
        Seen::Fault,
    ]);
}

#[test]
fn the_s1_child_has_no_entry_for_the_live_controls() {
    let mut children = LensChildren::new();
    let mut ports = Fake::default();
    assert_eq!(children.s1(control(LensControl::PrepareDrive), &mut ports), ChildOutcome::Inactive);
    assert_eq!(children.s1(command(K::LensDrive), &mut ports), ChildOutcome::Inactive);
    assert_eq!(children.s1(control(LensControl::CancelDrive), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.s1(command(K::Cancel), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.s1(command(K::Command10), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.s1(done(C::Lens34), &mut ports), ChildOutcome::Complete);
    assert_eq!(ports.take()[1..4], [
        Seen::Send(LensPayload::Finish { value: V(0x77), record: record() }),
        Seen::Reply(Reply::Cancel(Status::Accepted)),
        Seen::Send(LensPayload::Halt),
    ]);
}

#[test]
fn the_bring_up_child_takes_the_root_messages_with_the_record() {
    let mut children = LensChildren::new();
    let mut ports = Fake::default();
    let bring_up = Message::Command(Command { kind: K::LensBringUp, parameters: stock::parameters(&[]) });
    assert_eq!(children.bring_up(bring_up, record(), &mut ports), ChildOutcome::Pending);
    assert_eq!(children.bring_up(Message::Completed(completed(C::Lens2f, &[])), record(), &mut ports), ChildOutcome::Complete);
    let drive = Message::Command(Command { kind: K::LensDrive, parameters: stock::parameters(&[]) });
    assert_eq!(children.bring_up(drive, record(), &mut ports), ChildOutcome::Inactive);
    assert_eq!(ports.take()[0], Seen::Send(LensPayload::BringUp { parameters: [1, 2, 3, 4, 5].map(V), record: record() }));
}
