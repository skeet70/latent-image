use crate::search::{BridgePhase, BridgeReport, BridgeRequest, BridgeStatus, LensClass};
use crate::stock::search::{bridge_report, bridge_request};

#[test]
fn a_bridge_request_lays_out_phase_window_travel_and_speed() {
    let request = BridgeRequest {
        phase: BridgePhase::PeakSearch,
        scanning: true,
        window: [0x1234, 0x5678],
        travel: -3,
        rate: 0x0a0b0c0d,
        moves: true,
        speed: 2,
        lens_mode: 1,
    };
    assert_eq!(
        bridge_request(&request),
        [
            1, 0, 0, 0, 1, 0, 0, 0, 0x34, 0x12, 0x78, 0x56, 0xfd, 0xff, 0, 0, 0x0d, 0x0c, 0x0b, 0x0a, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0,
            0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
}

#[test]
fn bridge_phases_take_the_index_stock_sends() {
    let index = |phase| bridge_request(&BridgeRequest { phase, scanning: false, window: [0; 2], travel: 0, rate: 0, moves: false, speed: 0, lens_mode: 0 })[0];
    let phases = [
        BridgePhase::Direction,
        BridgePhase::PeakSearch,
        BridgePhase::Peak,
        BridgePhase::Return,
        BridgePhase::Focus,
        BridgePhase::FocusDrive,
        BridgePhase::Approach,
        BridgePhase::Monitor,
        BridgePhase::Other,
    ];
    for (expected, phase) in phases.into_iter().enumerate() {
        assert_eq!(usize::from(index(phase)), expected);
    }
}

#[test]
fn a_bridge_report_parses_status_position_and_class() {
    let payload = [0xa2, 0xff, 0xff, 0xff, 0x10, 0x80, 0, 0, 2, 0, 0, 0];
    assert_eq!(bridge_report(&payload), Some(BridgeReport { status: BridgeStatus::Fault, position: -32752, class: LensClass(2) }));
    assert_eq!(bridge_report(&[21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).map(|report| report.status), Some(BridgeStatus::Deferred));
    assert_eq!(bridge_report(&payload[..11]), None);
}
