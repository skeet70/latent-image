use super::{Adapter, AdapterError, AdapterEvent, AddressConfiguration, CompletionDisposition, ConfigurationDisposition, CountryCode, FrameOwner, FrameStatus, HardwareConfiguration, HardwareRevision, PendingQueue, PhysicalWrite, RadioCapabilities, RequestId};
use super::super::vendor_wireless_network::{MacAddress, Ssid};
use super::protocols::{ProtocolTimer, Protocols, ScanRecord, TimerCompletion, TimerDeadline};
use super::regulatory::RegulatoryInput;

fn configuration() -> HardwareConfiguration {
    HardwareConfiguration {
        first_revision: HardwareRevision { major: 0, minor: 0x107 },
        second_revision: HardwareRevision { major: 0, minor: 1 },
        use_calibration: true, calibration: [0xa5; 40],
        feature_requested: true, feature_supported: true, feature_disabled: false,
        country: CountryCode(0x5553), radio_parameters: [0x6b; 144],
        regulatory: RegulatoryInput::country([4, 5]),
        antenna: Some([1, 2, 3]),
    }
}

#[test]
fn pending_teardown_flattens_station_queues_before_unclassified_and_returns_buffers() {
    let mut one = [1, 2];
    let mut two = [3, 4];
    let mut three = [5, 6];
    let mut adapter = Adapter::<3, 2>::new();
    let tail = adapter.enqueue_frame(FrameOwner::Supplicant, &mut one).unwrap();
    let later = adapter.enqueue_frame(FrameOwner::Network, &mut two).unwrap();
    let first = adapter.enqueue_frame(FrameOwner::Network, &mut three).unwrap();
    adapter.mark_pending(tail).unwrap();
    adapter.mark_pending_in(later, PendingQueue::station(19, 4).unwrap()).unwrap();
    adapter.mark_pending_in(first, PendingQueue::station(0, 0).unwrap()).unwrap();
    adapter.cancel_pending_frames();
    assert_eq!(adapter.publication().outstanding_network, 0);
    assert_eq!(adapter.publication().outstanding_supplicant, 0);
    for (identity, bytes) in [(first, [5, 6]), (later, [3, 4]), (tail, [1, 2])] {
        let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing completion") };
        assert_eq!(completion.identity, identity);
        assert_eq!(completion.status, FrameStatus::PendingCancelled);
        assert_eq!(completion.bytes, &bytes);
    }
    assert!(adapter.pop_event().is_none());
    assert_eq!(adapter.complete_frame(first, FrameStatus::Sent), CompletionDisposition::Late);
}

#[test]
fn descriptor_filters_preserve_other_owners_until_their_teardown() {
    let mut bytes = [8, 9, 10];
    let mut adapter = Adapter::<1, 4>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    let descriptor = adapter.enqueue_frame_descriptor(frame).unwrap();
    adapter.enqueue_request(RequestId(42)).unwrap();
    adapter.configure_address(AddressConfiguration { address: Some(MacAddress([2, 1, 2, 3, 4, 5])), preserve_invalid: false, timestamp: 0, capabilities: RadioCapabilities(0) }).unwrap();
    adapter.cancel_commands();
    assert!(matches!(adapter.pop_event(), Some(AdapterEvent::Request { identity: RequestId(42), cancelled: true })));
    assert!(adapter.pop_event().is_none());
    assert_eq!(adapter.publication().outstanding_network, 1);
    adapter.cancel_received_descriptors();
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.status, FrameStatus::DescriptorCancelled);
    assert_eq!(completion.bytes, &[8, 9, 10]);
    assert_eq!(adapter.complete_descriptor(descriptor), CompletionDisposition::Late);
    let identity = adapter.command().unwrap().identity();
    adapter.finish_command(identity, PhysicalWrite::Written);
    assert_eq!(adapter.publication().address, MacAddress([2, 1, 2, 3, 4, 5]));
}

#[test]
fn shutdown_rejects_late_address_write_without_publishing_it() {
    let mut adapter = Adapter::<0, 1>::new();
    adapter.configure_address(AddressConfiguration { address: None, preserve_invalid: false, timestamp: 0x12345678, capabilities: RadioCapabilities(0x7f) }).unwrap();
    let command = adapter.command().unwrap();
    let identity = command.identity();
    assert_eq!(command.argument(), 0x9000_5814);
    assert_eq!(&command.bytes()[8..14], &[0, 8, 0x22, 0x78, 0x56, 0x34]);
    assert_eq!(&command.bytes()[16..20], &[7, 0, 7, 0]);
    adapter.cancel();
    assert_eq!(adapter.finish_command(identity, PhysicalWrite::Written), CompletionDisposition::Late);
    assert_eq!(adapter.publication().address, MacAddress([0; 6]));
    assert!(adapter.command().is_none());
}

#[test]
fn invalid_preserved_address_produces_no_transfer() {
    let mut adapter = Adapter::<0, 1>::new();
    for address in [None, Some(MacAddress([0; 6])), Some(MacAddress([1, 2, 3, 4, 5, 6]))] {
        assert_eq!(adapter.configure_address(AddressConfiguration { address, preserve_invalid: true, timestamp: 0, capabilities: RadioCapabilities(0) }), Ok(ConfigurationDisposition::Unchanged));
        assert!(adapter.command().is_none());
    }
}

#[test]
fn configuration_transmits_calibration_radio_rates_and_antenna_in_order() {
    let mut adapter = Adapter::<0, 4>::new();
    adapter.configure(&configuration()).unwrap();
    assert_eq!(adapter.publication().country, CountryCode(0x5553));
    assert!(adapter.publication().feature_enabled);
    for (opcode, length) in [(0x28, 48), (0x31, 152), (0x13, 64), (0x36, 12)] {
        let command = adapter.command().unwrap();
        assert_eq!(command.bytes()[4], opcode);
        assert_eq!(command.bytes().len(), length);
        if opcode == 0x13 {
            assert_eq!(&command.bytes()[8..12], &[0x53, 0x55, 0, 0]);
            assert_eq!(&command.bytes()[12..20], &[81, 1, 1, 1, 11, 0, 0, 0]);
            assert_eq!(&command.bytes()[60..64], &[4, 5, 0, 0]);
        }
        if opcode == 0x36 { assert_eq!(&command.bytes()[8..12], &[1, 2, 3, 0]); }
        let identity = command.identity();
        assert_eq!(adapter.finish_command(identity, PhysicalWrite::Failed), CompletionDisposition::Applied);
    }
    assert_eq!(adapter.last_physical(), Some(PhysicalWrite::Failed));
    assert!(adapter.command().is_none());
}

#[test]
fn configuration_rejects_revision_and_capacity_without_partial_publication() {
    let mut adapter = Adapter::<0, 3>::new();
    let mut config = configuration();
    config.second_revision.minor = 0x108;
    assert_eq!(adapter.configure(&config), Err(AdapterError::InvalidHardwareRevision));
    config.second_revision.minor = 0x107;
    assert_eq!(adapter.configure(&config), Err(AdapterError::DescriptorCapacity));
    assert_eq!(adapter.publication().country, CountryCode(0));
    assert!(!adapter.publication().feature_enabled);
    assert!(adapter.command().is_none());
}

#[test]
fn released_frame_slot_rejects_old_completions_after_reuse() {
    let mut first = [1];
    let mut second = [2];
    let mut adapter = Adapter::<1, 1>::new();
    let old = adapter.enqueue_frame(FrameOwner::Network, &mut first).unwrap();
    adapter.cancel_queued_frames();
    assert!(matches!(adapter.pop_event(), Some(AdapterEvent::Frame(_))));
    let current = adapter.enqueue_frame(FrameOwner::Supplicant, &mut second).unwrap();
    assert_ne!(old, current);
    assert_eq!(adapter.complete_frame(old, FrameStatus::Sent), CompletionDisposition::Late);
    assert_eq!(adapter.publication().outstanding_supplicant, 1);
    assert_eq!(adapter.complete_frame(current, FrameStatus::Sent), CompletionDisposition::Applied);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing completion") };
    assert_eq!(completion.owner, FrameOwner::Supplicant);
    assert_eq!(completion.bytes, &[2]);
}

#[test]
fn stop_cancels_registered_station_timers_and_invalidates_scan_visibility() {
    let mut protocols = Protocols::new();
    let timers = [ProtocolTimer::Background, ProtocolTimer::Discovery, ProtocolTimer::Authentication, ProtocolTimer::Association, ProtocolTimer::Scan, ProtocolTimer::StationZero, ProtocolTimer::StationOne, ProtocolTimer::StationTwo];
    let identities = timers.map(|timer| protocols.arm(timer, TimerDeadline(100)).unwrap());
    let scan = protocols.remember_scan(ScanRecord { address: MacAddress([2; 6]), ssid: Ssid::parse(b"synthetic").unwrap() }).unwrap();
    protocols.stop();
    for (timer, identity) in timers.into_iter().zip(identities) {
        assert_eq!(protocols.deadline(timer), None);
        assert_eq!(protocols.complete_timer(identity), TimerCompletion::Late);
    }
    assert!(protocols.scan(scan).is_err());
    protocols.reset(MacAddress([2, 3, 4, 5, 6, 7]));
    assert!(protocols.scan(scan).is_err());
    assert_eq!(protocols.template(), Some((MacAddress([2, 3, 4, 5, 6, 7]), [0; 3])));
}

#[test]
fn rearmed_timer_ignores_previous_generation() {
    let mut protocols = Protocols::new();
    let first = protocols.arm(ProtocolTimer::Discovery, TimerDeadline(100)).unwrap();
    let second = protocols.arm(ProtocolTimer::Discovery, TimerDeadline(200)).unwrap();
    assert_eq!(protocols.complete_timer(first), TimerCompletion::Late);
    assert_eq!(protocols.deadline(ProtocolTimer::Discovery), Some(TimerDeadline(200)));
    assert_eq!(protocols.complete_timer(second), TimerCompletion::Current);
}

#[test]
fn sequence_is_assigned_at_transfer_after_shared_counter_handoff() {
    let mut adapter = Adapter::<0, 4>::new();
    adapter.configure(&configuration()).unwrap();
    assert!(adapter.has_pending_commands());
    adapter.set_last_sequence(254);
    let command = adapter.command().unwrap();
    assert_eq!(command.bytes()[6], 255);
    let identity = command.identity();
    assert_eq!(adapter.command().unwrap().bytes()[6], 255);
    adapter.finish_command(identity, PhysicalWrite::Written);
    assert_eq!(adapter.last_sequence(), 255);
    assert_eq!(adapter.command().unwrap().bytes()[6], 0);
}

#[test]
fn credit_exhaustion_does_not_report_a_drained_command_queue() {
    let mut adapter = Adapter::<0, 5>::new();
    adapter.configure(&configuration()).unwrap();
    adapter.configure_address(AddressConfiguration { address: Some(MacAddress([2; 6])), preserve_invalid: false, timestamp: 0, capabilities: RadioCapabilities(0) }).unwrap();
    for _ in 0..4 {
        let identity = adapter.command().unwrap().identity();
        adapter.finish_command(identity, PhysicalWrite::Written);
    }
    assert!(adapter.command().is_none());
    assert!(adapter.has_pending_commands());
    adapter.observe_credit_returns([0, 0, 0, 0, 1, 0, 0, 0]).unwrap();
    let identity = adapter.command().unwrap().identity();
    adapter.finish_command(identity, PhysicalWrite::Written);
    assert!(!adapter.has_pending_commands());
}

#[test]
fn custom_eeprom_band_serialization_omits_ranges_for_unsupported_bands() {
    let mut config = configuration();
    config.regulatory = RegulatoryInput::from_eeprom(2, 0, [[91, 7, 4, 36, 9, 255]; 6], [1, 2]);
    let mut adapter = Adapter::<0, 4>::new();
    adapter.configure(&config).unwrap();
    for _ in 0..2 {
        let identity = adapter.command().unwrap().identity();
        adapter.finish_command(identity, PhysicalWrite::Written);
    }
    let command = adapter.command().unwrap();
    assert_eq!(command.bytes()[4], 0x13);
    assert_eq!(&command.bytes()[12..20], &[91, 7, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&command.bytes()[60..64], &[1, 2, 0, 0]);
}

#[test]
fn owner_notification_reset_publishes_capabilities_and_automatic_rate() {
    use super::protocols::{OwnerCapabilities, OwnerRate};
    let mut adapter = Adapter::<0, 0>::new();
    adapter.protocols().reset_owner_notification();
    let state = adapter.protocols().owner_notification();
    assert_eq!(state.capabilities, OwnerCapabilities(0x13));
    assert_eq!(state.rate, OwnerRate::Automatic);
    assert!(!adapter.has_pending_commands());
}

#[test]
fn shutdown_retains_pending_events_until_every_owner_has_received_completion() {
    let mut bytes = [1, 2, 3];
    let mut adapter = Adapter::<1, 1>::new();
    adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    adapter.enqueue_request(RequestId(71)).unwrap();
    assert!(!adapter.has_pending_events());
    adapter.cancel();
    let mut saw_frame = false;
    let mut saw_request = false;
    for remaining in (0..2).rev() {
        match adapter.pop_event() {
            Some(AdapterEvent::Frame(_)) => saw_frame = true,
            Some(AdapterEvent::Request { identity: RequestId(71), cancelled: true }) => saw_request = true,
            _ => panic!("missing owner completion"),
        }
        assert_eq!(adapter.has_pending_events(), remaining != 0);
    }
    assert!(saw_frame && saw_request);
    assert!(!adapter.has_pending_events());
    assert!(adapter.pop_event().is_none());
}
