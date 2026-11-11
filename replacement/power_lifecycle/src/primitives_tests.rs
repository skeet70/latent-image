use super::*;

#[derive(Default)]
struct Gpio {
    ports: [[u32; 7]; 16],
    stores: usize,
    global: u32,
    irq: u32,
    fiq: u32,
}
impl GpioHardware for Gpio {
    fn write_port(&mut self, port: GpioPort, register: GpioRegister, value: u32) {
        self.ports[usize::from(port.index())][register.offset() as usize / 4] = value;
        self.stores += 1;
    }
    fn save_irq(&mut self) -> SavedIrq { let saved = self.irq; self.irq = 0x80; SavedIrq(saved) }
    fn clear_global_enable(&mut self) { assert_eq!(self.irq, 0x80); self.global &= !1; }
    fn restore_irq(&mut self, saved: SavedIrq) { self.irq = saved.0; }
}
#[test]
fn gpio_reset_clears_enable_and_programs_all_six_registers_of_each_port() {
    let mut gpio = Gpio { global: 0xa5, fiq: 0x40, ..Gpio::default() };
    reset_gpio(&mut gpio);
    assert_eq!(gpio.stores, 96);
    for port in gpio.ports { assert_eq!(port, [0xffff0000, 0xffff0000, 0xffff0000, 0xffff0000, 0xffff0000, 0, 0]); }
    assert_eq!(gpio.global, 0xa4);
    assert_eq!(gpio.irq, 0);
    assert_eq!(gpio.fiq, 0x40);
}

struct Power { config: [u8; 4], fields: [Option<u8>; 35] }
impl TerminalPower for Power {
    fn read_configuration(&mut self, field: PowerConfiguration) -> u8 { self.config[(field.offset() - 0x3b7) as usize] }
    fn write_field(&mut self, field: PowerField, value: u8) -> ServiceStatus { self.fields[field.index() as usize] = Some(value); ServiceStatus(-17) }
}
#[test]
fn terminal_pin_values_come_from_configuration_and_write_errors_do_not_skip_f0() {
    let mut power = Power { config: [0x11, 0x22, 0x33, 3], fields: [None; 35] };
    assert_eq!(power_pin_write_f0(&mut power), ServiceStatus(0));
    assert_eq!(power.fields[0x22], Some(1));
    assert_eq!(power.fields[0x1f], Some(0x11));
    assert_eq!(power.fields[0x20], Some(0x22));
    assert_eq!(power.fields[0x21], Some(0x33));
    assert_eq!(power.fields[5], Some(0xf0));
}
#[test]
fn zero_configuration_mode_leaves_optional_pin_fields_untouched() {
    let mut power = Power { config: [1, 2, 3, 0], fields: [None; 35] };
    power_pin_write_f0(&mut power);
    assert_eq!(power.fields[5], Some(0xf0));
    assert_eq!(power.fields.iter().filter(|value| value.is_some()).count(), 1);
}
#[test]
fn queue_and_mailbox_ids_have_distinct_bounds() {
    assert_eq!(QueueId::from_abi(0), None);
    assert_eq!(QueueId::from_abi(121), None);
    assert_eq!(QueueId::from_abi(0x2005d).unwrap().slot(), 93);
    assert_eq!(MailboxId::from_abi(8), None);
    assert_eq!(MailboxId::from_abi(0x10001).unwrap().slot(), 1);
}

struct QueueState { messages: u32, blocks: u32, queue_exists: bool, pool_exists: bool, mailbox_exists: bool, flags: [u32; 2] }
impl QueueRtos for QueueState {
    fn receive_poll(&mut self, _: QueueId) -> ReceivePoll {
        if self.messages == 0 { ReceivePoll::Stopped(ServiceStatus(-50)) } else { self.messages -= 1; ReceivePoll::Message(PoolBlock(self.messages + 1)) }
    }
    fn release_block(&mut self, _: QueueId, _: PoolBlock) -> ServiceStatus { self.blocks -= 1; ServiceStatus(0) }
    fn delete_pool(&mut self, _: QueueId) -> ServiceStatus { self.pool_exists = false; ServiceStatus(0) }
    fn delete_queue(&mut self, _: QueueId) -> ServiceStatus { self.queue_exists = false; ServiceStatus(0) }
}
struct Storage { reserved: [bool; 240], active: bool, error: Option<ServiceStatus> }
impl QueueStorage for Storage {
    fn release_storage(&mut self, slot: StorageSlot) -> ServiceStatus { self.reserved[slot.0 as usize] = false; ServiceStatus(0) }
    fn clear_active(&mut self, _: QueueId) { self.active = false; }
    fn diagnostic(&mut self, _: QueueDiagnostic, _: u32, _: ServiceStatus) {}
    fn record_error(&mut self, status: ServiceStatus) { self.error = Some(status); }
}
#[test]
fn queue_destruction_releases_storage_without_touching_mailbox_or_event_flags() {
    for channel in [8, 41] {
        let mut queue = QueueState { messages: 3, blocks: 3, queue_exists: true, pool_exists: true, mailbox_exists: true, flags: [1, 1] };
        let mut storage = Storage { reserved: [true; 240], active: true, error: None };
        assert_eq!(destroy_queue(&mut queue, &mut storage, channel), ServiceStatus(0));
        assert_eq!((queue.messages, queue.blocks), (0, 0));
        assert!(!queue.queue_exists && !queue.pool_exists);
        assert!(queue.mailbox_exists);
        assert_eq!(queue.flags, [1, 1]);
        assert!(!storage.reserved[channel as usize - 1]);
        assert!(!storage.reserved[channel as usize + 0x77]);
        assert_eq!(storage.reserved.iter().filter(|reserved| !**reserved).count(), 2);
        assert!(!storage.active);
        assert_eq!(storage.error, None);
    }
}
#[test]
fn invalid_queue_id_changes_only_the_reported_error() {
    let mut queue = QueueState { messages: 3, blocks: 3, queue_exists: true, pool_exists: true, mailbox_exists: true, flags: [1, 1] };
    let mut storage = Storage { reserved: [true; 240], active: true, error: None };
    assert_eq!(destroy_queue(&mut queue, &mut storage, 0), ServiceStatus(-18));
    assert_eq!((queue.messages, queue.blocks), (3, 3));
    assert!(queue.queue_exists && queue.pool_exists && queue.mailbox_exists && storage.active);
    assert!(storage.reserved.iter().all(|reserved| *reserved));
    assert_eq!(storage.error, Some(ServiceStatus(-18)));
}
