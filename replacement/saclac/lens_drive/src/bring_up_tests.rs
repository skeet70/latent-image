use crate::bring_up::{BringUp, Event};
use crate::ports::{Reply, ResourceIndex, Status};
use crate::test_ports::{completed, record, Fake, Seen};
use crate::Outcome::{self, Complete, Fault, Inactive, Pending};
use std::vec;
use xm1_saclac_interface::lens::LensPayload;
use xm1_saclac_interface::{Completion as C, ParameterValue as V};

fn run(child: &mut BringUp, ports: &mut Fake, event: Event<'_>) -> Outcome { child.handle(event, record(), ports) }

const REPORT: [u8; 16] = [0xfe, 0xff, 0xff, 0xff, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0];

fn report() -> [Seen; 2] {
    [Seen::SelectLookup(ResourceIndex(2)), Seen::Reply(Reply::BringUp { status: Status::Lens(-2), values: [V(1), V(2), V(3)] })]
}

#[test]
fn bring_up_reports_each_progress_and_completes_on_lens2f_then_stops() {
    let mut child = BringUp::new();
    let mut ports = Fake::default();
    assert_eq!(run(&mut child, &mut ports, Event::BringUp), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens31, &REPORT))), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Image3f, &[]))), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens32, &REPORT))), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens2f, &REPORT))), Complete);
    let mut expected = vec![Seen::Send(LensPayload::BringUp { parameters: [1, 2, 3, 4, 5].map(V), record: record() })];
    for _ in 0..3 { expected.extend(report()); }
    assert_eq!(ports.take(), expected);

    assert_eq!(run(&mut child, &mut ports, Event::Stop), Pending);
    let stopped = [0x15, 0, 0, 0, 9, 0, 0, 0];
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens30, &stopped))), Complete);
    assert_eq!(ports.take(), vec![
        Seen::Send(LensPayload::Stop { parameters: [V(6), V(7)], record: record() }),
        Seen::Reply(Reply::LensStop { status: Status::Lens(0x15), value: V(9) }),
    ]);
    assert_eq!(run(&mut child, &mut ports, Event::BringUp), Pending);
}

#[test]
fn a_refused_post_still_waits_for_lens_control() {
    let mut child = BringUp::new();
    let mut ports = Fake { refuse: true, ..Fake::default() };
    assert_eq!(run(&mut child, &mut ports, Event::Stop), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::BringUp), Fault);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens30, &[]))), Complete);
    assert_eq!(run(&mut child, &mut ports, Event::BringUp), Pending);
    assert_eq!(run(&mut child, &mut ports, Event::Stop), Fault);
    assert!(ports.effects_require_contract);
}

#[test]
fn completions_out_of_turn_fault_and_others_are_inactive() {
    let mut child = BringUp::new();
    let mut ports = Fake::default();
    for done in [C::Lens2f, C::Lens30, C::Lens31, C::Lens32, C::Image3f, C::Image40] {
        assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(done, &[]))), Fault);
    }
    assert_eq!(run(&mut child, &mut ports, Event::Other), Inactive);
    assert_eq!(run(&mut child, &mut ports, Event::Completed(completed(C::Lens34, &[]))), Inactive);
    assert_eq!(ports.take(), vec![Seen::Fault; 6]);
}
