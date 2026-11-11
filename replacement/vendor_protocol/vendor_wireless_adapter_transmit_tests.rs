use super::{BssIndex, DataMetadata, DescriptorTag, DevicePacketId, DeviceTxStatus, FrameSequence, HeaderLength, Lifetime, ManagementMetadata, PacketKind, PacketPriority, RateSelection, RetryLimit, StationIndex, TransmitMetadata, UserPriority};
use super::super::{Adapter, AdapterError, AdapterEvent, CompletionDisposition, FirmwareRequest, FrameOwner, FrameStatus, PhysicalWrite, RadioCapabilities};
use super::super::super::vendor_wireless_parameter_driver::TrafficClass;

#[test]
fn ethernet_transmission_contains_actual_payload_and_data_fifo_header() {
    let mut bytes = [0x42; 18];
    let mut adapter = Adapter::<1, 1>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    adapter.enqueue_transmit_frame(frame, TransmitMetadata::Ethernet(DataMetadata { queue: TrafficClass::One, bss: BssIndex::parse(2).unwrap(), tag: DescriptorTag(6) })).unwrap();
    let command = adapter.command().unwrap();
    let identity = command.identity();
    assert_eq!(command.argument(), 0x9000_5024);
    assert_eq!(&command.bytes()[..16], &[34, 0, 14, 4, 14, 0x60, 0, 0, 0, 0, 6, 0x20, 0, 0, 0, 0]);
    assert_eq!(&command.bytes()[16..34], &[0x42; 18]);
    assert_eq!(&command.bytes()[34..36], &[0; 2]);
    adapter.finish_command(identity, PhysicalWrite::Failed);
    assert_eq!(adapter.last_sequence(), 0);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.status, FrameStatus::Sent);
    assert_eq!(completion.bytes, &[0x42; 18]);
    assert_eq!(adapter.publication().outstanding_network, 0);
}

#[test]
fn management_header_encodes_fields_and_releases_only_on_actual_pid_status() {
    let mut bytes = [0x33; 24];
    let mut adapter = Adapter::<1, 1>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Supplicant, &mut bytes).unwrap();
    let metadata = ManagementMetadata {
        priority: PacketPriority(5), queue: TrafficClass::Command, packet_kind: PacketKind::Two,
        station: StationIndex(9), bss: BssIndex::parse(2).unwrap(), user_priority: UserPriority(7),
        direct_link: true, sequence: FrameSequence(0x1234), lifetime: Lifetime(0xabcd),
        retry_limit: RetryLimit(1), rate: RateSelection(3), protected: true,
        insert_timestamp: true, burst: true, header_length: HeaderLength(24), extra_header_length: HeaderLength(2), report_status: true,
    };
    adapter.enqueue_transmit_frame(frame, TransmitMetadata::Management(metadata)).unwrap();
    let command = adapter.command().unwrap();
    let identity = command.identity();
    let pid = DevicePacketId::from_wire(command.bytes()[12]);
    assert_eq!(&command.bytes()[..16], &[40, 0x50, 21, 0x90, 24, 0xe7, 0x34, 0x12, 0xcd, 0xab, 9, 0x2d, pid.wire(), 7, 0, 0]);
    assert_eq!(&command.bytes()[16..40], &[0x33; 24]);
    adapter.finish_command(identity, PhysicalWrite::Written);
    assert!(adapter.pop_event().is_none());
    assert_eq!(adapter.publication().outstanding_supplicant, 1);
    let status = DeviceTxStatus::from_wire(0x9e);
    assert_eq!(adapter.complete_device_packet(pid, status), CompletionDisposition::Applied);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.status, FrameStatus::DeviceCompleted(status));
    assert_eq!(completion.bytes, &[0x33; 24]);
    assert_eq!(adapter.complete_device_packet(pid, status), CompletionDisposition::Late);
}

#[test]
fn early_device_completion_keeps_the_lease_until_physical_transfer_finishes() {
    let mut bytes = [1; 24];
    let mut adapter = Adapter::<1, 1>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Supplicant, &mut bytes).unwrap();
    adapter.enqueue_transmit_frame(frame, TransmitMetadata::Management(ManagementMetadata::management())).unwrap();
    let command = adapter.command().unwrap();
    let identity = command.identity();
    let pid = DevicePacketId::from_wire(command.bytes()[12]);
    assert_eq!(adapter.complete_device_packet(pid, DeviceTxStatus::from_wire(0)), CompletionDisposition::Applied);
    assert!(adapter.pop_event().is_none());
    adapter.finish_command(identity, PhysicalWrite::Written);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.status, FrameStatus::DeviceCompleted(DeviceTxStatus::from_wire(0)));
}

#[test]
fn aggregate_transmission_packs_distinct_pids_and_completes_each_owner() {
    let mut first = [1; 24];
    let mut second = [2; 24];
    let mut adapter = Adapter::<2, 2>::new();
    let a = adapter.enqueue_frame(FrameOwner::Supplicant, &mut first).unwrap();
    let b = adapter.enqueue_frame(FrameOwner::Supplicant, &mut second).unwrap();
    adapter.enqueue_transmit_frame(a, TransmitMetadata::Management(ManagementMetadata::management())).unwrap();
    adapter.enqueue_transmit_frame(b, TransmitMetadata::Management(ManagementMetadata::management())).unwrap();
    let command = adapter.command().unwrap();
    let identity = command.identity();
    let a_pid = DevicePacketId::from_wire(command.bytes()[12]);
    let b_pid = DevicePacketId::from_wire(command.bytes()[52]);
    assert_ne!(a_pid, b_pid);
    assert_eq!(command.bytes().len(), 80);
    assert_eq!(&command.bytes()[16..40], &[1; 24]);
    assert_eq!(&command.bytes()[56..80], &[2; 24]);
    adapter.finish_command(identity, PhysicalWrite::Written);
    assert!(!adapter.has_pending_commands());
    assert_eq!(adapter.complete_device_packet(b_pid, DeviceTxStatus::from_wire(3)), CompletionDisposition::Applied);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.identity, b);
    assert_eq!(completion.bytes, &[2; 24]);
    assert_eq!(adapter.complete_device_packet(a_pid, DeviceTxStatus::from_wire(0)), CompletionDisposition::Applied);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.identity, a);
}

#[test]
fn canceled_pid_does_not_complete_a_subsequent_frame() {
    let mut first = [1; 24];
    let mut second = [2; 24];
    let mut adapter = Adapter::<2, 2>::new();
    let a = adapter.enqueue_frame(FrameOwner::Supplicant, &mut first).unwrap();
    adapter.enqueue_transmit_frame(a, TransmitMetadata::Management(ManagementMetadata::management())).unwrap();
    let command = adapter.command().unwrap();
    let transfer = command.identity();
    let old_pid = DevicePacketId::from_wire(command.bytes()[12]);
    adapter.finish_command(transfer, PhysicalWrite::Written);
    adapter.cancel_pending_frames();
    assert!(matches!(adapter.pop_event(), Some(AdapterEvent::Frame(_))));
    let b = adapter.enqueue_frame(FrameOwner::Supplicant, &mut second).unwrap();
    adapter.enqueue_transmit_frame(b, TransmitMetadata::Management(ManagementMetadata::management())).unwrap();
    let command = adapter.command().unwrap();
    let transfer = command.identity();
    let new_pid = DevicePacketId::from_wire(command.bytes()[12]);
    assert_ne!(old_pid, new_pid);
    assert_eq!(adapter.complete_device_packet(old_pid, DeviceTxStatus::from_wire(0)), CompletionDisposition::Late);
    adapter.finish_command(transfer, PhysicalWrite::Written);
    assert!(adapter.pop_event().is_none());
    assert_eq!(adapter.complete_device_packet(new_pid, DeviceTxStatus::from_wire(0)), CompletionDisposition::Applied);
    let Some(AdapterEvent::Frame(completion)) = adapter.pop_event() else { panic!("missing frame") };
    assert_eq!(completion.identity, b);
}

#[test]
fn large_data_and_beacon_commands_use_block_mode_and_zero_padding() {
    let mut bytes = [0x55; 512];
    let mut adapter = Adapter::<1, 2>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    adapter.enqueue_frame_descriptor(frame).unwrap();
    let command = adapter.command().unwrap();
    assert_eq!(command.argument(), 0x9800_5002);
    assert_eq!(command.bytes().len(), 1024);
    assert_eq!(&command.bytes()[16..528], &[0x55; 512]);
    assert!(command.bytes()[528..].iter().all(|byte| *byte == 0));
    let identity = command.identity();
    adapter.finish_command(identity, PhysicalWrite::Written);
    let body = [0x66; 608];
    adapter.enqueue_firmware_request(FirmwareRequest::BeaconTemplate(&body)).unwrap();
    let command = adapter.command().unwrap();
    assert_eq!(command.argument(), 0x9800_5802);
    assert_eq!(&command.bytes()[..6], &[0x68, 2, 0, 0x50, 0x1c, 1]);
    assert_eq!(&command.bytes()[8..616], &body);
    assert!(command.bytes()[616..].iter().all(|byte| *byte == 0));
}

#[test]
fn metadata_bound_and_oversized_payload_fail_without_consuming_physical_credit() {
    assert_eq!(BssIndex::parse(3), Err(AdapterError::InvalidTransmitMetadata));
    let mut bytes = [0; 4080];
    let mut adapter = Adapter::<1, 1>::new();
    let frame = adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    assert_eq!(adapter.enqueue_frame_descriptor(frame), Err(AdapterError::PayloadTooLong));
    assert!(adapter.command().is_none());
    adapter.enqueue_firmware_request(FirmwareRequest::FlushInterface(&[0; 4])).unwrap();
    assert!(adapter.command().is_some());
}

#[test]
fn aggregate_data_applies_checksum_flags_from_owned_capabilities() {
    let mut bytes = [0x44; 28];
    let mut adapter = Adapter::<1, 1>::new();
    adapter.set_transmit_capabilities(RadioCapabilities(1));
    let frame = adapter.enqueue_frame(FrameOwner::Network, &mut bytes).unwrap();
    let mut fields = ManagementMetadata::management();
    fields.queue = TrafficClass::One;
    fields.report_status = false;
    adapter.enqueue_transmit_frame(frame, TransmitMetadata::AggregatedData(fields)).unwrap();
    let command = adapter.command().unwrap();
    assert_eq!(command.bytes()[2], 14);
    assert_eq!(command.bytes()[3], 7);
    assert_eq!(command.bytes()[5] & 0x80, 0);
    assert_eq!(command.bytes()[13] & 1, 0);
}
