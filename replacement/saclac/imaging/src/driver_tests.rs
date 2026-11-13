use crate::driver::{acquire, release, Client, Event, Fault, Isp, Operation, Record, Refusal, SessionId, Unit};
use crate::fake::{Effect, FakeIsp, IDLE};

#[test]
fn acquire_hands_out_the_next_session_and_names_the_holder() {
    let mut isp = FakeIsp::new();
    assert_eq!(acquire(&mut isp, Unit::Raid0, Client::S2), Ok(SessionId(1)));
    assert_eq!(isp.stored_record(Unit::Raid0), Record { holder: Client::S2, operation: Operation::ACQUIRED, active: Some(SessionId(1)), next: SessionId(2) });
    assert_eq!(isp.take(), [Effect::Enter, Effect::Leave]);
}

#[test]
fn a_second_acquire_is_logged_with_the_holder_inside_the_critical_section() {
    let mut isp = FakeIsp::new();
    acquire(&mut isp, Unit::Mvrec, Client::REC).unwrap();
    isp.note(Unit::Mvrec, Operation(15));
    isp.take();
    assert_eq!(acquire(&mut isp, Unit::Mvrec, Client::MV), Err(Refusal::Busy));
    let busy = Event::Busy { holder: Client::REC, requester: Client::MV, unit: Unit::Mvrec, operation: Operation(15) };
    assert_eq!(isp.take(), [Effect::Enter, Effect::Log(busy), Effect::Leave]);
    assert_eq!(isp.stored_record(Unit::Mvrec).active, Some(SessionId(1)));
}

#[test]
fn sessions_wrap_after_0x1000() {
    let mut isp = FakeIsp::new();
    isp.put_record(Unit::Auto, Record { next: SessionId(0x1000), ..IDLE });
    assert_eq!(acquire(&mut isp, Unit::Auto, Client::S1), Ok(SessionId(0x1000)));
    assert_eq!(isp.stored_record(Unit::Auto).next, SessionId(1));
}

#[test]
fn release_frees_the_unit_it_was_handed() {
    let mut isp = FakeIsp::new();
    let session = acquire(&mut isp, Unit::Raid0, Client::S1).unwrap();
    isp.take();
    assert_eq!(release(&mut isp, Unit::Raid0, Client::S1, Some(session)), Ok(()));
    assert_eq!(isp.stored_record(Unit::Raid0), Record { holder: Client::NONE, operation: Operation::RELEASED, active: None, next: SessionId(2) });
    assert_eq!(isp.take(), [Effect::Note(Unit::Raid0, Operation::RELEASING), Effect::Enter, Effect::Leave]);
}

#[test]
fn release_of_another_session_is_logged_and_keeps_the_holder() {
    let mut isp = FakeIsp::new();
    acquire(&mut isp, Unit::Raid0, Client::S1).unwrap();
    isp.take();
    assert_eq!(release(&mut isp, Unit::Raid0, Client::S2, Some(SessionId(7))), Err(Refusal::WrongSession));
    let refused = Event::Refused { client: Client::S2, unit: Unit::Raid0, operation: Operation::RELEASED, fault: Fault::ResourceId };
    assert_eq!(isp.logs(), [refused]);
    let record = isp.stored_record(Unit::Raid0);
    assert_eq!((record.holder, record.operation, record.active), (Client::S1, Operation::RELEASING, Some(SessionId(1))));
}

#[test]
fn a_failed_acquire_releases_an_idle_unit() {
    let mut isp = FakeIsp::new();
    assert_eq!(release(&mut isp, Unit::Mvrec, Client::JIG, None), Ok(()));
    assert!(isp.logs().is_empty());
}
