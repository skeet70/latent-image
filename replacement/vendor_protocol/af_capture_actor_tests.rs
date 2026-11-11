use super::{ActorError, ClientId, LensCompletion, LensMachineState, LensOutcome, LensRequest, LensRequestConsumer, Submission};
use super::super::lens_queue_runtime::{RuntimeError, Status};
use super::super::lens_queue_runtime::physical_fixture::{expire, initialize, received, setup, transmitted};
use xm1_lens_transport::wire::Error as WireError;

fn client(value: u32) -> ClientId { ClientId::new(value).unwrap() }
fn ready() -> LensRequestConsumer { LensRequestConsumer::from_machine_snapshot(LensMachineState::from_snapshot(2).unwrap()) }

#[test]
fn selected_requests_transmit_their_actual_lens_commands() {
    for (request, bytes) in [
        (LensRequest::ReadPosition { client: client(1) }, [0, 0, 0x48, 0x28]),
        (LensRequest::SetOption { client: client(1), enabled: true }, [0, 1, 0x54, 0x3a]),
        (LensRequest::SetMode { client: client(1), enabled: true }, [0, 1, 0x58, 0x0a]),
    ] {
        let (mut runtime, physical) = setup();
        initialize(&mut runtime);
        let mut actor = ready();
        assert_eq!(actor.submit(&mut runtime, request).unwrap(), Submission::Waiting);
        assert_eq!(transmitted(&physical, 4), bytes);
        assert_eq!(runtime.pop_publication(), None);
    }
}

#[test]
fn read_position_requires_the_command_response_and_its_acknowledgement() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    let mut actor = ready();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(8) }).unwrap();
    received(&mut runtime, &physical, &[0, 0, 0, 0]);
    expire(&mut runtime, &physical);
    // 0x006bdf44 compares the ack's nine-bit command field at 0x006be210..0x006be228 with the command on the wire.
    received(&mut runtime, &physical, &[0x08, 0, 0xc8, 0x3a]);
    assert_eq!(runtime.pop_publication(), None);
    expire(&mut runtime, &physical);
    received(&mut runtime, &physical, &[0x92, 0x34, 0x48, 0x10]);
    assert_eq!(runtime.pop_publication(), None);
    expire(&mut runtime, &physical);
    received(&mut runtime, &physical, &[0x08, 0, 0x80, 0x12]);
    let completion = actor.consume(runtime.pop_publication().unwrap()).unwrap().unwrap();
    assert_eq!(completion.client, client(8));
    assert!(matches!(completion.outcome, LensOutcome::Position { flag: true, position } if position.get() == 0x1234));
    assert_eq!(runtime.pop_publication(), None);
}

#[test]
fn duplicate_requests_do_not_replace_the_active_client() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    let mut actor = ready();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(1) }).unwrap();
    let wire = physical.wire.get();
    assert_eq!(actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(2) }).unwrap(), Submission::Duplicate);
    assert_eq!(physical.wire.get(), wire);
    runtime.cancel().unwrap();
    assert_eq!(actor.consume(runtime.pop_publication().unwrap()).unwrap(), Some(LensCompletion { client: client(1), outcome: LensOutcome::Driver(Status::Stopped) }));
}

#[test]
fn another_selected_job_is_rejected_while_waiting_without_wire_activity() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    let mut actor = ready();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(1) }).unwrap();
    let wire = physical.wire.get();
    assert_eq!(actor.submit(&mut runtime, LensRequest::SetMode { client: client(2), enabled: false }).unwrap(), Submission::Completed(LensCompletion { client: client(2), outcome: LensOutcome::RejectedBusy }));
    assert_eq!(physical.wire.get(), wire);
}

#[test]
fn cancellation_does_not_complete_a_later_client() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    let mut actor = ready();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(1) }).unwrap();
    let stale_wire = physical.wire.get().unwrap();
    runtime.cancel().unwrap();
    let cancellation = runtime.pop_publication().unwrap();
    assert_eq!(actor.consume(cancellation).unwrap(), Some(LensCompletion { client: client(1), outcome: LensOutcome::Driver(Status::Stopped) }));
    assert_eq!(actor.consume(cancellation).unwrap(), None);
    initialize(&mut runtime);
    actor.submit(&mut runtime, LensRequest::SetOption { client: client(2), enabled: true }).unwrap();
    assert!(matches!(runtime.wire_completed(stale_wire), Err(RuntimeError::Transport(xm1_lens_transport::Error::Backend(WireError::StaleToken)))));
    assert_eq!(actor.consume(cancellation).unwrap(), None);
    runtime.cancel().unwrap();
    assert_eq!(actor.consume(runtime.pop_publication().unwrap()).unwrap(), Some(LensCompletion { client: client(2), outcome: LensOutcome::Driver(Status::Stopped) }));
}

#[test]
fn uninitialized_task_does_not_issue_a_lens_request() {
    let (mut runtime, physical) = setup();
    let state = LensMachineState::from_snapshot(0).unwrap();
    let mut actor = LensRequestConsumer::from_machine_snapshot(state);
    assert!(matches!(actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(1) }), Err(ActorError::Uninitialized(value)) if value == state));
    assert!(physical.words.borrow().is_empty());
    assert_eq!(runtime.pop_publication(), None);
}

#[test]
fn stopped_driver_is_not_reported_as_success() {
    let (mut runtime, physical) = setup();
    let mut actor = ready();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(3) }).unwrap();
    assert_eq!(actor.consume(runtime.pop_publication().unwrap()).unwrap(), Some(LensCompletion { client: client(3), outcome: LensOutcome::Driver(Status::Stopped) }));
    assert!(physical.words.borrow().is_empty());
}

#[test]
fn external_task50_motion_uses_the_same_admission_state() {
    let (mut runtime, physical) = setup();
    initialize(&mut runtime);
    let mut actor = ready();
    let idle = actor.machine_state();
    let moving = LensMachineState::from_snapshot(15).unwrap();
    actor.transition_when_idle(idle, moving).unwrap();
    assert_eq!(actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(1) }).unwrap(), Submission::Completed(LensCompletion { client: client(1), outcome: LensOutcome::RejectedBusy }));
    assert_eq!(actor.transition_when_idle(idle, idle), Err(super::StateTransitionError::StaleState));
    actor.transition_when_idle(moving, idle).unwrap();
    actor.submit(&mut runtime, LensRequest::ReadPosition { client: client(2) }).unwrap();
    let wire = physical.wire.get();
    assert_eq!(actor.transition_when_idle(moving, idle), Err(super::StateTransitionError::SelectedRequestPending));
    assert_eq!(physical.wire.get(), wire);
}
