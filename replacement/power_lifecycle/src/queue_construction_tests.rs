use super::*;

struct Kernel {
    storage: [Option<ServiceAddress>; 240],
    available: u32,
    data_queue: Option<(u32, u32, ServiceAddress)>,
    pool: Option<(u32, u32, u32, ServiceAddress)>,
    metadata: Option<(u32, u32, ServiceAddress)>,
    queue_failure: i32,
    pool_failure: i32,
    error: Option<i32>,
}
impl Kernel {
    fn new(available: u32) -> Self { Self { storage: [None; 240], available, data_queue: None, pool: None, metadata: None, queue_failure: 0, pool_failure: 0, error: None } }
}
impl QueueConstruction for Kernel {
    fn reserve(&mut self, slot: StorageSlot, bytes: u32) -> Result<ServiceAddress, ServiceStatus> {
        if bytes > self.available { return Err(ServiceStatus(-33)); }
        if self.storage[slot.0 as usize].is_some() { return Err(ServiceStatus(-18)); }
        self.available -= bytes;
        let address = ServiceAddress(0x1000 + slot.0 * 0x1000);
        self.storage[slot.0 as usize] = Some(address);
        Ok(address)
    }
    fn release(&mut self, slot: StorageSlot) { self.storage[slot.0 as usize] = None; }
    fn publish_ring(&mut self, _: QueueId, _: ServiceAddress) {}
    fn publish_pool(&mut self, _: QueueId, message_size: u32, bytes: u32, address: ServiceAddress) { self.metadata = Some((message_size, bytes, address)); }
    fn clear_pool(&mut self, _: QueueId) { self.metadata = None; }
    fn create_data_queue(&mut self, queue: QueueId, configuration: &DataQueueConfiguration) -> ServiceStatus {
        if self.queue_failure == 0 { self.data_queue = Some((queue.slot(), configuration.capacity, configuration.ring)); }
        ServiceStatus(self.queue_failure)
    }
    fn delete_data_queue(&mut self, _: QueueId) { self.data_queue = None; }
    fn create_pool(&mut self, queue: QueueId, configuration: &PoolConfiguration) -> ServiceStatus {
        if self.pool_failure == 0 { self.pool = Some((queue.slot(), configuration.blocks, configuration.block_size, configuration.buffer)); }
        ServiceStatus(self.pool_failure)
    }
    fn error(&mut self, _: ConstructionDiagnostic, _: u32, status: ServiceStatus) { self.error = Some(status.0); }
}
#[test]
fn task6_queue_has_48_slots_and_sixteen_byte_pool_blocks() {
    let mut kernel = Kernel::new(4096);
    assert_eq!(create_queue(&mut kernel, 6, QueueConfiguration { flags: 48, message_size: 12 }), ServiceStatus(0));
    assert_eq!(kernel.data_queue, Some((6, 48, ServiceAddress(0x7e000))));
    assert_eq!(kernel.pool, Some((6, 48, 16, ServiceAddress(0x6000))));
    assert_eq!(kernel.metadata, Some((12, 768, ServiceAddress(0x6000))));
    assert_eq!(kernel.error, None);
}
#[test]
fn constructor_payload_sizes_preserve_pool_link_word() {
    for (queue, payload, block) in [(37, 20, 24), (83, 36, 40)] {
        let mut kernel = Kernel::new(4096);
        assert_eq!(create_queue(&mut kernel, queue, QueueConfiguration { flags: 48, message_size: payload }), ServiceStatus(0));
        let (actual_queue, blocks, block_size, _) = kernel.pool.unwrap();
        assert_eq!((actual_queue, blocks, block_size), (queue, 48, block));
    }
}
#[test]
fn failed_pool_allocation_rolls_back_mailbox_and_reservation() {
    let mut kernel = Kernel::new(192);
    assert_eq!(create_queue(&mut kernel, 6, QueueConfiguration { flags: 48, message_size: 12 }), ServiceStatus(-33));
    assert!(kernel.storage.iter().all(Option::is_none));
    assert!(kernel.data_queue.is_none());
    assert!(kernel.pool.is_none());
    assert!(kernel.metadata.is_none());
    assert_eq!(kernel.error, Some(-33));
}
#[test]
fn failed_kernel_creation_releases_only_new_resources() {
    for pool_failure in [false, true] {
        let mut kernel = Kernel::new(4096);
        if pool_failure { kernel.pool_failure = -5; } else { kernel.queue_failure = -5; }
        assert_eq!(create_queue(&mut kernel, 83, QueueConfiguration { flags: 48, message_size: 36 }), ServiceStatus(-5));
        assert!(kernel.storage.iter().all(Option::is_none));
        assert!(kernel.data_queue.is_none());
        assert!(kernel.pool.is_none());
        assert!(kernel.metadata.is_none());
        assert_eq!(kernel.error, Some(-5));
    }
}
#[test]
fn invalid_channel_cannot_reserve_storage() {
    for queue in [0, 121, 0x10000] {
        let mut kernel = Kernel::new(4096);
        assert_eq!(create_queue(&mut kernel, queue, QueueConfiguration { flags: 48, message_size: 12 }), ServiceStatus(-18));
        assert!(kernel.storage.iter().all(Option::is_none));
    }
}
