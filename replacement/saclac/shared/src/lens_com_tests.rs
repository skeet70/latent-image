use std::cell::RefCell;

use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_interface::{LensCom, LensComOperation, LensComRequest};

use crate::lens_com::*;

#[derive(Debug, PartialEq)]
enum Event {
    Remembered(Option<u32>, u8),
    Completed(u32, u8),
    Posted(LensComOperation, Vec<u8>),
    Unposted,
}

type Log = RefCell<Vec<Event>>;

const COMPLETION: StockCallbackWord = StockCallbackWord(0x006b_6f9c);

struct Client<'a>(&'a Log);

impl LensComClient for Client<'_> {
    fn remember(&mut self, callback: Option<StockCallbackWord>, number: RequestNumber) {
        self.0.borrow_mut().push(Event::Remembered(callback.map(|callback| callback.0), number.get()))
    }

    fn completion(&self) -> StockCallbackWord { COMPLETION }

    fn complete(&mut self, callback: StockCallbackWord, number: RequestNumber) {
        self.0.borrow_mut().push(Event::Completed(callback.0, number.get()))
    }

    fn report_unposted(&mut self) { self.0.borrow_mut().push(Event::Unposted) }
}

struct Peer<'a> {
    log: &'a Log,
    refuse: bool,
}

impl LensCom for Peer<'_> {
    type Error = ();

    fn request(&mut self, request: LensComRequest<'_>) -> Result<(), ()> {
        self.log.borrow_mut().push(Event::Posted(request.operation, request.parameters.bytes().to_vec()));
        if self.refuse { Err(()) } else { Ok(()) }
    }
}

const CALLBACK: StockCallbackWord = StockCallbackWord(0x0102_0304);

fn run(number: u32, callback: Option<StockCallbackWord>, refuse: bool) -> Vec<Event> {
    let log = Log::default();
    request(RequestNumber::new(number).unwrap(), callback, &mut Client(&log), &mut Peer { log: &log, refuse });
    log.into_inner()
}

#[test]
fn odd_numbers_and_zero_post_their_code_to_lenscom() {
    for (number, code) in [(0, 4), (1, 0), (3, 1), (5, 2), (7, 3), (9, 3)] {
        let events = run(number, Some(CALLBACK), false);
        let [Event::Remembered(remembered, stored), Event::Posted(operation, bytes)] = events.as_slice() else { panic!("{events:?}") };
        assert_eq!((*remembered, *stored), (Some(CALLBACK.0), number as u8));
        assert_eq!(*operation, LensComOperation::Request2d);
        assert_eq!(&bytes[..4], &COMPLETION.0.to_le_bytes());
        assert_eq!(bytes[6], code, "{number}");
    }
}

#[test]
fn even_numbers_complete_at_once_without_lenscom() {
    for number in [2, 4, 6, 8, 10] {
        assert_eq!(run(number, Some(CALLBACK), false), [Event::Completed(CALLBACK.0, number as u8)]);
        assert_eq!(run(number, None, false), []);
    }
}

#[test]
fn a_posted_request_without_a_callback_still_clears_the_stored_one() {
    assert_eq!(run(1, None, false)[0], Event::Remembered(None, 1));
}

#[test]
fn a_request_lenscom_refuses_is_reported() {
    let events = run(3, Some(CALLBACK), true);
    assert_eq!(events.last(), Some(&Event::Unposted));
}

#[test]
fn numbers_above_ten_are_not_requests() {
    assert_eq!(RequestNumber::new(10).map(RequestNumber::get), Some(10));
    assert_eq!(RequestNumber::new(11), None);
}
