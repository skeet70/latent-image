use super::*;
use std::vec::Vec;
use std::vec;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinkError { Busy, Unregistered, Rejected }
#[derive(Default)]
struct WireInput {
    registered: bool,
    wire: Option<(TransferToken, Vec<u8>)>,
    timer: Option<(TimerToken, TimerTicks)>,
    requested: bool,
    reject_start: bool,
    reject_output: bool,
}
impl Backend for WireInput {
    type Error = LinkError;
    fn register(&mut self) -> Result<(), Self::Error> { self.registered = true; Ok(()) }
    fn start(&mut self, token: TransferToken, bytes: &[u8]) -> Result<(), Self::Error> {
        if !self.registered { return Err(LinkError::Unregistered); }
        if self.reject_start { return Err(LinkError::Rejected); }
        if self.wire.is_some() { return Err(LinkError::Busy); }
        self.wire = Some((token, bytes.to_vec()));
        Ok(())
    }
    fn arm_timer(&mut self, token: TimerToken, delay: TimerTicks) -> Result<(), Self::Error> {
        if self.timer.is_some() { return Err(LinkError::Busy); }
        self.timer = Some((token, delay));
        Ok(())
    }
    fn cancel_timer(&mut self) -> Result<(), Self::Error> { self.timer = None; Ok(()) }
    fn abort_wire(&mut self) -> Result<(), Self::Error> { self.wire = None; Ok(()) }
    fn unregister(&mut self) -> Result<(), Self::Error> { self.registered = false; Ok(()) }
    fn set_requested_output(&mut self, active: bool) -> Result<(), Self::Error> {
        if active && self.reject_output { return Err(LinkError::Rejected); }
        self.requested = active;
        Ok(())
    }
    fn diagnostic_halt(&mut self) -> ! { panic!("diagnostic mode was not selected by this fixture") }
}

#[derive(Debug, Eq, PartialEq)]
enum Receipt {
    Command(RequestToken, CommandWord),
    Write(RequestToken, PageWriteCompletion),
    Read(RequestToken, PageId, Vec<u8>),
    Terminal(RequestToken, CompletionStatus),
    Failure,
}
#[derive(Default)]
struct Consumer { receipts: Vec<Receipt> }
impl CompletionSink for Consumer {
    fn command_completed(&mut self, token: RequestToken, response: CommandWord) -> CallbackDelay {
        self.receipts.push(Receipt::Command(token, response)); CallbackDelay::Deferred
    }
    fn page_written(&mut self, token: RequestToken, response: PageWriteCompletion) { self.receipts.push(Receipt::Write(token, response)); }
    fn page_read(&mut self, token: RequestToken, page: PageId, bytes: &[u8]) { self.receipts.push(Receipt::Read(token, page, bytes.to_vec())); }
    fn terminated(&mut self, token: RequestToken, status: CompletionStatus) { self.receipts.push(Receipt::Terminal(token, status)); }
    fn transport_failed(&mut self) { self.receipts.push(Receipt::Failure); }
}

type Session = Transport<WireInput, Consumer>;
fn timing() -> TimingParameters {
    TimingParameters {
        command: TimerTicks::new(1).unwrap(), normal: TimerTicks::new(2).unwrap(), deferred: TimerTicks::new(3).unwrap(),
        accepted_page: TimerTicks::new(4).unwrap(), rejected_page: TimerTicks::new(5).unwrap(), recovery: TimerTicks::new(6).unwrap(),
        recovery_limit: RetryLimit::new(2), busy_limit: RetryLimit::new(2), failure_mode: FailureMode::Notify,
    }
}
fn session() -> Session {
    let mut session = Transport::new(WireInput::default(), Consumer::default(), timing());
    session.initialize().unwrap();
    session
}
fn receive(session: &mut Session, bytes: &[u8]) -> Result<(), Error<LinkError>> {
    let (token, _) = session.backend_mut().wire.take().unwrap();
    session.receive(token, bytes)
}
fn expire(session: &mut Session) -> Result<(), Error<LinkError>> {
    let (token, _) = session.backend().timer.unwrap();
    session.timer_expired(token)
}
fn transmitted(session: &Session) -> &[u8] { &session.backend().wire.as_ref().unwrap().1 }

#[test]
fn command_response_is_published_after_its_acknowledgement() {
    let mut session = session();
    let request = session.enqueue_command(CommandWord::new(0x40)).unwrap();
    assert_eq!(transmitted(&session), &[0, 0, 0, 0x42]);
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    expire(&mut session).unwrap();
    assert_eq!(transmitted(&session), &[0x08, 0x10, 0x80, 0x22]);
    receive(&mut session, &[0x08, 0, 0x80, 0x54]).unwrap();
    assert!(session.completions().receipts.is_empty());
    expire(&mut session).unwrap();
    assert_eq!(transmitted(&session), &[0, 0, 0, 0]);
    receive(&mut session, &[0, 0, 0, 0x42]).unwrap();
    assert!(session.completions().receipts.is_empty());
    expire(&mut session).unwrap();
    receive(&mut session, &[0x08, 0, 0x80, 0x12]).unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Command(request, CommandWord::new(0x42))]);
    session.stop().unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Command(request, CommandWord::new(0x42))]);
}

#[test]
fn checksum_error_uses_source_ack_retry_then_recovery() {
    let mut session = session();
    session.enqueue_command(CommandWord::new(0x40)).unwrap();
    receive(&mut session, &[0, 0, 0, 2]).unwrap();
    assert!(session.completions().receipts.is_empty());
    expire(&mut session).unwrap();
    assert_eq!(transmitted(&session), &[0x08, 0x02, 0x80, 0x14]);
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(6).unwrap());
    expire(&mut session).unwrap();
    assert_eq!(transmitted(&session), &[0xa5, 0x5a, 0x3c, 0xc3]);
}

#[test]
fn busy_responses_use_configured_limit() {
    let mut session = session();
    let request = session.enqueue_command(CommandWord::new(0x40)).unwrap();
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    expire(&mut session).unwrap();
    receive(&mut session, &[0x08, 0x10, 0x80, 0x64]).unwrap();
    assert!(session.completions().receipts.is_empty());
    expire(&mut session).unwrap();
    receive(&mut session, &[0x08, 0x10, 0x80, 0x64]).unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Failure, Receipt::Terminal(request, CompletionStatus::Failed)]);
}

#[test]
fn write_receipt_requires_explicit_acceptance_before_next_request() {
    let mut session = session();
    let request = session.write_page(PageId::new(0xff00), &[1, 2], ChecksumMode::Enabled).unwrap();
    assert_eq!(transmitted(&session), &[0xff, 0, 1, 2, 2]);
    assert_eq!(session.read_page(PageId::new(1), PayloadLength::new(0).unwrap(), TransferOption::Plain, ReadMarker::Disabled), Err(Error::Busy));
    receive(&mut session, &[0xff, 0, 0, 0, 0x5a]).unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Write(request, PageWriteCompletion { page: PageId::new(0xff00), marker_valid: true })]);
    session.accept_page(PageAcceptance::Accepted).unwrap();
    assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(4).unwrap());
    session.stop().unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Write(request, PageWriteCompletion { page: PageId::new(0xff00), marker_valid: true })]);
}

#[test]
fn short_frame_and_bounded_length_are_rejected_without_publication() {
    let mut session = session();
    assert_eq!(session.write_page(PageId::new(1), &[0; MAX_PAYLOAD_LENGTH + 1], ChecksumMode::Enabled), Err(Error::Frame(FrameError::LengthOverflow)));
    assert!(session.backend().wire.is_none());
    let request = session.read_page(PageId::new(0x1234), PayloadLength::new(2).unwrap(), TransferOption::Plain, ReadMarker::Disabled).unwrap();
    assert_eq!(transmitted(&session), &[0x12, 0x34, 0, 0]);
    assert_eq!(receive(&mut session, &[0x12]), Err(Error::Frame(FrameError::ShortFrame)));
    assert!(session.completions().receipts.is_empty());
    session.cancel().unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Terminal(request, CompletionStatus::Cancelled)]);
}

#[test]
fn read_payload_is_not_an_echo_and_rejection_selects_its_delay() {
    let mut session = session();
    let request = session.read_page(PageId::new(0x1234), PayloadLength::new(2).unwrap(), TransferOption::ExtraByte, ReadMarker::Enabled).unwrap();
    assert_eq!(transmitted(&session), &[0x12, 0x34, 0, 0, 0x5a]);
    receive(&mut session, &[0x56, 0x78, 0xa7, 0x31, 0]).unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Read(request, PageId::new(0x5678), vec![0xa7, 0x31, 0])]);
    session.accept_page(PageAcceptance::Rejected).unwrap();
    assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(5).unwrap());
}

#[test]
fn cancellation_rejects_late_wire_and_timer_events() {
    let mut session = session();
    let request = session.enqueue_command(CommandWord::new(0x40)).unwrap();
    let wire_token = session.backend().wire.as_ref().unwrap().0;
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    let timer_token = session.backend().timer.unwrap().0;
    session.cancel().unwrap();
    assert_eq!(session.receive(wire_token, &[0, 0, 0, 0]), Err(Error::LateCompletion));
    assert_eq!(session.timer_expired(timer_token), Err(Error::LateCompletion));
    assert!(!session.backend().registered);
    assert_eq!(session.completions().receipts, vec![Receipt::Terminal(request, CompletionStatus::Cancelled)]);
}

#[test]
fn reset_does_not_reuse_completion_identity() {
    let mut session = session();
    let first = session.write_page(PageId::new(1), &[7], ChecksumMode::Disabled).unwrap();
    let old_token = session.backend().wire.as_ref().unwrap().0;
    session.reset().unwrap();
    let second = session.write_page(PageId::new(2), &[8], ChecksumMode::Disabled).unwrap();
    assert_ne!(first, second);
    assert_eq!(session.receive(old_token, &[0, 1, 7]), Err(Error::LateCompletion));
    receive(&mut session, &[0, 2, 8]).unwrap();
    session.stop().unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Terminal(first, CompletionStatus::Reset), Receipt::Write(second, PageWriteCompletion { page: PageId::new(2), marker_valid: true })]);
}

#[test]
fn recovery_timeouts_exhaust_only_the_source_limit() {
    let mut session = session();
    let request = session.enqueue_command(CommandWord::new(0x40)).unwrap();
    receive(&mut session, &[0xc3, 0x3c, 0xa5, 0x5a]).unwrap();
    expire(&mut session).unwrap();
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    expire(&mut session).unwrap();
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    assert_eq!(session.completions().receipts, vec![Receipt::Failure, Receipt::Terminal(request, CompletionStatus::Failed)]);
    session.stop().unwrap();
    assert!(!session.backend().registered);
}

#[test]
fn stale_timer_cannot_advance_a_new_transfer() {
    let mut session = session();
    session.enqueue_command(CommandWord::new(0x40)).unwrap();
    receive(&mut session, &[0, 0, 0, 0]).unwrap();
    let old = session.backend().timer.unwrap().0;
    expire(&mut session).unwrap();
    let packet = transmitted(&session).to_vec();
    assert_eq!(session.timer_expired(old), Err(Error::LateCompletion));
    assert_eq!(transmitted(&session), packet);
}

#[test]
fn failed_request_publication_retires_the_request_and_allows_fresh_initialization() {
    #[derive(Clone, Copy)]
    enum Request { Command, Read, Write }
    for request in [Request::Command, Request::Read, Request::Write] {
        for reject_output in [false, true] {
            let mut session = session();
            session.backend_mut().reject_start = !reject_output;
            session.backend_mut().reject_output = reject_output;
            let result = match request {
                Request::Command => session.enqueue_command(CommandWord::new(0x40)),
                Request::Read => session.read_page(PageId::new(1), PayloadLength::new(1).unwrap(), TransferOption::Plain, ReadMarker::Disabled),
                Request::Write => session.write_page(PageId::new(1), &[7], ChecksumMode::Disabled),
            };
            assert_eq!(result, Err(Error::Backend(LinkError::Rejected)));
            assert!(matches!(session.completions().receipts.as_slice(), [Receipt::Failure, Receipt::Terminal(_, CompletionStatus::Failed)]));
            assert!(session.backend().wire.is_none());
            assert!(!session.backend().registered);
            session.backend_mut().reject_start = false;
            session.backend_mut().reject_output = false;
            session.initialize().unwrap();
            session.enqueue_command(CommandWord::new(0x40)).unwrap();
            assert_eq!(transmitted(&session), &[0, 0, 0, 0x42]);
        }
    }
}

#[test]
fn registration_replacement_preserves_wire_and_timer_identity() {
    let mut first = session();
    first.enqueue_command(CommandWord::new(0x40)).unwrap();
    let old_wire = first.backend().wire.as_ref().unwrap().0;
    receive(&mut first, &[0, 0, 0, 0]).unwrap();
    let old_timer = first.backend().timer.unwrap().0;
    first.stop().unwrap();
    let generation = first.generation_checkpoint();
    let mut second = Transport::resume(WireInput::default(), Consumer::default(), timing(), generation);
    second.initialize().unwrap();
    second.enqueue_command(CommandWord::new(0x40)).unwrap();
    assert_eq!(second.receive(old_wire, &[0, 0, 0, 0]), Err(Error::LateCompletion));
    receive(&mut second, &[0, 0, 0, 0]).unwrap();
    assert_eq!(second.timer_expired(old_timer), Err(Error::LateCompletion));
    expire(&mut second).unwrap();
    assert_eq!(transmitted(&second), &[0x08, 0x10, 0x80, 0x22]);
}

#[test]
fn timing_profiles_change_only_the_five_command_delays() {
    for deferred in [false, true] {
        let mut session = session();
        session.enqueue_command(CommandWord::new(0x40)).unwrap();
        let profile = TimingProfile {
            command: TimerTicks::new(11).unwrap(), normal: TimerTicks::new(12).unwrap(), deferred: TimerTicks::new(13).unwrap(),
            accepted_page: TimerTicks::new(14).unwrap(), rejected_page: TimerTicks::new(15).unwrap(),
        };
        if deferred { session.replace_timing(profile); } else { session.set_timing(profile); }
        receive(&mut session, &[0, 0, 0, 0]).unwrap();
        assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(if deferred { 1 } else { 11 }).unwrap());
        expire(&mut session).unwrap();
        receive(&mut session, &[0x08, 0, 0x80, 0x54]).unwrap();
        assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(12).unwrap());
        expire(&mut session).unwrap();
        receive(&mut session, &[0xc3, 0x3c, 0xa5, 0x5a]).unwrap();
        assert_eq!(session.backend().timer.unwrap().1, TimerTicks::new(6).unwrap());
    }
}
