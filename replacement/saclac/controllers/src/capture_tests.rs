use crate::{capture::*, stock::{RecordHalfword as H, RecordWord as W}};
use xm1_saclac_interface::stock::StockCallbackWord as C;
use std::vec::Vec;

#[derive(Debug, PartialEq)]
enum Event {
    Minimum,
    Maximum,
    Dimensions,
    Position,
    Split(u16),
    Policy,
    Setup(SourceSlot, W, W),
    Request([u8; 176]),
}

struct Peer { events: Vec<Event> }
impl Port for Peer {
    fn minimum_exposure(&mut self) -> W { self.events.push(Event::Minimum); W(10) }
    fn maximum_exposure(&mut self) -> W { self.events.push(Event::Maximum); W(2) }
    fn policy_dimensions(&mut self, _: W) -> Dimensions {
        self.events.push(Event::Dimensions);
        Dimensions { width: H(640), height: H(480) }
    }
    fn encode_position(&mut self, _: crate::adjustment::LensPosition) -> u8 {
        self.events.push(Event::Position); 7
    }
    fn split_height(&mut self, height: H) -> HeightSplit {
        self.events.push(Event::Split(height.0));
        HeightSplit { height: H(height.0.min(100)), excess: H(height.0.saturating_sub(100)) }
    }
    fn policy_enabled(&mut self, _: W) -> bool { self.events.push(Event::Policy); true }
    fn setup_source(&mut self, slot: SourceSlot, identity: W, length: W) {
        self.events.push(Event::Setup(slot, identity, length));
    }
    fn request_capture(&mut self, packet: &[u8; 176]) { self.events.push(Event::Request(*packet)); }
}

fn input() -> Capture {
    Capture {
        channel: W(23), dimensions: Dimensions { width: H(300), height: H(150) },
        position: crate::adjustment::LensPosition(-20), policy: W(35), option: 9,
        sources: [
            Source { identity: W(1), parameters: [W(2), W(3), W(4)], length: W(5) },
            Source { identity: W(6), parameters: [W(7), W(8), W(9)], length: W(10) },
            Source { identity: W(11), parameters: [W(12), W(13), W(14)], length: W(15) },
        ],
        source_option: 17, exposure: W(50), auxiliary: W(41), mode: CaptureMode::Ordinary,
        multiple_frames: true,
    }
}

fn callbacks() -> Callbacks {
    Callbacks { completion: C(0x11), sample: C(0x22), auxiliary: C(0x33), progress: C(0x44) }
}

#[test]
fn capture_converts_then_registers_sources_before_image_request() {
    let mut peer = Peer { events: Vec::new() };
    start(input(), callbacks(), &mut peer);
    assert_eq!(&peer.events[..9], &[
        Event::Minimum, Event::Minimum, Event::Dimensions, Event::Position,
        Event::Split(150), Event::Split(150), Event::Policy,
        Event::Setup(SourceSlot::Primary, W(1), W(5)),
        Event::Setup(SourceSlot::Tertiary, W(11), W(15)),
    ]);
    let Event::Request(packet) = &peer.events[9] else { panic!("missing image request") };
    let mut expected = [0_u8; 176];
    expected[..24].copy_from_slice(&[17,0,0,0,51,0,0,0,34,0,0,0,0,0,0,0,68,0,0,0,0,0,0,0]);
    expected[24..60].copy_from_slice(&[
        1,0,0,0,2,0,0,0,3,0,0,0,4,0,0,0,6,0,0,0,7,0,0,0,8,0,0,0,9,0,0,0,23,0,0,0,
    ]);
    expected[60..64].copy_from_slice(&[44,1,150,0]);
    expected[84..88].copy_from_slice(&[128,2,224,1]);
    expected[96] = 2;
    expected[100] = 23;
    expected[104] = 35;
    expected[116] = 41;
    expected[128..132].copy_from_slice(&[100,0,16,0]);
    expected[136..138].copy_from_slice(&[236,255]);
    expected[140..150].copy_from_slice(&[44,1,100,0,50,0,100,0,50,0]);
    expected[161..167].copy_from_slice(&[7,17,0,9,1,1]);
    expected[171..173].copy_from_slice(&[1,1]);
    assert_eq!(*packet, expected);
    assert_eq!(peer.events.len(), 10);
}

#[test]
fn mode_six_skips_exposure_queries_and_absent_sources() {
    let mut request = input();
    request.mode = CaptureMode::ModeSix;
    request.sources[0].identity = W(0);
    request.sources[2].identity = W(0);
    let mut peer = Peer { events: Vec::new() };
    start(request, callbacks(), &mut peer);
    assert_eq!(&peer.events[..5], &[Event::Dimensions, Event::Position, Event::Split(150), Event::Split(150), Event::Policy]);
    let Event::Request(packet) = &peer.events[5] else { panic!("missing image request") };
    assert_eq!(packet[171], 0);
    assert_eq!(peer.events.len(), 6);
}

#[test]
fn exposure_bounds_are_observed_without_replacing_request_fields() {
    for (exposure, queries) in [
        (1000, std::vec![Event::Minimum, Event::Maximum]),
        (2_000_001, std::vec![Event::Minimum, Event::Maximum, Event::Maximum]),
    ] {
        let mut request = input();
        request.exposure = W(exposure);
        let mut peer = Peer { events: Vec::new() };
        start(request, callbacks(), &mut peer);
        assert_eq!(&peer.events[..queries.len()], queries.as_slice());
    }
}
