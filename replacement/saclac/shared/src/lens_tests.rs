use xm1_saclac_interface::shared::{MoveMode, RelativeMove};
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{Completion, LensDc, LensDcOperation, LensDcRequest};

use crate::lens::*;

#[derive(Default)]
struct Notifier {
    enabled: bool,
    word: Option<StockCallbackWord>,
    entered: Vec<(u32, u32, LensBounds)>,
}

impl LensNotices for Notifier {
    fn notice(&self, _: Notice) -> u32 { 0 }

    fn clear_notices(&mut self) {}

    fn notifier_enabled(&self) -> bool { self.enabled }

    fn notifier(&self) -> Option<StockCallbackWord> { self.word }

    fn enter_notifier(&mut self, notifier: StockCallbackWord, detail: u32, bounds: LensBounds) {
        self.entered.push((notifier.0, detail, bounds))
    }
}

const BOUNDS: LensBounds = LensBounds([10, 20, 30]);

#[test]
fn bounds_reach_a_set_and_enabled_notifier() {
    let mut notices = Notifier { enabled: true, word: Some(StockCallbackWord(0x0123_4567)), ..Notifier::default() };
    notify_bounds(5, BOUNDS, &mut notices);
    assert_eq!(notices.entered, [(0x0123_4567, 5, BOUNDS)]);
}

#[test]
fn bounds_need_both_an_enabled_and_a_set_notifier() {
    for (enabled, word) in [(false, Some(StockCallbackWord(0x0123_4567))), (true, None), (false, None)] {
        let mut notices = Notifier { enabled, word, ..Notifier::default() };
        notify_bounds(5, BOUNDS, &mut notices);
        assert!(notices.entered.is_empty());
    }
}

/// Zone k - 1 ends below 100 * k.
struct Zones;

impl LensControl for Zones {
    fn probe(&mut self, _: LensProbe) -> ProbeState { ProbeState::Clear }

    fn value(&mut self) -> u32 { 0 }

    fn step(&mut self) -> u32 { 0 }

    fn prefers_relative(&mut self) -> bool { false }

    fn zone_limit(&mut self, zone: u8) -> i16 { 100 * i16::from(zone) }
}

#[test]
fn a_lens_value_falls_in_the_first_zone_ending_above_it() {
    for (value, zone) in [(0, 0), (1, 1), (-50, 1), (199, 1), (200, 2), (550, 5), (999, 9), (1000, 10), (i32::MAX, 10)] {
        assert_eq!(crate::lens::zone(value, &mut Zones).get(), zone, "{value}");
    }
}

#[derive(Default)]
struct LensDcRecorder {
    requests: Vec<(LensDcOperation, Vec<u8>, Vec<Completion>)>,
    refuse: bool,
}

impl LensDc for LensDcRecorder {
    type Error = ();

    fn request(&mut self, request: LensDcRequest<'_>) -> Result<(), ()> {
        self.requests.push((request.operation, request.parameters.bytes().to_vec(), request.completions.to_vec()));
        if self.refuse { Err(()) } else { Ok(()) }
    }
}

#[test]
fn a_relative_move_posts_its_payload_with_the_callers_completions() {
    let mut lens = LensDcRecorder::default();
    let request = RelativeMove { callback: StockCallbackWord(0x00f2_8e94), step: 12, mode: MoveMode::Second, words: [1, 0, 5, 4] };
    assert_eq!(move_relative(&request, &[Completion::Lens33, Completion::Lens34], &mut lens), Ok(()));
    let (operation, bytes, completions) = &lens.requests[0];
    assert_eq!(*operation, LensDcOperation::Request0b);
    assert_eq!(bytes.as_slice(), xm1_saclac_interface::stock::shared::relative_move(&request).as_slice());
    assert_eq!(completions.as_slice(), [Completion::Lens33, Completion::Lens34]);
}

#[test]
fn a_refused_relative_move_reports_the_refusal() {
    let mut lens = LensDcRecorder { refuse: true, ..LensDcRecorder::default() };
    let request = RelativeMove { callback: StockCallbackWord(1), step: -1, mode: MoveMode::First, words: [0; 4] };
    assert_eq!(move_relative(&request, &[], &mut lens), Err(()));
}

struct Waiter<'a> {
    log: &'a std::cell::RefCell<Vec<String>>,
}

impl Rendezvous for Waiter<'_> {
    fn arm(&mut self) -> StockCallbackWord {
        self.log.borrow_mut().push("arm".into());
        StockCallbackWord(0x00f4_6be0)
    }

    fn wait(&mut self) { self.log.borrow_mut().push("wait".into()) }
}

#[test]
fn a_synchronization_arms_posts_then_waits() {
    let log = std::cell::RefCell::new(Vec::new());
    let mut lens = LensDcRecorder::default();
    assert_eq!(synchronize(&mut lens, &mut Waiter { log: &log }), Ok(()));
    assert_eq!(*log.borrow(), ["arm", "wait"]);
    let (operation, bytes, completions) = &lens.requests[0];
    assert_eq!(*operation, LensDcOperation::Request17);
    assert_eq!(bytes.as_slice(), [0xe0, 0x6b, 0xf4, 0, 0, 0, 0, 0]);
    assert!(completions.is_empty());
}

#[test]
fn a_synchronization_lensdc_refuses_does_not_wait() {
    let log = std::cell::RefCell::new(Vec::new());
    let mut lens = LensDcRecorder { refuse: true, ..LensDcRecorder::default() };
    assert_eq!(synchronize(&mut lens, &mut Waiter { log: &log }), Err(()));
    assert_eq!(*log.borrow(), ["arm"]);
}
