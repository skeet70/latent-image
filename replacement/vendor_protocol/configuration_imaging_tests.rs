use super::{configuration_packet, strobe, ImmediateAction};
use crate::{backend::ConfigImages, configuration_actions_low::ActionCompletion};
use xm1_power_services::{charge_timers::{ChargeTransport, DelayMicros, SerialHandle, SerialStatus}, primitives::ServiceStatus, serial_driver::StatusResidue, osal::TimerReload};

struct Device { bytes: [u8; 16], length: usize, replies: [u16; 16], read: usize, pin: u32, clock: u32, direction: u32, closed: bool, open_error: bool }
impl Device { fn new() -> Self { Self { bytes: [0; 16], length: 0, replies: [0; 16], read: 0, pin: 0, clock: 0, direction: 0, closed: false, open_error: false } } }
impl ChargeTransport for Device {
    fn open_charge_serial(&mut self) -> Result<SerialHandle, ServiceStatus> { if self.open_error { Err(ServiceStatus(-20)) } else { Ok(SerialHandle(1)) } }
    fn write_charge_pin(&mut self, value: u32) { self.pin = value; }
    fn configure(&mut self, _: SerialHandle) -> (ServiceStatus, StatusResidue) { (ServiceStatus(0), StatusResidue(0)) }
    fn send_byte(&mut self, _: SerialHandle, byte: u8, _: StatusResidue) -> ServiceStatus { self.bytes[self.length] = byte; self.length += 1; ServiceStatus(0) }
    fn status(&mut self, _: SerialHandle) -> (ServiceStatus, SerialStatus) { (ServiceStatus(0), SerialStatus { state: 1, available: 1 }) }
    fn read_response(&mut self, _: SerialHandle) -> ServiceStatus { self.read += 1; ServiceStatus(0) }
    fn delay(&mut self, _: DelayMicros) -> TimerReload { TimerReload(200) }
    fn stop(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn close(&mut self, _: SerialHandle) -> ServiceStatus { self.closed = true; ServiceStatus(0) }
}
impl strobe::Hardware for Device {
    fn read_word(&mut self, _: SerialHandle) -> u16 { let result = self.replies[self.read]; self.read += 1; result }
    fn set_receive_clock(&mut self, value: u32) { self.clock = value; }
    fn set_receive_direction(&mut self, value: u32) { self.direction = value; }
}

#[test]
fn immediate_actions_publish_service_success() {
    for action in [76, 77, 81, 82, 83, 84] { assert_eq!(ImmediateAction::from_queue_action(action).unwrap().execute(), ActionCompletion::service_success()); }
    for action in [75, 78, 80, 85, 101] { assert_eq!(ImmediateAction::from_queue_action(action), None); }
}

#[test]
fn packet_writes_reach_the_serial_device() {
    for (kind, expected) in [(0, 9), (1, 2), (2, 4), (3, 2), (4, 3)] {
        let mut bytes = [0; 256];
        bytes[0x81] = kind;
        bytes[0x82..0x8a].copy_from_slice(&[11, 12, 13, 14, 15, 16, 17, 18]);
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut device = Device::new();
        assert_eq!(configuration_packet(&mut configuration, &mut device), Ok(ActionCompletion::service_success()));
        assert_eq!(&device.bytes[..expected], &[kind, 11, 12, 13, 14, 15, 16, 17, 18][..expected]);
        assert!(device.closed);
        assert_eq!(device.pin, 0x40000);
    }
}

#[test]
fn packet_reads_publish_requested_fields_and_clear_the_remaining_fields() {
    for (kind, populated) in [(0x20, 7), (0x21, 1), (0x22, 3), (0x23, 1)] {
        let mut bytes = [0xff; 256]; bytes[0x81] = kind;
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let mut device = Device::new(); device.replies = [0xeeee, 11, 12, 13, 14, 15, 16, 17, 18, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(configuration_packet(&mut configuration, &mut device), Ok(ActionCompletion::service_success()));
        for index in 1..=10 { assert_eq!(configuration.byte(0x81 + index as u32), Some(if index <= populated { 10 + index as u8 } else { 0 })); }
        assert_eq!(device.clock, 0x30001); assert_eq!(device.direction, 0x10001); assert!(device.closed);
    }
}

#[test]
fn invalid_packet_kind_rejects_without_touching_the_device() {
    let mut bytes = [0; 256]; bytes[0x81] = 0x24;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut device = Device::new();
    assert_eq!(configuration_packet(&mut configuration, &mut device), Ok(ActionCompletion::service_rejected()));
    assert_eq!(device.length, 0); assert_eq!(device.pin, 0);
}

#[test]
fn serial_open_failure_is_not_promoted_to_a_packet_action_failure() {
    let mut bytes = [0; 256]; bytes[0x81] = 0x21;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let mut device = Device::new(); device.open_error = true;
    assert_eq!(configuration_packet(&mut configuration, &mut device), Ok(ActionCompletion::service_success()));
    assert_eq!(configuration.byte(0x82), Some(0)); assert_eq!(device.pin, 0x40000); assert!(!device.closed);
}
