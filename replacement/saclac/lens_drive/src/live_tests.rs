use crate::live::{DriveOrigin, Event, LiveLens, Profile};
use crate::ports::{DriveDetail, DriveFourth, DriveReply, DrivePolicy, Probe, Probes, Reply, ResourceSelection, Status};
use crate::test_ports::{completed, record, Fake, Seen, AUTOFOCUS_MODE, PRIMARY_MODE, SECONDARY_MODE};
use crate::Outcome::{self, Complete, Fault, Inactive, Pending};
use std::vec;
use xm1_saclac_interface::lens::{DriveOwner, LensMode, LensPayload, SelectionCode, Setting};
use xm1_saclac_interface::{Completion as C, ParameterValue as V};

fn run(lens: &mut LiveLens, ports: &mut Fake, event: Event<'_>) -> Outcome { lens.handle(event, record(), ports) }

fn done(completion: C) -> Event<'static> { Event::Completed(completed(completion, &[])) }

fn setting(setting: Setting, mode: LensMode, ports: &Fake) -> Seen {
    Seen::Send(LensPayload::Setting { setting, slot: ports.slot, mode })
}

fn drive_sent(ports: &Fake, parameters: [V; 4]) -> Seen {
    Seen::Send(LensPayload::Drive { owner: DriveOwner::Live, parameters, slot: ports.slot })
}

fn policy(always: bool, resource_one: bool, resource_zero: bool) -> DrivePolicy {
    DrivePolicy { always, resource_one, resource_zero }
}

fn correcting() -> Fake {
    Fake { policy: policy(true, false, false), shared_bit: true, ..Fake::default() }
}

#[test]
fn a_profile_sends_request05_with_its_mode_and_ends_on_lens36() {
    for (profile, mode) in [(Profile::Primary, PRIMARY_MODE), (Profile::Secondary, SECONDARY_MODE)] {
        let mut lens = LiveLens::new();
        let mut ports = Fake::default();
        assert_eq!(run(&mut lens, &mut ports, Event::Profile(profile)), Pending);
        assert_eq!(ports.take(), vec![setting(Setting::Request05, mode, &ports)]);
        assert_eq!(run(&mut lens, &mut ports, done(C::Lens36)), Complete);
        assert_eq!(run(&mut lens, &mut ports, done(C::Lens36)), Fault);
        assert_eq!(ports.take(), vec![Seen::Fault]);
    }
}

#[test]
fn owed_settings_follow_in_order_04_06_05() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request04)), Pending);
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request06)), Pending);
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request05)), Pending);
    assert_eq!(ports.take(), vec![setting(Setting::Request04, AUTOFOCUS_MODE, &ports)]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens38)), Pending);
    assert_eq!(ports.take(), vec![setting(Setting::Request06, AUTOFOCUS_MODE, &ports)]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens39)), Pending);
    assert_eq!(ports.take(), vec![setting(Setting::Request05, AUTOFOCUS_MODE, &ports)]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens36)), Complete);
    assert_eq!(ports.take(), vec![]);
}

#[test]
fn a_setting_is_owed_only_after_every_owed_one() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    run(&mut lens, &mut ports, Event::Setting(Setting::Request04));
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request05)), Pending);
    ports.take();
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request06)), Fault);
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request05)), Fault);
    assert_eq!(ports.take(), vec![Seen::Fault, Seen::Fault]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens38)), Pending);
    assert_eq!(ports.take(), vec![setting(Setting::Request05, AUTOFOCUS_MODE, &ports)]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens39)), Fault);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens36)), Complete);
}

#[test]
fn a_refused_setting_faults_without_the_fault_reaction_and_keeps_the_state() {
    let mut lens = LiveLens::new();
    let mut ports = Fake { refuse: true, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request04)), Fault);
    assert_eq!(ports.take(), vec![setting(Setting::Request04, AUTOFOCUS_MODE, &ports)]);
    ports.refuse = false;
    assert_eq!(run(&mut lens, &mut ports, Event::Setting(Setting::Request06)), Pending);
    run(&mut lens, &mut ports, Event::Setting(Setting::Request05));
    ports.take();
    ports.refuse = true;
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens39)), Fault);
    ports.refuse = false;
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens39)), Pending);
    assert_eq!(ports.take().last(), Some(&setting(Setting::Request05, AUTOFOCUS_MODE, &ports)));
}

#[test]
fn a_corrected_drive_forces_word_four_and_reports_the_first_word() {
    let probes = Probes { bit0: Probe::Forced, bit1: Probe::Set, bit2: Probe::Forced };
    let mut lens = LiveLens::new();
    let mut ports = Fake { probes, ..correcting() };
    assert_eq!(run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command)), Pending);
    assert_eq!(ports.take(), vec![drive_sent(&ports, [V(11), V(12), V(13), V(1)])]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens34)), Complete);
    assert_eq!(ports.take(), vec![Seen::Reply(Reply::Drive(DriveReply {
        status: Status::Done, value: V(11), fourth: DriveFourth::One, detail: DriveDetail::Lens(probes),
    }))]);
}

#[test]
fn the_correction_needs_the_policy_and_the_shared_bit() {
    for (policy, resource, shared_bit, corrected) in [
        (policy(true, false, false), ResourceSelection::Other, true, true),
        (policy(false, true, false), ResourceSelection::One, true, true),
        (policy(false, true, false), ResourceSelection::Zero, true, false),
        (policy(false, false, true), ResourceSelection::Zero, true, true),
        (policy(false, false, true), ResourceSelection::One, true, false),
        (policy(true, false, false), ResourceSelection::Other, false, false),
        (policy(false, false, false), ResourceSelection::One, true, false),
    ] {
        let mut lens = LiveLens::new();
        let mut ports = Fake { policy, resource, shared_bit, ..Fake::default() };
        run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command));
        let fourth = if corrected { V(1) } else { V(14) };
        assert_eq!(ports.take(), vec![drive_sent(&ports, [V(11), V(12), V(13), fourth])], "{policy:?} {resource:?} {shared_bit}");
    }
}

#[test]
fn a_drive_on_cancellation_keeps_word_four_and_its_end_follows_the_policy() {
    let mut lens = LiveLens::new();
    let mut ports = correcting();
    assert_eq!(run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Cancel)), Pending);
    assert_eq!(ports.take(), vec![drive_sent(&ports, [V(11), V(12), V(13), V(14)])]);
    ports.policy = policy(false, false, false);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens37)), Complete);
    assert_eq!(ports.take(), vec![Seen::Reply(Reply::Drive(DriveReply {
        status: Status::Done, value: V(0x954), fourth: DriveFourth::Carried(V(14)), detail: DriveDetail::Lens(ports.probes),
    }))]);
}

#[test]
fn a_refused_drive_replies_and_stays_idle() {
    let mut lens = LiveLens::new();
    let mut ports = Fake {
        refuse: true,
        drive: crate::ports::DriveWords { fourth: DriveFourth::One, ..Fake::default().drive },
        ..Fake::default()
    };
    assert_eq!(run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command)), Fault);
    assert_eq!(ports.take(), vec![
        drive_sent(&ports, [V(11), V(12), V(13), V(1)]),
        Seen::Reply(Reply::Drive(DriveReply {
            status: Status::Refused, value: V(0x954), fourth: DriveFourth::One, detail: DriveDetail::Lens(ports.probes),
        })),
    ]);
    let mut ports = Fake { refuse: true, ..correcting() };
    assert_eq!(run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command)), Fault);
    assert_eq!(ports.take()[1], Seen::Reply(Reply::Drive(DriveReply {
        status: Status::Refused, value: V(11), fourth: DriveFourth::One, detail: DriveDetail::Lens(ports.probes),
    })));
    ports.refuse = false;
    assert_eq!(run(&mut lens, &mut ports, Event::PrepareDrive), Pending);
}

#[test]
fn a_cancel_while_prepared_becomes_the_drives_third_word() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut lens, &mut ports, Event::PrepareDrive), Pending);
    assert_eq!(run(&mut lens, &mut ports, Event::Cancel), Pending);
    assert_eq!(ports.take(), vec![Seen::SetDriveThird(V(0x77)), Seen::Reply(Reply::Cancel(Status::Accepted))]);
    assert_eq!(run(&mut lens, &mut ports, Event::Stop), Complete);
    assert_eq!(run(&mut lens, &mut ports, Event::Stop), Complete);
    assert_eq!(ports.take(), vec![]);
    assert_eq!(run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command)), Pending);
    assert_eq!(ports.take(), vec![drive_sent(&ports, [V(11), V(12), V(0x77), V(14)])]);
}

#[test]
fn cancel_and_stop_while_driving_send_and_accept() {
    for refuse in [false, true] {
        let mut lens = LiveLens::new();
        let mut ports = Fake::default();
        run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command));
        ports.take();
        ports.refuse = refuse;
        let outcome = if refuse { Fault } else { Pending };
        assert_eq!(run(&mut lens, &mut ports, Event::Cancel), outcome);
        assert_eq!(run(&mut lens, &mut ports, Event::Stop), outcome);
        assert_eq!(ports.take(), vec![
            Seen::Send(LensPayload::Finish { value: V(0x77), record: record() }),
            Seen::Reply(Reply::Cancel(Status::Accepted)),
            Seen::Send(LensPayload::Halt),
            Seen::Reply(Reply::Stop(Status::Accepted)),
        ]);
        assert_eq!(run(&mut lens, &mut ports, done(C::Lens34)), Complete);
    }
}

#[test]
fn cancel_and_stop_with_nothing_running_complete_as_unneeded() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut lens, &mut ports, Event::Cancel), Complete);
    assert_eq!(run(&mut lens, &mut ports, Event::Stop), Complete);
    assert_eq!(run(&mut lens, &mut ports, Event::Release), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Reply(Reply::Cancel(Status::Unneeded)),
        Seen::Reply(Reply::Stop(Status::Unneeded)),
        Seen::Reply(Reply::Release(Status::Unneeded)),
    ]);
}

#[test]
fn a_step_reply_carries_the_probes_it_read() {
    let probes = Probes { bit0: Probe::Forced, bit1: Probe::Forced, bit2: Probe::Forced };
    let mut lens = LiveLens::new();
    let mut ports = Fake { probes, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::Step), Pending);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens35)), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Step(ports.step)),
        Seen::Reply(Reply::Step { status: Status::Done, probes }),
    ]);
    ports.refuse = true;
    assert_eq!(run(&mut lens, &mut ports, Event::Step), Fault);
    assert_eq!(ports.take()[1], Seen::Reply(Reply::Step { status: Status::Refused, probes }));
    ports.refuse = false;
    run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command));
    run(&mut lens, &mut ports, done(C::Lens34));
    assert!(matches!(ports.take()[1], Seen::Reply(Reply::Drive(DriveReply { detail: DriveDetail::Lens(read), .. })) if read == probes));
}

#[test]
fn a_selection_prepares_the_record_after_its_request_and_releases() {
    let probes = Probes { bit0: Probe::Clear, bit1: Probe::Forced, bit2: Probe::Clear };
    let mut lens = LiveLens::new();
    let mut ports = Fake { configured: true, probes, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::Select), Pending);
    assert_eq!(run(&mut lens, &mut ports, Event::Release), Pending);
    let payload = [0, 0, 0, 0, 1, 0, 0, 0, 0x2d, 0x3d, 0, 0];
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens3d, &payload))), Complete);
    assert_eq!(run(&mut lens, &mut ports, Event::Release), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Select { value: V(0x2c2c), code: SelectionCode::Configured, record: record() }),
        Seen::PrepareSelection(record()),
        Seen::Send(LensPayload::Release),
        Seen::Reply(Reply::Release(Status::Accepted)),
        Seen::Reply(Reply::Select { status: Status::Done, probes, value: V(0x3d2d) }),
        Seen::Reply(Reply::Release(Status::Unneeded)),
    ]);
}

#[test]
fn a_refused_selection_replies_without_preparing_the_record() {
    let mut lens = LiveLens::new();
    let mut ports = Fake { refuse: true, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::Select), Fault);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Select { value: V(0x2c2c), code: SelectionCode::Unconfigured, record: record() }),
        Seen::Reply(Reply::Select { status: Status::Refused, probes: ports.probes, value: V(0) }),
    ]);
    ports.refuse = false;
    assert_eq!(run(&mut lens, &mut ports, Event::Select), Pending);
    ports.refuse = true;
    assert_eq!(run(&mut lens, &mut ports, Event::Release), Fault);
    assert_eq!(&ports.take()[2..], &[Seen::Send(LensPayload::Release), Seen::Reply(Reply::Release(Status::Refused))]);
}

#[test]
fn events_without_an_entry_are_inactive_in_every_state() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    for setup in [None, Some(Event::PrepareDrive), Some(Event::Drive(DriveOrigin::Command)), Some(Event::Step)] {
        if let Some(event) = setup { run(&mut lens, &mut ports, event); }
        ports.take();
        assert_eq!(run(&mut lens, &mut ports, Event::Other), Inactive);
        assert_eq!(run(&mut lens, &mut ports, done(C::Lens33)), Inactive);
        assert_eq!(run(&mut lens, &mut ports, done(C::Image3f)), Inactive);
        assert_eq!(ports.take(), vec![]);
        lens = LiveLens::new();
    }
}

#[test]
fn an_event_out_of_turn_takes_the_fault_reaction_and_keeps_the_state() {
    let mut lens = LiveLens::new();
    let mut ports = Fake::default();
    run(&mut lens, &mut ports, Event::Drive(DriveOrigin::Command));
    ports.take();
    for event in [Event::PrepareDrive, Event::Profile(Profile::Primary), Event::Step, Event::Select, Event::Release, done(C::Lens35)] {
        assert_eq!(run(&mut lens, &mut ports, event), Fault);
    }
    assert_eq!(ports.take(), vec![Seen::Fault; 6]);
    assert_eq!(run(&mut lens, &mut ports, done(C::Lens34)), Complete);
}

#[test]
fn every_request_is_marked_lens_reaching() {
    let mut lens = LiveLens::new();
    let mut ports = correcting();
    for event in [
        Event::Profile(Profile::Primary), done(C::Lens36),
        Event::Setting(Setting::Request04), Event::Setting(Setting::Request06), Event::Setting(Setting::Request05),
        done(C::Lens38), done(C::Lens39), done(C::Lens36),
        Event::Drive(DriveOrigin::Command), Event::Cancel, Event::Stop, done(C::Lens34),
        Event::Step, done(C::Lens35), Event::Select, Event::Release, done(C::Lens3d),
    ] {
        run(&mut lens, &mut ports, event);
    }
    assert_eq!(ports.seen.iter().filter(|seen| matches!(seen, Seen::Send(_))).count(), 10);
    assert!(ports.effects_require_contract);
}
