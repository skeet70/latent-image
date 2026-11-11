use super::*;

struct Controller { registers: [u8; 128], response: u16, opened: bool, transactions: u32, open_error: Option<ServiceStatus>, configure_status: ServiceStatus, fifo_control: u32 }
impl Controller { fn new() -> Self { Self { registers: [0; 128], response: 0, opened: false, transactions: 0, open_error: None, configure_status: ServiceStatus(0), fifo_control: 0 } } }
impl SerialController for Controller {
    fn open_power_endpoint(&mut self) -> Result<SerialHandle, ServiceStatus> {
        if let Some(error) = self.open_error { return Err(error); }
        assert!(!self.opened);
        self.opened = true;
        Ok(SerialHandle(1))
    }
    fn start(&mut self, _: SerialHandle) -> ServiceStatus { assert!(self.opened); self.configure_status }
    fn command(&mut self, _: SerialHandle, command: SerialCommand, residue: FifoControlResidue) -> ServiceStatus {
        self.fifo_control = residue.0 | 0x3311;
        let index = usize::from((command.0 >> 8) & 0x7f);
        if command.0 & 0x8000 != 0 { self.registers[index] = command.0 as u8; }
        self.response = u16::from(self.registers[index]);
        self.transactions += 1;
        ServiceStatus(0)
    }
    fn poll(&mut self, _: SerialHandle) -> Result<SerialStatus, ServiceStatus> { Ok(SerialStatus { fifo_flag9: 1, transmit_threshold: 1 }) }
    fn read(&mut self, _: SerialHandle) -> Result<u16, ServiceStatus> { Ok(self.response) }
    fn stop(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn close(&mut self, _: SerialHandle) -> ServiceStatus { self.opened = false; ServiceStatus(0) }
}
#[test]
fn whole_register_write_uses_the_pin_descriptor_register() {
    let mut controller = Controller::new();
    let result = write_field(&mut controller, PowerPin::from_abi(5).unwrap(), 0xf0, FieldWriteMode::WholeRegister);
    assert_eq!(result.observation, SerialObservation::Received(0xf0));
    assert_eq!(controller.registers[0x60], 0xf0);
    assert!(!controller.opened);
}
#[test]
fn masked_field_write_preserves_the_other_bits_of_the_register() {
    let mut controller = Controller::new();
    controller.registers[0x60] = 0xa1;
    let result = write_field(&mut controller, PowerPin::from_abi(7).unwrap(), 1, FieldWriteMode::Masked);
    assert_eq!(controller.registers[0x60], 0xa5);
    assert_eq!(result.observation, SerialObservation::Received(0xa5));
    assert!(!controller.opened);
}
#[test]
fn a_full_width_mask_does_not_read_the_register_before_writing() {
    let mut controller = Controller::new();
    write_field(&mut controller, PowerPin::from_abi(0xf).unwrap(), 0x42, FieldWriteMode::Masked);
    assert_eq!(controller.registers[0x50], 0x42);
    assert_eq!(controller.transactions, 1);
}
#[test]
fn acquisition_failure_is_not_a_received_zero_byte() {
    let mut controller = Controller::new();
    controller.open_error = Some(ServiceStatus(-255));
    let result = write_field(&mut controller, PowerPin::from_abi(5).unwrap(), 0xf0, FieldWriteMode::WholeRegister);
    assert_eq!(result.status, ServiceStatus(0));
    assert_eq!(result.observation, SerialObservation::NotOpened(ServiceStatus(-255)));
    assert_eq!(controller.transactions, 0);
    assert_eq!(controller.registers[0x60], 0);
}
#[test]
fn serial_completion_requires_both_actual_status_fields() {
    for (fifo_flag9, transmit_threshold, complete) in [(0, 1, false), (1, 0, false), (2, 1, false), (1, 1, true), (1, 3, true)] {
        assert_eq!(SerialStatus { fifo_flag9, transmit_threshold }.complete(), complete);
    }
}

#[test]
fn configure_failure_preserves_open_frame_fifo_residue_without_skipping_the_command() {
    let mut controller = Controller::new();
    controller.configure_status = ServiceStatus(-12);
    let result = write_field(&mut controller, PowerPin::from_abi(5).unwrap(), 0xf0, FieldWriteMode::WholeRegister);
    assert_eq!(controller.fifo_control, 0x3317);
    assert_eq!(controller.registers[0x60], 0xf0);
    assert_eq!(result.observation, SerialObservation::Received(0xf0));
}
