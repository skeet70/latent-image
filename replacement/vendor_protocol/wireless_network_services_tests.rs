use super::*;
use super::selftest::{encode_request, TestArgument, TestCommand, TestError, TestIdentity};
use super::super::wireless_association::station::ScanBatch;
fn writes(output: &mut ServiceOutput) -> Vec<(ConfigKey, u8)> {
    let mut result = Vec::new();
    while let Some(effect) = output.pop() { if let ServiceEffect::WriteByte { key, value } = effect { result.push((key, value)); } }
    result
}
#[test]
fn scan_notifies_only_after_canonical_driver_completion() {
    let mut station = StationRuntime::new(MacAddress([2, 0, 0, 0, 0, 1]));
    let mut service = NetworkServices::new();
    let mut output = ServiceOutput::new();
    let keys = ScanKeys { result: ConfigKey(100), count: ConfigKey(101) };
    service.scan(&mut station, keys, &mut output).unwrap();
    assert_eq!(writes(&mut output), [(ConfigKey(100), 255)]);
    assert!(!service.poll(&station, &mut output));
    let command = station.poll().unwrap();
    station.receive(command.scanned(ScanBatch::new()).unwrap()).unwrap();
    assert!(service.poll(&station, &mut output));
    assert_eq!(writes(&mut output), [(ConfigKey(100), 0), (ConfigKey(101), 0)]);
    assert!(!service.poll(&station, &mut output));
}
#[test]
fn wps_credentials_preserve_unwritten_configuration_tail() {
    let mut output = ServiceOutput::new();
    NetworkServices::wps_notification(WpsResult::Credentials, b"AP", b"secret", ConfigKey(77), &mut output).unwrap();
    assert_eq!(writes(&mut output), [(ConfigKey(0x3d40), b'A'), (ConfigKey(0x3d41), b'P'), (ConfigKey(0x3d62), b's'), (ConfigKey(0x3d63), b'e'), (ConfigKey(0x3d64), b'c'), (ConfigKey(0x3d65), b'r'), (ConfigKey(0x3d66), b'e'), (ConfigKey(0x3d67), b't'), (ConfigKey(77), 0)]);
}
#[test]
fn overlap_and_failure_share_display_but_keep_distinct_status() {
    let mut output = ServiceOutput::new();
    NetworkServices::wps_notification(WpsResult::Overlap, b"", b"", ConfigKey(7), &mut output).unwrap();
    match output.pop().unwrap() {
        ServiceEffect::Display { text, x, y } => { assert_eq!(text.bytes(), b"WPS FAIL "); assert_eq!((x, y), (1, 4)); }
        other => panic!("unexpected effect: {other:?}"),
    }
    assert_eq!(writes(&mut output), [(ConfigKey(7), 3)]);
}
#[test]
fn iperf_execution_failure_is_a_result_byte_not_protocol_failure() {
    let mut output = ServiceOutput::new();
    NetworkServices::complete_iperf(-255, ConfigKey(9), &mut output).unwrap();
    assert_eq!(output.pop(), Some(ServiceEffect::WriteByte { key: ConfigKey(9), value: 1 }));
    assert_eq!(output.pop(), Some(ServiceEffect::Signal(ServiceSignal::Complete)));
}
#[test]
fn scan_copy_leaves_configuration_tail_untouched() {
    let mut output = ServiceOutput::new();
    NetworkServices::publish_scan_ssid(Ssid::parse(b"AP").unwrap(), &[ConfigKey(1), ConfigKey(2), ConfigKey(3)], &mut output).unwrap();
    assert_eq!(writes(&mut output), [(ConfigKey(1), b'A'), (ConfigKey(2), b'P')]);
}
#[test]
fn framed_selftest_status_request_has_stock_wire_layout() {
    let mut bytes = [0; 32];
    let length = encode_request(TestIdentity(0x12345678), TestArgument(0x9abc), TestCommand::QueryStatus, &[], &mut bytes).unwrap();
    assert_eq!(length, 16);
    assert_eq!(&bytes[..length], &[0x24, 0xc3, 0, 16, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0, 0, 0xfc, 0x9e, 13, 10]);
}
#[test]
fn framed_selftest_payload_and_checksum_are_network_order() {
    let mut bytes = [0; 20];
    let length = encode_request(TestIdentity(0), TestArgument(1), TestCommand::SetMode, &[1, 0, 0, 0], &mut bytes).unwrap();
    assert_eq!(length, 20);
    assert_eq!(&bytes[..length], &[0x24, 0xb3, 0, 20, 0, 0, 0, 0, 0, 1, 0, 0, 1, 0, 0, 0, 0xff, 0x12, 13, 10]);
}
#[test]
fn selftest_rejects_oversize_without_mutating_output() {
    let mut bytes = [0x5a; 2048];
    assert_eq!(encode_request(TestIdentity(0), TestArgument(0), TestCommand::WriteBlock, &[0; 2033], &mut bytes), Err(TestError::Capacity));
    assert_eq!(bytes, [0x5a; 2048]);
}

#[test]
fn remote_page_reader_runs_over_canonical_lan_and_keeps_page_order() {
    use super::remote::{RemoteRequest, RemoteResult, RemoteTest};
    use super::selftest::TestTime;
    use super::super::vendor_wireless_network::{dhcp::Milliseconds, lan::{LanMode, LanOutcome, LanRuntime}, state::{Ipv4Address, Lease}, transport::{ReceiveOutcome, TcpPort, TcpSequence}};
    let mut slots_a = [];
    let mut slots_b = [];
    let mut client = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, 1]), &mut slots_a);
    let mut server = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, 2]), &mut slots_b);
    let mut frame = [0; 4096];
    for (lan, address) in [(&mut client, [192, 168, 0, 1]), (&mut server, [192, 168, 0, 251])] {
        lan.start(LanMode::StaticStation { lease: Lease { address: Ipv4Address(address), mask: crate::vendor_wireless_network::state::Ipv4Mask([255, 255, 255, 0]), router: Ipv4Address([0; 4]) } }, Milliseconds(0), &mut frame).unwrap();
    }
    let listener = server.listen_tcp(TcpPort::parse(8080).unwrap(), TcpSequence(900)).unwrap();
    let mut remote = RemoteTest::new(TestIdentity(7), TestArgument(3), TcpPort::parse(40000).unwrap(), TcpSequence(100));
    remote.start(RemoteRequest::Pages).unwrap();
    let mut connection = None;
    let mut request = Vec::new();
    for tick in 0..1000 {
        remote.poll(&mut client, TestTime(tick));
        let mut frames = std::collections::VecDeque::new();
        if let LanOutcome::Frame { length } = client.poll(Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((true, frame[..length].to_vec())); }
        if let LanOutcome::Frame { length } = server.poll(Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((false, frame[..length].to_vec())); }
        while let Some((to_server, bytes)) = frames.pop_front() {
            let target = if to_server { &mut server } else { &mut client };
            if let LanOutcome::Frame { length } = target.receive(&bytes, Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((!to_server, frame[..length].to_vec())); }
        }
        if connection.is_none() { connection = server.accept_tcp(listener).ok(); }
        if let Some(socket) = connection {
            let mut received = [0; 2048];
            if let Ok(ReceiveOutcome::Bytes(length)) = server.receive_tcp(socket, &mut received) { request.extend_from_slice(&received[..length]); }
            if request.len() >= 20 {
                assert_eq!(request[1], 0x40);
                let index = u32::from_be_bytes(request[12..16].try_into().unwrap()) as u8;
                let mut response = [0; 500];
                response[2..4].copy_from_slice(&500u16.to_be_bytes());
                response[20..].fill(index + 1);
                assert_eq!(server.send_tcp(socket, &response).unwrap(), 500);
                request.clear();
            }
        }
        if remote.result().is_some() { break; }
    }
    assert_eq!(remote.result(), Some(Ok(RemoteResult::Pages)));
    let pages = remote.pages().unwrap();
    assert_eq!(&pages[..480], &[1; 480]);
    assert_eq!(&pages[480..960], &[2; 480]);
    assert_eq!(&pages[960..], &[3; 480]);
}

#[test]
fn service_effect_backpressure_preserves_pending_output() {
    let mut output = ServiceOutput::new();
    for _ in 0..128 { NetworkServices::complete_iperf(0, ConfigKey(4), &mut output).unwrap(); }
    assert_eq!(NetworkServices::complete_iperf(-255, ConfigKey(5), &mut output), Err(ServiceError::OutputCapacity));
    for _ in 0..128 {
        assert_eq!(output.pop(), Some(ServiceEffect::WriteByte { key: ConfigKey(4), value: 0 }));
        assert_eq!(output.pop(), Some(ServiceEffect::Signal(ServiceSignal::Complete)));
    }
    assert_eq!(output.pop(), None);
    NetworkServices::complete_iperf(-255, ConfigKey(5), &mut output).unwrap();
    assert_eq!(writes(&mut output), [(ConfigKey(5), 1)]);
}

#[test]
fn remote_transfer_negotiates_pads_and_reads_duration_over_canonical_lan() {
    use super::remote::{RemoteRequest, RemoteResult, RemoteTest};
    use super::selftest::TestTime;
    use super::super::vendor_wireless_network::{dhcp::Milliseconds, lan::{LanMode, LanOutcome, LanRuntime}, state::{Ipv4Address, Lease}, transport::{ReceiveOutcome, TcpPort, TcpSequence}};
    let mut slots_a = [];
    let mut slots_b = [];
    let mut client = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, 1]), &mut slots_a);
    let mut server = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, 2]), &mut slots_b);
    let mut frame = [0; 4096];
    for (lan, address) in [(&mut client, [192, 168, 0, 1]), (&mut server, [192, 168, 0, 251])] {
        lan.start(LanMode::StaticStation { lease: Lease { address: Ipv4Address(address), mask: crate::vendor_wireless_network::state::Ipv4Mask([255, 255, 255, 0]), router: Ipv4Address([0; 4]) } }, Milliseconds(0), &mut frame).unwrap();
    }
    let listener = server.listen_tcp(TcpPort::parse(8080).unwrap(), TcpSequence(900)).unwrap();
    let mut remote = RemoteTest::new(TestIdentity(7), TestArgument(3), TcpPort::parse(40000).unwrap(), TcpSequence(100));
    remote.start(RemoteRequest::Transfer(&[1, 2, 3, 4, 5])).unwrap();
    let mut connection = None;
    let mut request = Vec::new();
    let mut commands = Vec::new();
    for tick in 0..5000 {
        remote.poll(&mut client, TestTime(tick));
        let mut frames = std::collections::VecDeque::new();
        if let LanOutcome::Frame { length } = client.poll(Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((true, frame[..length].to_vec())); }
        if let LanOutcome::Frame { length } = server.poll(Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((false, frame[..length].to_vec())); }
        while let Some((to_server, bytes)) = frames.pop_front() {
            let target = if to_server { &mut server } else { &mut client };
            if let LanOutcome::Frame { length } = target.receive(&bytes, Milliseconds(tick as u32), &mut frame).unwrap() { frames.push_back((!to_server, frame[..length].to_vec())); }
        }
        if connection.is_none() { connection = server.accept_tcp(listener).ok(); }
        if let Some(socket) = connection {
            let mut received = [0; 2048];
            if let Ok(ReceiveOutcome::Bytes(length)) = server.receive_tcp(socket, &mut received) { request.extend_from_slice(&received[..length]); }
            if request.len() >= 4 && request.len() == usize::from(u16::from_be_bytes([request[2], request[3]])) {
                let command = request[1];
                commands.push(command);
                if command == 0x51 { assert_eq!(&request[12..24], &[2, 0, 0, 0, 0, 5, 0, 0, 0, 0, 0, 0]); }
                if command == 0x52 {
                    let index = u32::from_be_bytes(request[12..16].try_into().unwrap());
                    assert_eq!(&request[16..20], if index == 0 { &[1, 2, 3, 4] } else { &[5, 0, 0, 0] });
                }
                let mut response = [0; 20];
                response[2..4].copy_from_slice(&20u16.to_be_bytes());
                if command == 0x51 { response[18..20].copy_from_slice(&4u16.to_be_bytes()); }
                if command == 0xb0 { response[16..20].copy_from_slice(&7u32.to_be_bytes()); }
                assert_eq!(server.send_tcp(socket, &response).unwrap(), 20);
                request.clear();
            }
        }
        if remote.result().is_some() { break; }
    }
    assert_eq!(commands, [0xb3, 0x50, 0x51, 0x52, 0x52, 0x53, 0xb0]);
    assert_eq!(remote.result(), Some(Ok(RemoteResult::Transferred { duration: 700 })));
}
