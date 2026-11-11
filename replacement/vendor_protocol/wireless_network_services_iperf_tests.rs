use super::*;
use crate::vendor_wireless_network::{MacAddress, dhcp::LeaseSlot, lan::LanMode, state::Lease};

#[test]
fn defaults_and_reachable_option_order_match_the_service() {
    let tcp = Options::parse(b"-c 192.168.1.2").unwrap();
    assert_eq!(tcp.protocol, Protocol::Tcp);
    assert_eq!(tcp.port.get(), 5001);
    assert_eq!(tcp.duration, Seconds(10));
    assert_eq!(tcp.block, BlockSize(8192));
    assert_eq!(tcp.rate, BitsPerSecond(1_048_576));
    assert_eq!(Options::parse(b"-u -s").unwrap().protocol, Protocol::Udp);
    assert_eq!(Options::parse(b"-s -u"), Err(ParseError::ClientOnly));
    assert_eq!(Options::parse(b"-s -c 192.168.1.2"), Err(ParseError::ConflictingRoles));
    assert_eq!(Options::parse(b"-c 192.168.1.2 -s"), Err(ParseError::ConflictingRoles));
}

#[test]
fn dead_parser_switch_cases_are_not_synthesized_as_options() {
    let ordinary = Options::parse(b"-c 192.168.1.2").unwrap();
    assert_eq!(Options::parse(b"-6 -N -P 128 -M 9216 -R -c 192.168.1.2").unwrap(), ordinary);
}

#[test]
fn numeric_limits_and_scaled_units_are_observable() {
    assert_eq!(Options::parse(b"-c 1.2.3.4 -l 1M -w 128M -b 2m -n 3K").unwrap().block, BlockSize(1_048_576));
    let values = Options::parse(b"-c 1.2.3.4 -w 128M -b 2m -n 3K").unwrap();
    assert_eq!(values.window, SocketBufferSize(134_217_728));
    assert_eq!(values.rate, BitsPerSecond(2_000_000));
    assert_eq!(values.byte_limit, ByteCount(3072));
    assert_eq!(Options::parse(b"-c 1.2.3.4 -t 3601"), Err(ParseError::Duration));
    assert_eq!(Options::parse(b"-c 1.2.3.4 -l 1048577"), Err(ParseError::Block));
    assert_eq!(Options::parse(b"-c 1.2.3.4 -w 134217729"), Err(ParseError::Window));
}

#[test]
fn tcp_and_udp_description_bytes_are_external_wire_contracts() {
    assert_eq!(Options::parse(b"-c 1.2.3.4").unwrap().description(), [
        0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x13, 0x89,
        0, 0, 0x20, 0, 0, 0, 0, 0, 0xff, 0xff, 0xfc, 0x18,
    ]);
    assert_eq!(Options::parse(b"-c 1.2.3.4 -u -r").unwrap().description(), [
        0x80, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x13, 0x89,
        0, 0, 5, 0xbe, 0, 0x10, 0, 0, 0xff, 0xff, 0xfc, 0x18,
    ]);
}

#[test]
fn datagram_and_server_report_decoding_use_network_byte_order() {
    let input = [0xff, 0xff, 0xff, 0xfd, 0, 0, 0, 2, 0, 0, 0, 3];
    assert_eq!(DatagramHeader::parse(&input), Some(DatagramHeader { sequence: -3, seconds: 2, microseconds: 3 }));
    assert_eq!(DatagramHeader::parse(&input[..11]), None);
    let report = [
        0xff, 0xff, 0xff, 0xfd, 0, 0, 0, 2, 0, 0, 0, 3,
        0x80, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 4,
        0, 0, 0, 5, 0, 0, 0, 6, 0, 0, 0, 7,
        0, 0, 0, 8, 0, 0, 0, 9, 0, 0, 0, 10, 0, 0, 0, 11,
    ];
    assert_eq!(ServerReport::parse(&report), Some(ServerReport {
        bytes: ByteCount(4_294_967_300), duration_us: 5_000_006,
        lost: 7, out_of_order: 8, datagrams: 9, jitter_us: 10_000_011,
    }));
    assert_eq!(ServerReport::parse(&report[..51]), None);
}

fn start_lan<'a>(slots: &'a mut [LeaseSlot], address: [u8; 4], suffix: u8) -> LanRuntime<'a> {
    let mut lan = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, suffix]), slots);
    let lease = Lease { address: Ipv4Address(address), mask: crate::vendor_wireless_network::state::Ipv4Mask([255, 255, 255, 0]), router: Ipv4Address([0; 4]) };
    lan.start(LanMode::StaticStation { lease }, Milliseconds(0), &mut [0; 4096]).unwrap();
    lan
}

fn exchange(target: &mut LanRuntime<'_>, packet: &[u8], now: Milliseconds, output: &mut [u8; 4096]) -> Option<usize> {
    match target.receive(packet, now, output).unwrap() { LanOutcome::Frame { length } => Some(length), _ => None }
}
/// A reply can elicit another frame; drain the exchange until it settles.
fn deliver(source: &mut LanRuntime<'_>, destination: &mut LanRuntime<'_>, packet: &[u8], now: Milliseconds) {
    let mut current = [0; 4096];
    current[..packet.len()].copy_from_slice(packet);
    let mut length = packet.len();
    let mut reply = [0; 4096];
    let mut to_destination = true;
    for _ in 0..64 {
        let reply_length = if to_destination { exchange(destination, &current[..length], now, &mut reply) }
            else { exchange(source, &current[..length], now, &mut reply) };
        let Some(reply_length) = reply_length else { return };
        core::mem::swap(&mut current, &mut reply);
        length = reply_length;
        to_destination = !to_destination;
    }
    panic!("frame exchange did not settle");
}
fn pump(source: &mut LanRuntime<'_>, destination: &mut LanRuntime<'_>, now: Milliseconds) {
    let mut packet = [0; 4096];
    if let LanOutcome::Frame { length } = source.poll(now, &mut packet).unwrap() { deliver(source, destination, &packet[..length], now); }
}
fn step(runtime: &mut IperfRuntime<'_>, source: &mut LanRuntime<'_>, destination: &mut LanRuntime<'_>, now: Milliseconds) {
    let mut packet = [0; 4096];
    if let IperfPoll::Frame { length } = runtime.poll(source, now, &mut packet).unwrap() { deliver(source, destination, &packet[..length], now); }
}

#[test]
fn tcp_client_server_transfer_and_fin_teardown_use_the_canonical_lan() {
    let mut client_slots = []; let mut server_slots = [];
    let mut client_lan = start_lan(&mut client_slots, [192, 168, 1, 1], 1);
    let mut server_lan = start_lan(&mut server_slots, [192, 168, 1, 2], 2);
    let mut client_buffer = [0; 8192]; let mut server_buffer = [0; 8192];
    let mut client = IperfRuntime::new(&mut client_buffer); let mut server = IperfRuntime::new(&mut server_buffer);
    server.start(b"-s", Milliseconds(0)).unwrap();
    client.start(b"-c 192.168.1.2 -n 8192", Milliseconds(0)).unwrap();
    // The client's graceful close waits for the 120-second time-wait to expire.
    for tick in 0..200_000 {
        let now = Milliseconds(tick);
        step(&mut server, &mut server_lan, &mut client_lan, now);
        step(&mut client, &mut client_lan, &mut server_lan, now);
        pump(&mut client_lan, &mut server_lan, now); pump(&mut server_lan, &mut client_lan, now);
        if client.result().is_some() && server.result().is_some() { break; }
    }
    let sent = client.result().unwrap(); let received = server.result().unwrap();
    assert_eq!(sent.failure, None); assert_eq!(received.failure, None);
    assert_eq!(sent.progress.sent, 8192); assert_eq!(received.progress.received, 8192);
    assert_eq!(sent.status(), 0);
    for _ in 0..8 { assert!(client_lan.open_tcp_unbound().is_ok()); }
}

#[test]
fn udp_transfer_paces_packets_exchanges_results_and_releases_ports() {
    let mut client_slots = []; let mut server_slots = [];
    let mut client_lan = start_lan(&mut client_slots, [192, 168, 1, 1], 1);
    let mut server_lan = start_lan(&mut server_slots, [192, 168, 1, 2], 2);
    let mut client_buffer = [0; 8192]; let mut server_buffer = [0; 8192];
    let mut client = IperfRuntime::new(&mut client_buffer); let mut server = IperfRuntime::new(&mut server_buffer);
    server.start(b"-u -s", Milliseconds(0)).unwrap();
    client.start(b"-c 192.168.1.2 -u -n 4410 -b 117600", Milliseconds(0)).unwrap();
    for tick in 0..5000 {
        let now = Milliseconds(tick);
        step(&mut server, &mut server_lan, &mut client_lan, now);
        step(&mut client, &mut client_lan, &mut server_lan, now);
        pump(&mut client_lan, &mut server_lan, now); pump(&mut server_lan, &mut client_lan, now);
        if client.result().is_some() && server.result().is_some() { break; }
    }
    let sent = client.result().unwrap(); let received = server.result().unwrap();
    assert_eq!(sent.failure, None); assert_eq!(received.failure, None);
    assert_eq!(sent.progress.sent, 4410); assert_eq!(sent.progress.datagrams, 3);
    assert!(sent.progress.elapsed_ms >= 200);
    assert_eq!(sent.peer.unwrap().bytes, ByteCount(received.progress.received));
    assert_eq!(received.progress.lost, 0);
    assert_udp_slots_released(&mut client_lan);
    assert!(server_lan.open_udp(UdpPort(5001)).is_ok());
}

#[test]
fn cancellation_and_parser_failure_have_distinct_service_returns() {
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    assert_eq!(runtime.start(b"-s -u", Milliseconds(0)), Err(IperfError::Parse(ParseError::ClientOnly)));
    assert_eq!(runtime.result().unwrap().status(), -255);
    runtime.start(b"-u -s", Milliseconds(0)).unwrap();
    runtime.poll(&mut lan, Milliseconds(0), &mut [0; 4096]).unwrap();
    let result = runtime.cancel(&mut lan);
    assert_eq!(result.status(), 0); assert_eq!(result.failure, Some(IperfError::Cancelled));
    assert_eq!(runtime.cancel(&mut lan), result);
    assert!(lan.open_udp(UdpPort(5001)).is_ok());
}

#[test]
fn network_failure_is_not_hidden_by_the_vendor_runner_return() {
    let mut slots = []; let mut lan = LanRuntime::new(MacAddress([2, 0, 0, 0, 0, 1]), &mut slots);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-c 192.168.1.2", Milliseconds(0)).unwrap();
    let result = match runtime.poll(&mut lan, Milliseconds(0), &mut [0; 4096]).unwrap() {
        IperfPoll::Complete(result) => result, other => panic!("expected completed failure: {other:?}"),
    };
    assert_eq!(result.status(), 0); assert_eq!(result.failure, Some(IperfError::Network(LanError::Down)));
}

#[test]
fn missing_peer_report_retries_then_finishes_with_a_typed_failure() {
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-c 255.255.255.255 -u -n 1470", Milliseconds(0)).unwrap();
    let mut output = [0; 4096]; let mut retries = 0; let mut final_header = None;
    for tick in 0..3000 {
        if let IperfPoll::Frame { length } = runtime.poll(&mut lan, Milliseconds(tick), &mut output).unwrap() {
            let datagram = crate::vendor_wireless_network::packet::UdpDatagram::parse_ipv4(&output[14..length]).unwrap();
            let header = DatagramHeader::parse(datagram.payload).unwrap();
            if header.sequence < 0 {
                retries += 1;
                if let Some(first) = final_header { assert_eq!(header, first); } else { final_header = Some(header); }
            }
        }
    }
    assert_eq!(retries, 10);
    let result = runtime.result().unwrap();
    assert_eq!(result.failure, Some(IperfError::Timeout)); assert_eq!(result.status(), 0);
    assert_udp_slots_released(&mut lan);
}

#[test]
fn transmitted_udp_bytes_include_the_description_timestamp_and_traffic_class() {
    use crate::vendor_wireless_network::packet::UdpDatagram;
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-c 255.255.255.255 -u -b 117600 -S 0x20", Milliseconds(1000)).unwrap();
    let mut output = [0; 4096];
    assert_eq!(runtime.poll(&mut lan, Milliseconds(1000), &mut output).unwrap(), IperfPoll::Pending);
    assert_eq!(runtime.poll(&mut lan, Milliseconds(1099), &mut output).unwrap(), IperfPoll::Pending);
    let IperfPoll::Frame { length } = runtime.poll(&mut lan, Milliseconds(1100), &mut output).unwrap() else { panic!("expected paced datagram"); };
    assert_eq!(output[15], 0x20);
    let datagram = UdpDatagram::parse_ipv4(&output[14..length]).unwrap();
    assert_eq!(datagram.payload.len(), 1470);
    assert_eq!(DatagramHeader::parse(datagram.payload), Some(DatagramHeader { sequence: 0, seconds: 1, microseconds: 100_000 }));
    assert_eq!(&datagram.payload[12..36], &[
        0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0x13, 0x89,
        0, 0, 5, 0xbe, 0, 1, 0xcb, 0x60, 0xff, 0xff, 0xfc, 0x18,
    ]);
    assert_eq!(&datagram.payload[36..38], b"91");
    assert_eq!(runtime.poll(&mut lan, Milliseconds(1199), &mut output).unwrap(), IperfPoll::Pending);
    let IperfPoll::Frame { length } = runtime.poll(&mut lan, Milliseconds(1200), &mut output).unwrap() else { panic!("expected second paced datagram"); };
    let datagram = UdpDatagram::parse_ipv4(&output[14..length]).unwrap();
    assert_eq!(DatagramHeader::parse(datagram.payload).unwrap().sequence, 1);
}

#[test]
fn bound_multicast_membership_is_owned_by_the_canonical_lan() {
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-u -s -B 239.1.2.3", Milliseconds(0)).unwrap();
    runtime.poll(&mut lan, Milliseconds(0), &mut [0; 4096]).unwrap();
    let mut output = [0; 4096];
    let LanOutcome::Frame { length } = lan.poll(Milliseconds(0), &mut output).unwrap() else { panic!("expected membership report"); };
    assert!(length >= 42); assert_eq!(output[23], 2);
    runtime.cancel(&mut lan);
    assert!(lan.open_udp(UdpPort(5001)).is_ok());
}

#[test]
fn tokenizer_space_rules_and_failed_decimal_conversion_remain_observable() {
    assert_eq!(Options::parse(b"-c192.168.1.2").unwrap().role, Role::Client(Ipv4Address([255; 4])));
    assert_eq!(Options::parse(b"-c  192.168.1.2").unwrap().role, Role::Client(Ipv4Address([255; 4])));
    assert_eq!(Options::parse(b"-c 1.2.3.4 -t 12x").unwrap().duration, Seconds(10));
    assert_eq!(Options::parse(b"-c 1.2.3.4 -t").unwrap().duration, Seconds(0));
    let port = Options::parse(b"-c 1.2.3.4 -p 70000").unwrap();
    assert_eq!(port.port.get(), 4464);
    assert_eq!(&port.description()[8..12], &[0, 1, 0x11, 0x70]);
}

#[test]
fn zero_destination_port_is_a_runner_failure_not_a_parser_failure() {
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-c 192.168.1.2 -p 0", Milliseconds(0)).unwrap();
    let IperfPoll::Complete(result) = runtime.poll(&mut lan, Milliseconds(0), &mut [0; 4096]).unwrap() else { panic!("expected invalid destination completion"); };
    assert_eq!(result.failure, Some(IperfError::Network(LanError::Socket)));
    assert_eq!(result.status(), 0);
}

fn assert_udp_slots_released(lan: &mut LanRuntime<'_>) {
    for port in 60_000..60_008 { assert!(lan.open_udp(UdpPort(port)).is_ok()); }
}

#[test]
fn asynchronous_network_failure_preserves_the_cause_and_releases_the_service_socket() {
    let mut slots = []; let mut lan = start_lan(&mut slots, [192, 168, 1, 1], 1);
    let mut buffer = [0; 8192]; let mut runtime = IperfRuntime::new(&mut buffer);
    runtime.start(b"-u -s", Milliseconds(0)).unwrap();
    runtime.poll(&mut lan, Milliseconds(0), &mut [0; 4096]).unwrap();
    let result = runtime.network_failed(&mut lan, LanError::Down);
    assert_eq!(result.failure, Some(IperfError::Network(LanError::Down)));
    assert_eq!(result.status(), 0);
    assert!(lan.open_udp(UdpPort(5001)).is_ok());
}
