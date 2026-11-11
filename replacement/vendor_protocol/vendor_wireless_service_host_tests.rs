use super::{HostPump, PhysicalObservation};
use super::pool::ReceivePool;
use crate::sd_controller::{ControlError, ControlPlane, HalfRegister, Host, Mmio, OperationLock, TransferUnit, WordRegister};
use crate::transport::cpu::CacheInstructions;
use crate::vendor_wireless_power::RadioTick;
use crate::vendor_wireless_station_driver::receive::{ReceivePlan, ReceivePort};

struct Hardware;
impl Mmio for Hardware {
    fn read_half(&mut self, register: HalfRegister) -> u16 { if register == HalfRegister::Remaining { 1 } else { 0 } }
    fn write_half(&mut self, _: HalfRegister, _: u16) {}
    fn read_word(&mut self, _: WordRegister) -> u32 { 0 }
    fn write_word(&mut self, _: WordRegister, _: u32) {}
    fn response_half(&mut self, _: u8) -> u16 { 0 }
}
struct Lock { held: bool }
impl OperationLock for Lock {
    fn acquire(&mut self) -> Result<(), ControlError> { self.held = true; Ok(()) }
    fn release(&mut self) -> Result<(), ControlError> { self.held = false; Ok(()) }
}
struct Cache;
impl CacheInstructions for Cache {
    fn level_id(&mut self) -> u32 { 0 }
    fn select_level(&mut self, _: u32) {}
    fn instruction_barrier(&mut self) {}
    fn size_id(&mut self) -> u32 { 0 }
    fn clean_invalidate_set_way(&mut self, _: u32) {}
    fn clean_invalidate_address(&mut self, _: u32) {}
}

#[test]
fn dropped_transfer_releases_lock_but_does_not_publish_receive_packets() {
    let mut buffer = [0; 64];
    let pool = ReceivePool::new([buffer.as_mut_slice()]);
    let mut writer = pool.take().unwrap();
    let mut snapshot = [0; 88];
    snapshot[12] = 1;
    snapshot[16] = 4;
    let plan = ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 1).unwrap();
    let mut pump = HostPump::new();
    let mut control = ControlPlane::new();
    control.initialize();
    control.publish_available(true);
    control.publish_connected(true);
    let mut host = Host::new(TransferUnit::BLOCK_512);
    host.set_enabled(true);
    let mut mmio = Hardware;
    let mut cache = Cache;
    let mut lock = Lock { held: false };
    let transfer = match pump.begin_receive(&mut writer, plan, &control, &mut host, &mut mmio, &mut cache, &mut lock, RadioTick(0)) {
        Ok(Some(transfer)) => transfer, _ => panic!(),
    };
    drop(transfer);
    assert!(!lock.held);
    let receipt = pump.take_receive().unwrap().unwrap();
    assert_eq!(receipt.observation().physical, PhysicalObservation::Abandoned);
    let writer = match writer.publish(receipt) { Err((_, writer)) => writer, Ok(_) => panic!() };
    assert_eq!(pool.available(), 0);
    drop(writer);
    assert_eq!(pool.available(), 1);
}
