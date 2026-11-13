use crate::ports::{DriveDetail, DriveReply, LensFlags, Probe, Probes, Reply, Status};
use crate::s1::{Event, S1Lens};
use crate::test_ports::{completed, record, Fake, Seen};
use crate::Outcome::{self, Complete, Fault, Inactive, Pending};
use std::vec;
use xm1_saclac_interface::lens::{DriveOwner, LensPayload};
use xm1_saclac_interface::{Completion as C, ParameterValue as V};

fn run(lens: &mut S1Lens, ports: &mut Fake, event: Event<'_>) -> Outcome { lens.handle(event, record(), ports) }

fn reported(status: Status, lens: u8) -> Seen {
    Seen::Reply(Reply::Drive(DriveReply { status, value: V(0x750c), fourth_not_one: true, detail: DriveDetail::Lens(LensFlags(lens)) }))
}

#[test]
fn the_cancellation_drive_sends_the_command_words_and_reports_progress_then_end() {
    let mut lens = S1Lens::new();
    let mut ports = Fake { policy: crate::ports::DrivePolicy(8), shared_bit: true, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::CancelDrive), Pending);
    let progress = [0x12, 0, 0, 0, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0x78, 0x56, 0, 0];
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens33, &progress))), Pending);
    ports.probes = Probes { bit0: Probe::Clear, bit1: Probe::Forced, bit2: Probe::Clear };
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens34, &progress))), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Drive { owner: DriveOwner::S1, parameters: [V(11), V(12), V(13), V(14)], slot: ports.slot }),
        Seen::Reply(Reply::Drive(DriveReply {
            status: Status::Progress, value: V(0x1234), fourth_not_one: true, detail: DriveDetail::Reported(V(0x5678)),
        })),
        reported(Status::Done, 2),
    ]);
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens34, &[]))), Fault);
}

#[test]
fn a_refused_drive_replies_with_the_last_report_and_stays_idle() {
    let mut lens = S1Lens::new();
    let mut ports = Fake { refuse: true, ..Fake::default() };
    assert_eq!(run(&mut lens, &mut ports, Event::CancelDrive), Fault);
    assert_eq!(ports.take()[1], reported(Status::Refused, 0));
    ports.refuse = false;
    assert_eq!(run(&mut lens, &mut ports, Event::CancelDrive), Pending);
}

#[test]
fn cancel_and_stop_with_no_drive_accept_and_stop_reports_the_drive_done() {
    let mut lens = S1Lens::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut lens, &mut ports, Event::Cancel), Pending);
    assert_eq!(run(&mut lens, &mut ports, Event::Stop), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Reply(Reply::Cancel(Status::Accepted)),
        Seen::Reply(Reply::Stop(Status::Accepted)),
        reported(Status::Done, 0),
    ]);
}

#[test]
fn cancel_and_stop_during_the_drive_send_and_accept() {
    for refuse in [false, true] {
        let mut lens = S1Lens::new();
        let mut ports = Fake::default();
        run(&mut lens, &mut ports, Event::CancelDrive);
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
    }
}

#[test]
fn reports_without_a_drive_fault_and_other_events_are_inactive() {
    let mut lens = S1Lens::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens33, &[]))), Fault);
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens34, &[]))), Fault);
    assert_eq!(ports.take(), vec![Seen::Fault, Seen::Fault]);
    run(&mut lens, &mut ports, Event::CancelDrive);
    ports.take();
    assert_eq!(run(&mut lens, &mut ports, Event::CancelDrive), Fault);
    assert_eq!(run(&mut lens, &mut ports, Event::Other), Inactive);
    assert_eq!(run(&mut lens, &mut ports, Event::Completed(completed(C::Lens35, &[]))), Inactive);
    assert_eq!(ports.take(), vec![Seen::Fault]);
    assert!(ports.effects_require_contract);
}
