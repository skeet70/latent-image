use super::*;
use crate::wireless_association::station::{Cipher, InformationElements, PersonalProtocol, ScanBatch, ScanCandidate, SelectedAccessPoint};
use crate::vendor_wireless_network::SignalDbm;
use crate::vendor_wireless_service_task::{TaskMessageKind, TaskPayload};

fn configured() -> (ModeRuntime, StationRuntime) {
    let mut service = ModeRuntime::new();
    let ssid = Ssid::parse(b"camera").unwrap();
    let profile = StationProfile::parse(ssid, 0, b"", 0).unwrap();
    service.configure(profile, Ssid::parse(b"FUJIFILM").unwrap()).unwrap();
    (service, StationRuntime::new(MacAddress([2, 0, 0, 0, 0, 1])))
}
fn message(kind: u16) -> TaskMessage { TaskMessage::new(TaskMessageKind(kind), TaskPayload::parse(&[0; 200]).unwrap()) }
fn finish_scan(station: &mut StationRuntime, protected: bool) {
    let command = station.poll().unwrap();
    let mut batch = ScanBatch::new();
    if protected {
        batch.append(ScanCandidate { ssid: Ssid::parse(b"FUJIFILM-camera").unwrap(), privacy: true, signal: SignalDbm(-30),
            mode: crate::vendor_wireless_network::NetworkMode::Infrastructure, key_management: crate::vendor_wireless_network::KeyManagement::Unspecified,
            access_point: SelectedAccessPoint { address: MacAddress([2, 0, 0, 0, 0, 2]), protocol: PersonalProtocol::Rsn, pairwise: Cipher::Ccmp,
                group: Cipher::Ccmp, advertised_elements: InformationElements::parse(&[]).unwrap() } }).unwrap();
    }
    station.receive(command.scanned(batch).unwrap()).unwrap();
}
#[test]
fn scan_completion_cannot_be_injected_by_message_kind() {
    let (mut service, mut station) = configured();
    assert!(service.process(&message(0x6615), &mut station, Milliseconds(0)).unwrap());
    assert!(!service.process(&message(0x6627), &mut station, Milliseconds(0)).unwrap());
    assert_eq!(service.state(), ModeState::Scanning);
    assert_eq!(service.notification(), None);
    finish_scan(&mut station, true);
    assert!(service.poll_station(&mut station, Milliseconds(10)).unwrap());
    assert_eq!(service.state(), ModeState::Discovering);
    assert!(service.notification().is_none());
}
#[test]
fn empty_scan_timeout_publishes_not_found_before_returning_idle() {
    let (mut service, mut station) = configured();
    service.start(&mut station, Milliseconds(0)).unwrap();
    finish_scan(&mut station, false);
    assert!(!service.poll_station(&mut station, Milliseconds(110_000)).unwrap());
    assert_eq!(service.notification(), None);
    service.poll_station(&mut station, Milliseconds(110_500)).unwrap();
    assert_eq!(service.configuration_status(), ConfigurationStatus::NotFound);
    assert_eq!(service.state(), ModeState::Idle);
    assert_eq!(service.notification(), Some(ModeNotification { target: ModeCompletionTarget::Configure, status: CompletionStatus::Success, state: ModeState::Idle }));
}
#[test]
fn listing_cancellation_waits_for_actual_scan_and_publishes_reset() {
    let (mut service, mut station) = configured();
    service.list(&mut station).unwrap();
    assert!(service.cancel(&mut station).unwrap());
    assert_eq!(service.state(), ModeState::Cancelling);
    assert_eq!(service.notification(), None);
    finish_scan(&mut station, false);
    service.poll_station(&mut station, Milliseconds(1)).unwrap();
    assert_eq!(service.state(), ModeState::Idle);
    assert_eq!(service.notification().unwrap().target, ModeCompletionTarget::Reset);
    assert_eq!(service.notification(), None);
    assert!(!service.poll_station(&mut station, Milliseconds(2)).unwrap());
}
#[test]
fn listing_result_is_not_a_configuration_completion() {
    let (mut service, mut station) = configured();
    service.list(&mut station).unwrap();
    finish_scan(&mut station, false);
    service.poll_station(&mut station, Milliseconds(0)).unwrap();
    let notification = service.notification().unwrap();
    assert_eq!(notification.target, ModeCompletionTarget::Reset);
    assert_eq!(notification.state, ModeState::Listing);
    assert_eq!(service.state(), ModeState::Idle);
}
#[test]
fn busy_start_does_not_replace_pending_operation() {
    let (mut service, mut station) = configured();
    assert!(service.start(&mut station, Milliseconds(0)).unwrap());
    assert!(!service.connect(&mut station).unwrap());
    assert_eq!(service.state(), ModeState::Scanning);
    finish_scan(&mut station, false);
    service.poll_station(&mut station, Milliseconds(110_000)).unwrap();
    service.poll_station(&mut station, Milliseconds(110_500)).unwrap();
    assert_eq!(service.notification().unwrap().target, ModeCompletionTarget::Configure);
}
