use super::*;
use std::collections::BTreeMap;

#[derive(Default)]
struct World {
    memory: BTreeMap<u32, u32>,
    engine_requests: Vec<EngineOperation>,
    deferred: bool,
}
impl Runtime for World {
    fn read16(&self, address: Address) -> u16 { self.read32(address) as u16 }
    fn read32(&self, address: Address) -> u32 { self.memory.get(&address.0).copied().unwrap_or(0) }
    fn write16(&mut self, address: Address, value: u16) { self.memory.insert(address.0, u32::from(value)); }
    fn write32(&mut self, address: Address, value: u32) { self.memory.insert(address.0, value); }
    fn submit_engine(&mut self, operation: EngineOperation) { self.engine_requests.push(operation); }
    fn defer_current(&mut self) { self.deferred = true; }
}

#[test]
fn shutdown_waits_for_actual_suspend_completion() {
    let mut world = World::default();
    world.write16(RESULT, 2);
    assert_eq!(request(&mut world, [2, 0]), Ok(0));
    assert_eq!(world.read16(RESULT), 3);
    assert_eq!(world.engine_requests, [EngineOperation::Suspend]);
    world.write16(RECORDING, 0x13e7);
    assert_eq!(request(&mut world, [3, 0]), Ok(0));
    assert_eq!(world.read16(RESULT), 3);
    world.write16(RECORDING, 0x13e6);
    assert_eq!(request(&mut world, [3, 0]), Ok(1));
    assert_eq!(world.read16(RESULT), 0);
}

#[test]
fn resume_requires_matching_completion_before_suspend() {
    let mut world = World::default();
    assert_eq!(request(&mut world, [1, 0x1234]), Ok(0));
    assert_eq!(world.read32(Address(REQUEST.0 + 4)), 0x1234);
    assert_eq!(world.read16(RESULT), 1);
    assert_eq!(world.engine_requests, [EngineOperation::Resume]);
    assert_eq!(request(&mut world, [2, 0]), Ok(0));
    assert!(world.deferred);
    assert_eq!(world.engine_requests, [EngineOperation::Resume]);
    world.write16(RECORDING, 0x13e5);
    assert_eq!(request(&mut world, [3, 0]), Ok(1));
    assert_eq!(world.read16(RESULT), 2);
    assert_eq!(request(&mut world, [2, 0]), Ok(0));
    assert_eq!(world.engine_requests, [EngineOperation::Resume, EngineOperation::Suspend]);
}

#[test]
fn idle_shutdown_finishes_without_device_request() {
    let mut world = World::default();
    assert_eq!(request(&mut world, [2, 0]), Ok(1));
    assert_eq!(world.read16(RESULT), 0);
    assert!(world.engine_requests.is_empty());
}

#[test]
fn unsupported_request_preserves_engine_state_and_selection() {
    let mut world = World::default();
    world.write16(RESULT, 2);
    world.write16(SELECTION, 4);
    assert_eq!(request(&mut world, [u32::MAX, 7]), Ok(0));
    assert_eq!(world.read16(RESULT), 2);
    assert_eq!(world.read16(SELECTION), 4);
    assert!(world.engine_requests.is_empty());
}

#[test]
fn invalid_published_state_is_not_reported_complete() {
    let mut world = World::default();
    world.write16(RESULT, 4);
    assert_eq!(request(&mut world, [2, 0]), Err(DispatchError::InvalidState(4)));
    assert!(world.engine_requests.is_empty());
}
