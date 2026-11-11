use super::{PoolError, ReceivePool};
use super::super::{HostObservation, HostOperationId, PhysicalObservation, ReceiveRead};
use crate::sd_controller::{Completion, PhysicalResult, VendorCode};
use crate::vendor_wireless_station_driver::receive::{ReceiveError, ReceivePlan, ReceivePort};

fn plan() -> ReceivePlan {
    let mut snapshot = [0; 88];
    snapshot[12] = 2;
    snapshot[16] = 4;
    snapshot[18] = 4;
    ReceivePlan::from_interrupt_snapshot(&snapshot, ReceivePort::Zero, 2).unwrap()
}
fn observation() -> HostObservation {
    HostObservation { operation: HostOperationId(1), physical: PhysicalObservation::Completed(Completion {
        code: VendorCode::Success, physical: PhysicalResult::Completed, transferred: 16, status: 0, response: 0 }), release_error: None }
}
fn prepare(writer: &mut super::AggregateWriteLease<'_, '_, 1>) {
    let bytes = writer.buffer_mut();
    bytes[..16].copy_from_slice(&[4, 0, 2, 1, 0, 0, 0, 0, 4, 0, 2, 2, 0, 0, 0, 0]);
}

#[test]
fn aggregate_is_not_recycled_until_the_last_packet_is_released() {
    let mut bytes = [0; 32];
    let pool = ReceivePool::new([bytes.as_mut_slice()]);
    let mut writer = pool.take().unwrap();
    let identity = writer.identity();
    prepare(&mut writer);
    let receipt = ReceiveRead { aggregate: identity, plan: plan(), cancelled: false, observation: observation() };
    let mut batch = match writer.publish(receipt) { Ok(batch) => batch, Err(_) => panic!() };
    let first = batch.next().unwrap();
    let second = batch.next().unwrap();
    drop(batch);
    assert_eq!(pool.available(), 0);
    assert_eq!(first.payload(), &[4, 0, 2, 1]);
    drop(first);
    assert_eq!(pool.available(), 0);
    assert_eq!(second.payload(), &[4, 0, 2, 2]);
    drop(second);
    assert_eq!(pool.available(), 1);
    assert_ne!(pool.take().unwrap().identity(), identity);
}

#[test]
fn delayed_completion_cannot_publish_a_reused_aggregate() {
    let mut bytes = [0; 32];
    let pool = ReceivePool::new([bytes.as_mut_slice()]);
    let writer = pool.take().unwrap();
    let old_identity = writer.identity();
    drop(writer);
    let writer = pool.take().unwrap();
    let receipt = ReceiveRead { aggregate: old_identity, plan: plan(), cancelled: false, observation: observation() };
    let (error, writer) = match writer.publish(receipt) { Err(failure) => failure, Ok(_) => panic!() };
    assert_eq!(error, PoolError::Stale);
    assert_eq!(pool.available(), 0);
    drop(writer);
    assert_eq!(pool.available(), 1);
}

#[test]
fn malformed_packet_returns_the_exclusive_aggregate_lease() {
    let mut bytes = [0; 32];
    let pool = ReceivePool::new([bytes.as_mut_slice()]);
    let mut writer = pool.take().unwrap();
    prepare(&mut writer);
    writer.buffer_mut()[0] = 5;
    let receipt = ReceiveRead { aggregate: writer.identity(), plan: plan(), cancelled: false, observation: observation() };
    let (error, writer) = match writer.publish(receipt) { Err(failure) => failure, Ok(_) => panic!() };
    assert_eq!(error, PoolError::Receive(ReceiveError::InvalidLength));
    assert_eq!(pool.available(), 0);
    drop(writer);
    assert_eq!(pool.available(), 1);
}
