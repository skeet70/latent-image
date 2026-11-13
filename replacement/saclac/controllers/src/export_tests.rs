use crate::{adjustment as a, capture as c, exports::*, records as r, stock as s, synchronous as y};
use std::vec::Vec;
use xm1_saclac_interface::{stock, ClientCallback, CompletionStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Buffer(u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Query { PositionAvailability, Position, MotionAvailability, Motion, MissingLens, ImagePrimary, ImageSecondary, Movie(y::MoviePhase), Capture }
#[derive(Clone, Debug, Eq, PartialEq)]
enum Event {
    Reservation(y::Effect), Lens(a::Effect<ClientCallback>), Record(r::Reply<Buffer>),
    Start([u8;68]), Update([u8;20]), Capture([u8;176]), Query(Query),
    FinishPrimary(Buffer), FinishCapture, Release(r::Source), Sample(Buffer), ParametersReleased,
}
#[derive(Default)]
struct Peer(Vec<Event>);
impl y::Effects for Peer { fn emit(&mut self, effect: y::Effect) { self.0.push(Event::Reservation(effect)); } }
impl a::Effects<ClientCallback> for Peer { fn emit(&mut self, effect: a::Effect<ClientCallback>) { self.0.push(Event::Lens(effect)); } }
impl y::Queries for Peer {
    fn position_availability(&mut self) -> y::Availability { self.0.push(Event::Query(Query::PositionAvailability)); y::Availability::Available }
    fn position(&mut self) -> y::LensPositionCode { self.0.push(Event::Query(Query::Position)); y::LensPositionCode(37) }
    fn motion_availability(&mut self) -> y::Availability { self.0.push(Event::Query(Query::MotionAvailability)); y::Availability::Available }
    fn motion(&mut self) -> a::LensPosition { self.0.push(Event::Query(Query::Motion)); a::LensPosition(-4) }
    fn report_missing_lens(&mut self) { self.0.push(Event::Query(Query::MissingLens)); }
    fn image_primary(&mut self) -> y::ImageStatus { self.0.push(Event::Query(Query::ImagePrimary)); y::ImageStatus(5) }
    fn image_secondary(&mut self) -> y::ImageStatus { self.0.push(Event::Query(Query::ImageSecondary)); y::ImageStatus(6) }
    fn movie_readiness(&mut self, phase: y::MoviePhase) -> y::Readiness { self.0.push(Event::Query(Query::Movie(phase))); y::Readiness::Ready }
}
impl r::Ports<Buffer> for Peer {
    type Sample = u8;
    fn read_sample(&mut self, _: u8, destination: Buffer) -> [r::RecordPair;2] { self.0.push(Event::Sample(destination)); [r::RecordPair([r::RecordValue(0);2]);2] }
    fn finish_primary(&mut self, buffer: Buffer) { self.0.push(Event::FinishPrimary(buffer)); }
    fn finish_capture(&mut self) { self.0.push(Event::FinishCapture); }
    fn release(&mut self, source: r::Source) { self.0.push(Event::Release(source)); }
    fn reply(&mut self, reply: r::Reply<Buffer>) { self.0.push(Event::Record(reply)); }
}
impl c::Port for Peer {
    fn minimum_exposure(&mut self) -> s::RecordWord { s::RecordWord(0) }
    fn maximum_exposure(&mut self) -> s::RecordWord { s::RecordWord(1) }
    fn policy_dimensions(&mut self, _: s::RecordWord) -> c::Dimensions { c::Dimensions { width: s::RecordHalfword(1), height: s::RecordHalfword(1) } }
    fn encode_position(&mut self, _: a::LensPosition) -> u8 { 0 }
    fn split_height(&mut self, height: s::RecordHalfword) -> c::HeightSplit { c::HeightSplit { height, excess: s::RecordHalfword(0) } }
    fn policy_enabled(&mut self, _: s::RecordWord) -> bool { false }
    fn setup_source(&mut self, _: c::SourceSlot, _: s::RecordWord, _: s::RecordWord) { panic!("unexpected capture source") }
    fn request_capture(&mut self, packet: &[u8;176]) { self.0.push(Event::Capture(*packet)); }
}
impl Port<Buffer> for Peer {
    fn start_record(&mut self, packet: &[u8;68]) { self.0.push(Event::Start(*packet)); }
    fn update_record(&mut self, packet: &[u8;20]) { self.0.push(Event::Update(*packet)); }
    fn release_parameters(&mut self) { self.0.push(Event::ParametersReleased); }
    fn capture_value(&mut self) -> r::RecordValue { self.0.push(Event::Query(Query::Capture)); r::RecordValue(7) }
}
fn callback(word: u32) -> ClientCallback { stock::optional_callback(word).unwrap() }

#[test]
fn record_completion_and_notification_do_not_release_the_lens_client() {
    let mut session = Exports::new();
    let mut peer = Peer::default();
    session.request(Request::Reserve(callback(1)), &mut peer);
    session.request(Request::StartRecord {
        callback: callback(2), buffer: Buffer(9), completion: s::StockCallbackWord(3),
        record: s::RecordStart {
            channel: s::RecordWord(1), mode: s::RecordMode::Default, settings: [s::RecordWord(0);2],
            range: [s::RecordHalfword(0);2], source: s::RecordWord(0),
            sample: s::RecordSample { leading: s::RecordWord(0), components: [s::RecordHalfword(0);4], trailing: s::RecordWord(0) },
        },
    }, &mut peer);
    peer.0.clear();
    let values = [r::RecordPair([r::RecordValue(12),r::RecordValue(13)]);3];
    session.completed(Completion::Primary { status: CompletionStatus(4), values }, &mut peer).unwrap();
    session.request(Request::Notify(Some(callback(5))), &mut peer);
    session.request(Request::Lens { callback: callback(6), request: a::LensRequest::Stop }, &mut peer);
    session.completed(Completion::Lens(a::Completion::Finished), &mut peer).unwrap();
    session.request(Request::Lens { callback: callback(6), request: a::LensRequest::Stop }, &mut peer);
    assert_eq!(peer.0, [
        Event::FinishPrimary(Buffer(9)), Event::Record(r::Reply::Primary { callback: callback(2), status: CompletionStatus(4), buffer: Buffer(9), values }),
        Event::Reservation(y::Effect::Reply { callback: callback(5), status: CompletionStatus(0) }),
        Event::Lens(a::Effect::Reply { callback: callback(6), result: a::Reply::Busy }),
        Event::Lens(a::Effect::Reply { callback: callback(1), result: a::Reply::Success }),
        Event::Lens(a::Effect::Request(a::LensRequest::Stop)),
    ]);
}

#[test]
fn query_exports_return_the_selected_service_values_and_release_only_parameters() {
    let mut session = Exports::<Buffer>::new();
    let mut peer = Peer::default();
    assert_eq!(session.request(Request::LensPosition, &mut peer), Value::Position(y::LensPositionCode(37)));
    assert_eq!(session.request(Request::LensMotion, &mut peer), Value::Motion(a::LensPosition(-4)));
    assert_eq!(session.request(Request::PrimaryImageStatus, &mut peer), Value::Image(y::ImageStatus(5)));
    assert_eq!(session.request(Request::SecondaryImageStatus, &mut peer), Value::Image(y::ImageStatus(6)));
    assert_eq!(session.request(Request::CaptureValue, &mut peer), Value::Capture(r::RecordValue(7)));
    assert_eq!(session.request(Request::MovieStatus(y::MoviePhase::Primary), &mut peer), Value::Movie(y::Readiness::Ready));
    assert_eq!(session.request(Request::MovieStatus(y::MoviePhase::Secondary), &mut peer), Value::Movie(y::Readiness::Ready));
    assert_eq!(session.request(Request::SelectionRejected, &mut peer), Value::SelectionRejected(false));
    session.request(Request::ReleaseParameters, &mut peer);
    assert_eq!(peer.0, [Event::Query(Query::PositionAvailability),Event::Query(Query::Position),
        Event::Query(Query::MotionAvailability),Event::Query(Query::Motion),Event::Query(Query::ImagePrimary),
        Event::Query(Query::ImageSecondary),Event::Query(Query::Capture),Event::Query(Query::Movie(y::MoviePhase::Primary)),
        Event::Query(Query::Movie(y::MoviePhase::Secondary)),Event::ParametersReleased]);
}
