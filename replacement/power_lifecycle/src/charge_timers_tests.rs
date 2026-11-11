use super::*;

struct Timers {
    active: bool,
    unregister: i32,
    handles: [Option<InterfaceHandle>; 5],
    released: [u32; 5],
    releases: usize,
    locked: bool,
    locks: usize,
    release_status: i32,
}
impl TimerShutdown for Timers {
    fn registered(&mut self) -> bool { self.active }
    fn send_unregister(&mut self) -> ServiceStatus { ServiceStatus(self.unregister) }
    fn clear_registered(&mut self) { self.active = false; }
    fn lock_interrupts(&mut self) -> SavedIrq {
        assert!(!self.locked);
        self.locked = true;
        self.locks += 1;
        SavedIrq(0x80)
    }
    fn slot_handle(&mut self, slot: TimerSlot) -> Option<InterfaceHandle> { self.handles[slot.index()] }
    fn release_handle(&mut self, handle: InterfaceHandle) -> ServiceStatus {
        assert!(self.locked);
        self.released[self.releases] = handle.0;
        self.releases += 1;
        ServiceStatus(self.release_status)
    }
    fn clear_slot(&mut self, slot: TimerSlot) { self.handles[slot.index()] = None; }
    fn restore_interrupts(&mut self, saved: SavedIrq) { assert_eq!(saved.0, 0x80); self.locked = false; }
}
fn timers() -> Timers {
    Timers { active: true, unregister: 0, handles: [Some(InterfaceHandle(4)), None, Some(InterfaceHandle(9)), None, Some(InterfaceHandle(8))], released: [0; 5], releases: 0, locked: false, locks: 0, release_status: 0 }
}
#[test]
fn shutdown_releases_all_handles_and_registration() {
    let mut state = timers();
    assert_eq!(stop_timers(&mut state), TimerStop::Complete);
    assert!(!state.active);
    assert_eq!(state.handles, [None; 5]);
    assert_eq!(&state.released[..state.releases], &[4, 9, 8]);
    assert!(!state.locked);
    assert_eq!(state.locks, 5);
}
#[test]
fn failed_unregister_keeps_registration_but_still_releases_slots() {
    let mut state = timers();
    state.unregister = -255;
    assert_eq!(stop_timers(&mut state), TimerStop::Failed);
    assert!(state.active);
    assert_eq!(state.handles, [None; 5]);
    assert_eq!(state.releases, 3);
}
#[test]
fn failed_handle_release_clears_slots_and_continues() {
    let mut state = timers();
    state.release_status = -20;
    assert_eq!(stop_timers(&mut state), TimerStop::Failed);
    assert!(!state.active);
    assert_eq!(state.handles, [None; 5]);
    assert_eq!(state.releases, 3);
}
#[test]
fn empty_unregistered_shutdown_succeeds() {
    let mut state = timers();
    state.active = false;
    state.unregister = -255;
    state.handles = [None; 5];
    assert_eq!(stop_timers(&mut state), TimerStop::Complete);
    assert_eq!(state.releases, 0);
}
#[test]
fn charge_packet_uses_low_bit_in_stock_order() {
    assert_eq!(charge_packet([0, 1, 0]), [1, 2]);
    assert_eq!(charge_packet([0xfe, 0xff, 3]), [1, 3]);
    assert_eq!(charge_packet([1, 0, 0]), [1, 4]);
}
struct Registry { handles: [InterfaceHandle; 128], retired: Option<usize>, prepared: Option<usize>, status: i32 }
impl TimerHandleRegistry for Registry {
    fn handle(&mut self, index: usize) -> InterfaceHandle { self.handles[index] }
    fn prepare_interface(&mut self, index: usize) -> ServiceStatus { self.prepared = Some(index); ServiceStatus(self.status) }
    fn invalidate_interface(&mut self, index: usize) { self.retired = Some(index); }
    fn clear_handle(&mut self, index: usize) { self.handles[index] = InterfaceHandle(0); }
}
#[test]
fn release_matches_last_registry_slot_and_retires_even_on_error() {
    let mut registry = Registry { handles: [InterfaceHandle(0); 128], retired: None, prepared: None, status: -1 };
    registry.handles[127] = InterfaceHandle(55);
    assert_eq!(release_timer_handle(&mut registry, InterfaceHandle(55)), ServiceStatus(-255));
    assert_eq!(registry.handles[127], InterfaceHandle(0));
    assert_eq!(registry.retired, Some(127));
    assert_eq!(release_timer_handle(&mut registry, InterfaceHandle(55)), ServiceStatus(-20));
    assert_eq!(release_timer_handle(&mut registry, InterfaceHandle(0)), ServiceStatus(-20));
}

struct Charge { active: bool, locked: bool, token: Option<CallbackToken>, output: Option<[u8; 2]> }
impl ChargeShutdown for Charge {
    fn callback_token(&mut self) -> CallbackToken { CallbackToken(30001) }
    fn unregister_callback(&mut self, token: CallbackToken) { self.token = Some(token); }
    fn lock_charge(&mut self) { self.locked = true; }
    fn clear_charge_active(&mut self) { assert!(self.locked); self.active = false; }
    fn unlock_charge(&mut self) { self.locked = false; }
    fn send_charge_packet(&mut self, packet: [u8; 2]) -> ServiceStatus {
        assert!(!self.locked);
        self.output = Some(packet);
        ServiceStatus(-7)
    }
}
#[test]
fn charge_shutdown_clears_active_and_returns_transport_failure() {
    let mut state = Charge { active: true, locked: false, token: None, output: None };
    assert_eq!(stop_charge(&mut state), ServiceStatus(-7));
    assert!(!state.active);
    assert!(!state.locked);
    assert_eq!(state.token, Some(CallbackToken(30001)));
    assert_eq!(state.output, Some([1, 2]));
}

struct Serial {
    open_error: Option<ServiceStatus>,
    pins: [u32; 2],
    pin_count: usize,
    bytes: [u8; 2],
    byte_count: usize,
    residues: [StatusResidue; 2],
    configured_residue: StatusResidue,
    delayed: u32,
    reads: usize,
    stopped: bool,
    closed: bool,
}
impl ChargeTransport for Serial {
    fn open_charge_serial(&mut self) -> Result<SerialHandle, ServiceStatus> { self.open_error.map_or(Ok(SerialHandle(10)), Err) }
    fn write_charge_pin(&mut self, value: u32) { self.pins[self.pin_count] = value; self.pin_count += 1; }
    fn configure(&mut self, _: SerialHandle) -> (ServiceStatus, StatusResidue) { (ServiceStatus(1), self.configured_residue) }
    fn send_byte(&mut self, _: SerialHandle, byte: u8, residue: StatusResidue) -> ServiceStatus {
        self.bytes[self.byte_count] = byte;
        self.residues[self.byte_count] = residue;
        self.byte_count += 1;
        ServiceStatus(2)
    }
    fn status(&mut self, _: SerialHandle) -> (ServiceStatus, SerialStatus) { (ServiceStatus(4), SerialStatus { state: 1, available: 1 }) }
    fn read_response(&mut self, _: SerialHandle) -> ServiceStatus { self.reads += 1; ServiceStatus(8) }
    fn delay(&mut self, delay: DelayMicros) -> TimerReload {
        self.delayed += delay.0;
        TimerReload(0xa5f0_0000 | self.delayed)
    }
    fn stop(&mut self, _: SerialHandle) -> ServiceStatus { self.stopped = true; ServiceStatus(16) }
    fn close(&mut self, _: SerialHandle) -> ServiceStatus { self.closed = true; ServiceStatus(32) }
}
fn serial() -> Serial {
    Serial { open_error: None, pins: [0; 2], pin_count: 0, bytes: [0; 2], byte_count: 0, residues: [StatusResidue(0); 2], configured_residue: StatusResidue(0x15), delayed: 0, reads: 0, stopped: false, closed: false }
}
#[test]
fn transport_preserves_output_delays_and_all_status_bits() {
    let mut device = serial();
    assert_eq!(transmit_charge(&mut device, &[1, 2]), ServiceStatus(63));
    assert_eq!(device.bytes, [1, 2]);
    assert_eq!(device.residues, [StatusResidue(0x15), StatusResidue(0xa5f0_0320)]);
    assert_eq!(device.reads, 2);
    assert_eq!(device.delayed, 840);
    assert_eq!(device.pins, [0x40004, 0x40000]);
    assert!(device.stopped && device.closed);
}
#[test]
fn transport_open_failure_still_restores_charge_pin() {
    let mut device = serial();
    device.open_error = Some(ServiceStatus(-20));
    assert_eq!(transmit_charge(&mut device, &[1, 2]), ServiceStatus(-20));
    assert_eq!(device.pins, [0x40004, 0x40000]);
    assert_eq!(device.byte_count, 0);
    assert!(!device.stopped && !device.closed);
}

struct Irqs {
    handles: [InterfaceHandle; 128],
    sources: [Option<IrqSource>; 128],
    counter: u32,
    installed: Option<(IrqRoute, IrqHandler)>,
    locked: bool,
    diagnostics: usize,
    install_status: i32,
}
impl TimerHandleRegistry for Irqs {
    fn handle(&mut self, index: usize) -> InterfaceHandle { self.handles[index] }
    fn prepare_interface(&mut self, _: usize) -> ServiceStatus { self.installed = None; ServiceStatus(0) }
    fn invalidate_interface(&mut self, index: usize) { self.sources[index] = None; }
    fn clear_handle(&mut self, index: usize) { self.handles[index] = InterfaceHandle(0); }
}
impl TimerIrqRegistry for Irqs {
    fn route(&mut self, _: IrqSource) -> Option<IrqRoute> { IrqRoute::from_abi(5, 3, 2) }
    fn lock_registry(&mut self) -> SavedIrq { assert!(!self.locked); self.locked = true; SavedIrq(7) }
    fn unlock_registry(&mut self, saved: SavedIrq) { assert_eq!(saved.0, 7); self.locked = false; }
    fn token_counter(&mut self) -> u32 { self.counter }
    fn store_token_counter(&mut self, token: u32) { self.counter = token; }
    fn store_handle(&mut self, slot: usize, handle: InterfaceHandle) { self.handles[slot] = handle; }
    fn install_irq(&mut self, route: IrqRoute, handler: IrqHandler) -> ServiceStatus {
        assert!(self.locked);
        self.installed = Some((route, handler));
        ServiceStatus(self.install_status)
    }
    fn store_source(&mut self, slot: usize, source: IrqSource) { self.sources[slot] = Some(source); }
    fn registration_diagnostic(&mut self) { self.diagnostics += 1; }
}
fn irqs() -> Irqs {
    Irqs { handles: [InterfaceHandle(0); 128], sources: [None; 128], counter: u32::MAX, installed: None, locked: false, diagnostics: 0, install_status: 0 }
}
#[test]
fn irq_attach_wraps_skips_colliding_tokens_and_releases_for_reuse() {
    let mut state = irqs();
    state.handles[127] = InterfaceHandle(1);
    let source = IrqSource::from_abi(6).unwrap();
    let handler = IrqHandler::from_abi(0x656178).unwrap();
    assert_eq!(attach_timer_irq(&mut state, source, handler), Ok(InterfaceHandle(2)));
    assert_eq!(state.sources[5], Some(source));
    assert_eq!(state.installed, Some((IrqRoute::from_abi(5, 3, 2).unwrap(), handler)));
    assert_eq!(attach_timer_irq(&mut state, source, handler), Err(ServiceStatus(-20)));
    assert_eq!(state.diagnostics, 1);
    assert_eq!(release_timer_handle(&mut state, InterfaceHandle(2)), ServiceStatus(0));
    assert_eq!(state.sources[5], None);
    assert_eq!(state.installed, None);
    assert_eq!(attach_timer_irq(&mut state, source, handler), Ok(InterfaceHandle(3)));
    assert!(!state.locked);
}
#[test]
fn irq_attach_failure_keeps_reserved_slot_and_source() {
    let mut state = irqs();
    state.install_status = -1;
    let source = IrqSource::from_abi(6).unwrap();
    let handler = IrqHandler::from_abi(0x656178).unwrap();
    assert_eq!(attach_timer_irq(&mut state, source, handler), Err(ServiceStatus(-255)));
    assert_eq!(state.handles[5], InterfaceHandle(1));
    assert_eq!(state.sources[5], Some(source));
    assert!(!state.locked);
}

#[test]
fn transport_keeps_configured_residue_for_first_byte_only() {
    let mut device = serial();
    device.configured_residue = StatusResidue(0);
    let _ = transmit_charge(&mut device, &[1, 2]);
    assert_eq!(device.residues, [StatusResidue(0), StatusResidue(0xa5f0_0320)]);
}
