use crate::shutdown::{KernelStatus, QueueId, RequestId, ServiceMessage, ShutdownRtos};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct StorageCompletion(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StoragePostError;
impl StoragePostError {
    pub const fn abi_status(self) -> i32 { -255 }
}

pub fn post_shutdown(
    rtos: &mut impl ShutdownRtos,
    completion: StorageCompletion,
    scratch: &mut [u8; 8],
) -> Result<(), StoragePostError> {
    scratch[..4].copy_from_slice(&completion.0.to_le_bytes());
    let sender = rtos.current_task();
    rtos.post(ServiceMessage { operation: RequestId(0x7401), sender, destination: QueueId(100), payload: scratch })
        .map_err(|_| StoragePostError)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StorageRequest {
    command: crate::storage_shutdown_lifecycle::StorageCommand,
    callback: StorageCompletion,
    mode: crate::storage_shutdown_lifecycle::LensLoadMode,
}
impl StorageRequest {
    pub fn parse(operation: RequestId, payload: &[u8]) -> Result<Option<Self>, KernelStatus> {
        use crate::storage_shutdown_lifecycle::{LensLoadMode, StorageCommand};
        let Some(command) = StorageCommand::from_abi(operation.0) else { return Ok(None); };
        let needed = if command == StorageCommand::Persist { 4 } else { 8 };
        if payload.len() < needed { return Err(KernelStatus(-60)); }
        let callback = StorageCompletion(u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]));
        let mode = LensLoadMode(if command == StorageCommand::Persist { 0 } else {
            u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]])
        });
        Ok(Some(Self { command, callback, mode }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct NandBlock(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MediumStatus(pub i32);
pub trait LensProfileMedium {
    fn read_cache(&mut self, block: NandBlock, destination: &mut [u8; 0xa08]) -> MediumStatus;
    fn erase_blocks(&mut self, first: NandBlock, count: u32) -> MediumStatus;
    fn write_profile(&mut self, first: NandBlock, configuration: &[u8; 0x40000]) -> MediumStatus;
    fn write_cache(&mut self, first: NandBlock, cache: &[u8; 0xa08]) -> MediumStatus;
}

pub trait StorageCompletionSink {
    fn complete(&mut self, completion: StorageCompletion, status: MediumStatus);
}

pub struct LensSnapshot(pub [u8; 82]);
pub struct ProfileConfiguration {
    pub bytes: [u8; 10],
}

pub trait LensProfileSource {
    fn snapshot(&self) -> LensSnapshot;
    fn configuration(&self) -> ProfileConfiguration;
    fn profile(&self) -> &[u8; 0x40000];
}

pub struct StorageShutdownActor<'a> {
    cache: &'a mut [u8; 0xa08],
    context: &'a mut [u8; 0xc0],
    transfer: &'a mut [u8; 0x800],
    profile_first: NandBlock,
    cache_first: NandBlock,
}

impl<'a> StorageShutdownActor<'a> {
    pub fn new(
        cache: &'a mut [u8; 0xa08],
        context: &'a mut [u8; 0xc0],
        transfer: &'a mut [u8; 0x800],
        profile_partition: NandBlock,
        cache_partition: NandBlock,
    ) -> Self {
        Self {
            cache, context, transfer,
            profile_first: NandBlock(profile_partition.0.wrapping_sub(0x380)),
            cache_first: NandBlock(cache_partition.0.wrapping_sub(0x380)),
        }
    }

    pub fn initialize(&mut self, medium: &mut impl LensProfileMedium) {
        self.transfer.fill(0);
        self.context.fill(0);
        let payload = (self.context.as_mut_ptr() as usize as u32).wrapping_add(12);
        self.context[8..12].copy_from_slice(&payload.to_le_bytes());
        self.cache.fill(0);
        let _ = medium.read_cache(self.cache_first, &mut self.cache);
        if self.cache[4..8] != 0xabc123f9u32.to_le_bytes() {
            self.cache[..3].fill(0);
            self.cache[4..8].copy_from_slice(&0xabc123f9u32.to_le_bytes());
        } else if self.cache[2] > 20 || self.cache[0] > 19 || self.cache[1] > 19 {
            self.cache[..3].fill(0);
        }
        self.context[0x3c..0x40].copy_from_slice(&1u32.to_le_bytes());
    }

    pub fn context(&mut self) -> &mut [u8; 0xc0] { &mut self.context }
    pub fn cache(&self) -> &[u8; 0xa08] { &self.cache }

    fn refresh(&mut self, slot: usize, snapshot: &LensSnapshot, configuration: &ProfileConfiguration) {
        let base = slot * 0x80;
        self.cache[base + 8..base + 16].copy_from_slice(&snapshot.0[8..16]);
        self.cache[base + 0x18..base + 0x20].copy_from_slice(&snapshot.0[..8]);
        self.cache[base + 0x20..base + 0x2a].copy_from_slice(&snapshot.0[72..82]);
        self.cache[base + 0x30..base + 0x38].copy_from_slice(&configuration.bytes[..8]);
        for (offset, value) in [(0x38, 0x3e0000u32), (0x3c, 0x3e0000), (0x40, 0x2000), (0x44, 0x3e2000), (0x48, 0x13d00), (0x4c, 0x15d00)] {
            self.cache[base + offset..base + offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        self.cache[base + 0x50..base + 0x52].copy_from_slice(&configuration.bytes[8..]);
    }

    pub fn persist(
        &mut self,
        medium: &mut impl LensProfileMedium,
        source: &impl LensProfileSource,
    ) -> MediumStatus {
        let snapshot = source.snapshot();
        if self.context[0x3c..0x40] == 1u32.to_le_bytes() { return MediumStatus(0); }
        let profile = source.profile();
        let count = usize::from(self.cache[2]);
        let found = (0..count).find(|&slot| {
            let base = slot * 0x80;
            self.cache[base + 0x18..base + 0x20] == snapshot.0[..8]
                && self.cache[base + 0x20..base + 0x2a] == snapshot.0[72..82]
        });
        let slot = found.unwrap_or(if count == 20 { usize::from(self.cache[1]) } else { count });
        self.refresh(slot, &source.snapshot(), &source.configuration());
        let head = usize::from(self.cache[0]);
        if count == 0 {
            self.cache[0x86] = 0xff;
            self.cache[0x87] = 0xff;
            self.cache[0] = 0;
            self.cache[1] = 0;
            self.cache[2] = 1;
        } else if found.is_none() && count < 20 {
            self.cache[head * 0x80 + 0x86] = slot as u8;
            let tail = usize::from(self.cache[1]);
            self.cache[tail * 0x80 + 0x87] = 0xff;
            self.cache[slot * 0x80 + 0x86] = 0xff;
            self.cache[slot * 0x80 + 0x87] = head as u8;
            self.cache[0] = slot as u8;
            self.cache[2] = (count + 1) as u8;
        } else if slot != head {
            let previous = usize::from(self.cache[slot * 0x80 + 0x86]);
            let next = usize::from(self.cache[slot * 0x80 + 0x87]);
            if slot == usize::from(self.cache[1]) {
                self.cache[1] = previous as u8;
                self.cache[previous * 0x80 + 0x87] = 0xff;
            } else {
                self.cache[next * 0x80 + 0x86] = previous as u8;
            }
            self.cache[head * 0x80 + 0x86] = slot as u8;
            if next != 0xff { self.cache[previous * 0x80 + 0x87] = next as u8; }
            self.cache[slot * 0x80 + 0x86] = 0xff;
            self.cache[slot * 0x80 + 0x87] = head as u8;
            self.cache[0] = slot as u8;
        }
        let profile_first = NandBlock(self.profile_first.0.wrapping_add((slot as u32) * 2));
        let status = medium.erase_blocks(self.cache_first, 1);
        if status.0 != 0 { return status; }
        let status = medium.erase_blocks(profile_first, 2);
        if status.0 != 0 { return status; }
        let status = medium.write_profile(profile_first, profile);
        if status.0 != 0 { return status; }
        medium.write_cache(self.cache_first, &self.cache)
    }

    pub async fn dispatch_message(
        &mut self,
        request: StorageRequest,
        scratch: &mut [u8; 4],
        lens: &mut impl crate::storage_shutdown_lifecycle::LensEepromSoftware,
        medium: &mut impl LensProfileMedium,
        source: &impl LensProfileSource,
        completion: &mut impl StorageCompletionSink,
    ) -> MediumStatus {
        use crate::storage_shutdown_lifecycle::{self, LifecycleResult};
        let status = match storage_shutdown_lifecycle::dispatch(request.command, request.mode, self.context, self.cache, self.transfer, scratch, lens).await {
            LifecycleResult::Completed(status) => status,
            LifecycleResult::Persist => self.persist(medium, source),
        };
        if request.callback.0 != 0 { completion.complete(request.callback, status); }
        status
    }
}

#[cfg(target_arch = "arm")]
pub mod native {
    use super::{LensProfileSource, LensSnapshot, NandBlock, ProfileConfiguration, StorageCompletion, StorageCompletionSink, StorageRequest, StorageShutdownActor};
    extern "C" {
        fn xm1_power_interrupt_lock() -> u32;
        fn xm1_power_interrupt_unlock(saved: u32);
        fn xm1_power_configuration_byte(offset: u32) -> u32;
        fn xm1_power_storage_shutdown_callback() -> i32;
    }

    pub struct NativeProfileSource;
    impl LensProfileSource for NativeProfileSource {
        fn snapshot(&self) -> LensSnapshot { unsafe { snapshot() } }
        fn configuration(&self) -> ProfileConfiguration { unsafe { configuration() } }
        fn profile(&self) -> &[u8; 0x40000] { unsafe { profile() } }
    }

    pub async unsafe fn run_task100(
        lens: &mut impl crate::storage_shutdown_lifecycle::LensEepromSoftware,
        completion: &mut impl StorageCompletionSink,
        scratch: &mut [u8; 4],
    ) -> ! {
        use crate::shutdown::QueueId;
        use crate::shutdown_receive::{ReceiveDescriptor, ReceiveTimeout};
        use crate::shutdown_rtos::NativeShutdownRtos;
        let mut actor = unsafe { adopt() };
        let mut medium = crate::storage_shutdown_native::medium();
        actor.initialize(&mut medium);
        loop {
            let descriptor = actor.context().as_mut_ptr().cast::<ReceiveDescriptor>();
            if unsafe { NativeShutdownRtos.receive(QueueId(100), &mut *descriptor, ReceiveTimeout::Forever) }.is_err() { continue; }
            let descriptor = unsafe { &*descriptor };
            let payload = unsafe { core::slice::from_raw_parts(descriptor.payload.0 as *const u8, 8) };
            if let Ok(Some(request)) = StorageRequest::parse(descriptor.header().operation, payload) {
                actor.dispatch_message(request, scratch, lens, &mut medium, &NativeProfileSource, completion).await;
            }
        }
    }

    pub unsafe fn adopt<'a>() -> StorageShutdownActor<'a> {
        unsafe {
            StorageShutdownActor::new(
                &mut *(0x919b40 as *mut [u8; 0xa08]),
                &mut *(0x956774 as *mut [u8; 0xc0]),
                &mut *(0x955e90 as *mut [u8; 0x800]),
                NandBlock(core::ptr::read_volatile((0x7c8c6c + 0x58) as *const u32)),
                NandBlock(core::ptr::read_volatile((0x7c8c6c + 0x64) as *const u32)),
            )
        }
    }

    pub unsafe fn profile<'a>() -> &'a [u8; 0x40000] {
        let base = unsafe { core::ptr::read_volatile(0x96ca50 as *const u32) };
        unsafe { &*(base.wrapping_add(0x3e0000) as *const [u8; 0x40000]) }
    }

    pub unsafe fn snapshot() -> LensSnapshot {
        let mut snapshot = LensSnapshot([0; 82]);
        let saved = unsafe { xm1_power_interrupt_lock() };
        for (index, byte) in snapshot.0.iter_mut().enumerate() {
            *byte = unsafe { core::ptr::read_volatile((0x955970 + index) as *const u8) };
        }
        unsafe { xm1_power_interrupt_unlock(saved) };
        snapshot
    }

    pub unsafe fn configuration() -> ProfileConfiguration {
        let mut bytes = [0; 10];
        for (byte, key) in bytes.iter_mut().zip([0x3e0021, 0x3e0022, 0x3e001f, 0x3e0020, 0x3e0023, 0x3e0024, 0x3e0025, 0x3e0026, 0x204, 0x205]) {
            *byte = unsafe { xm1_power_configuration_byte(key) } as u8;
        }
        ProfileConfiguration { bytes }
    }

    pub struct ShutdownCompletion;
    impl ShutdownCompletion {
        pub fn address() -> StorageCompletion {
            StorageCompletion(xm1_power_storage_shutdown_callback as *const () as usize as u32)
        }
    }
}
