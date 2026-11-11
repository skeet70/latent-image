use crate::storage_shutdown_actor::MediumStatus;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageCommand { Load, Persist, SavePrimary, SaveExtended, SaveAuxiliary }
impl StorageCommand {
    pub const fn from_abi(value: u16) -> Option<Self> {
        match value {
            0x7400 => Some(Self::Load), 0x7401 => Some(Self::Persist),
            0x7403 => Some(Self::SavePrimary), 0x7404 => Some(Self::SaveExtended),
            0x7405 => Some(Self::SaveAuxiliary), _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleResult { Completed(MediumStatus), Persist }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensCompletion { Ready, Read, Write, Auxiliary, Dma, Extended, Information }
impl LensCompletion {
    pub const fn bits(self) -> u32 {
        match self { Self::Ready => 1, Self::Read => 0x20, Self::Write => 0x10, Self::Auxiliary => 2, Self::Dma => 4, Self::Extended => 0x40, Self::Information => 8 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum LensPageOperation {
    Bytes1, Bytes2, Bytes4, Bytes8, Bytes16, Bytes32,
    Bytes64, Bytes128, Bytes256, Bytes512, Bytes1024, Bytes2048,
}
impl LensPageOperation {
    pub const fn abi(self) -> u32 { self as u32 }
    pub const fn bytes(self) -> u16 { 1 << self.abi() }
}

pub fn transfer_plan(bytes: u32, maximum: LensPageOperation) -> Result<LensTransferPlan, MediumStatus> {
    if bytes == 0 { return Err(MediumStatus(-255)); }
    let operations = [
        LensPageOperation::Bytes1, LensPageOperation::Bytes2, LensPageOperation::Bytes4,
        LensPageOperation::Bytes8, LensPageOperation::Bytes16, LensPageOperation::Bytes32,
        LensPageOperation::Bytes64, LensPageOperation::Bytes128, LensPageOperation::Bytes256,
        LensPageOperation::Bytes512, LensPageOperation::Bytes1024, LensPageOperation::Bytes2048,
    ];
    let operation = operations.into_iter().take(maximum.abi() as usize + 1)
        .find(|operation| u32::from(operation.bytes()) >= bytes).unwrap_or(maximum);
    let page_bytes = operation.bytes();
    let pages = (bytes / u32::from(page_bytes) + u32::from(bytes % u32::from(page_bytes) != 0)) as u16;
    Ok(LensTransferPlan { pages, operation, page_bytes })
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensTransferPlan {
    pub pages: u16,
    pub operation: LensPageOperation,
    pub page_bytes: u16,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LensLoadMode(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LensDmaRegister { Control, Output, Mask }
impl LensDmaRegister {
    pub const fn address(self) -> u32 { match self { Self::Control => 0xfff89080, Self::Output => 0xfff89084, Self::Mask => 0xfff89090 } }
}

pub trait LensEepromSoftware: crate::storage_shutdown_load::LensLoadSoftware {
    fn prepare_service(&mut self);
    fn register_dma_callback(&mut self, enabled: bool);
    fn prepare_primary(&mut self);
    fn prepare_auxiliary(&mut self);
    fn select_auxiliary(&mut self, plan: LensTransferPlan);
    fn select_read(&mut self, plan: LensTransferPlan);
    fn read_ready(&mut self);
    fn request_extended(&mut self, scratch: &mut [u8; 4]);
    fn auxiliary_size(&mut self) -> u16;
    fn write_dma_register(&mut self, register: LensDmaRegister, value: u32);
}

enum DmaCleanup { Callback, Pulse }

struct DmaRegistration<'a, S: LensEepromSoftware> {
    software: &'a mut S,
    cleanup: DmaCleanup,
}
impl<'a, S: LensEepromSoftware> DmaRegistration<'a, S> {
    fn new(software: &'a mut S, cleanup: DmaCleanup) -> Self {
        software.register_dma_callback(true);
        Self { software, cleanup }
    }
    fn software(&mut self) -> &mut S { self.software }
}
impl<S: LensEepromSoftware> Drop for DmaRegistration<'_, S> {
    fn drop(&mut self) {
        self.software.register_dma_callback(false);
        if let DmaCleanup::Pulse = self.cleanup {
            self.software.write_dma_register(LensDmaRegister::Control, 0x10001000);
            self.software.write_dma_register(LensDmaRegister::Mask, 0x03000000);
            self.software.write_dma_register(LensDmaRegister::Output, 0x10001000);
        }
    }
}

fn word(context: &[u8; 0xc0], offset: usize) -> u32 {
    u32::from_le_bytes([context[offset], context[offset + 1], context[offset + 2], context[offset + 3]])
}
fn put_half(context: &mut [u8; 0xc0], offset: usize, value: u16) {
    context[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
async fn wait_success(software: &mut impl LensEepromSoftware, event: LensCompletion, context: &mut [u8; 0xc0]) -> bool {
    software.wait(event, context).await.0 == 0 && word(context, 0x14) == 0
}

async fn configure_access(software: &mut impl LensEepromSoftware, context: &mut [u8; 0xc0]) -> bool {
    software.clear_notifications();
    software.prepare_primary();
    wait_success(software, LensCompletion::Ready, context).await
}

pub async fn wait_dma(software: &mut impl LensEepromSoftware, context: &mut [u8; 0xc0]) -> MediumStatus {
    let mut registration = DmaRegistration::new(software, DmaCleanup::Pulse);
    let software = registration.software();
    software.write_dma_register(LensDmaRegister::Control, 0x10000000);
    software.write_dma_register(LensDmaRegister::Mask, 0x03000000);
    software.write_dma_register(LensDmaRegister::Output, 0x10001000);
    let status = software.wait(LensCompletion::Dma, context).await;
    if status.0 == 0 { MediumStatus(0) } else { MediumStatus(-255) }
}

pub async fn dispatch(
    command: StorageCommand,
    mode: LensLoadMode,
    context: &mut [u8; 0xc0],
    cache: &[u8; 0xa08],
    transfer: &mut [u8; 0x800],
    scratch: &mut [u8; 4],
    software: &mut impl LensEepromSoftware,
) -> LifecycleResult {
    let result = dispatch_operation(command, mode, context, cache, transfer, scratch, software).await;
    if let LifecycleResult::Completed(status) = result {
        if status.0 != 0 { return result; }
    }
    let status = software.software_status();
    if status.0 == 0 { result } else { LifecycleResult::Completed(status) }
}

async fn dispatch_operation(
    command: StorageCommand,
    mode: LensLoadMode,
    context: &mut [u8; 0xc0],
    cache: &[u8; 0xa08],
    transfer: &mut [u8; 0x800],
    scratch: &mut [u8; 4],
    software: &mut impl LensEepromSoftware,
) -> LifecycleResult {
    use crate::storage_shutdown_pages::{self, ConfigurationAddress, PageBoundary};
    let failed = LifecycleResult::Completed(MediumStatus(-255));
    match command {
        StorageCommand::Persist => LifecycleResult::Persist,
        StorageCommand::Load if mode.0 != 6 => LifecycleResult::Completed(crate::storage_shutdown_load::load_profile(software, cache, context, transfer, mode).await),
        StorageCommand::Load => {
            software.prepare_service();
            if !configure_access(software, context).await { return failed; }
            context[0x34..0x38].copy_from_slice(&0u32.to_le_bytes());
            put_half(context, 0x38, 0xff00);
            software.clear_notifications();
            software.read_ready();
            if !wait_success(software, LensCompletion::Ready, context).await || wait_dma(software, context).await.0 != 0 { return failed; }
            let Ok(plan) = transfer_plan(u32::from(software.auxiliary_size()), LensPageOperation::Bytes1024) else { return failed; };
            software.select_read(plan);
            if !wait_success(software, LensCompletion::Ready, context).await { return failed; }
            put_half(context, 0x18, plan.pages);
            put_half(context, 0x1a, if plan.pages == 1 { 0xffff } else { 1 });
            LifecycleResult::Completed(storage_shutdown_pages::read_auxiliary_pages(software, context, transfer, plan).await)
        }
        StorageCommand::SavePrimary => {
            software.prepare_service();
            let mut registration = DmaRegistration::new(software, DmaCleanup::Callback);
            let software = registration.software();
            software.clear_notifications();
            software.prepare_primary();
            let status = if !wait_success(software, LensCompletion::Ready, context).await {
                MediumStatus(-255)
            } else {
                software.clear_notifications();
                software.prepare_auxiliary();
                if !wait_success(software, LensCompletion::Auxiliary, context).await {
                    MediumStatus(-255)
                } else {
                    put_half(context, 0x18, 4);
                    storage_shutdown_pages::write_pages(software, context, ConfigurationAddress(0x3e0000), 0x800, LensPageOperation::Bytes2048, Some(PageBoundary::Primary)).await
                }
            };
            if status.0 == 0 { LifecycleResult::Persist } else { LifecycleResult::Completed(status) }
        }
        StorageCommand::SaveExtended => {
            let status = crate::storage_shutdown_load::load_profile(software, cache, context, transfer, mode).await;
            if status.0 != 0 || mode.0 == 1 { return LifecycleResult::Completed(status); }
            software.prepare_service();
            let mut registration = DmaRegistration::new(software, DmaCleanup::Callback);
            let software = registration.software();
            software.clear_notifications();
            software.prepare_primary();
            let succeeded = if wait_success(software, LensCompletion::Ready, context).await {
                scratch[0] = 1;
                software.request_extended(scratch);
                software.wait(LensCompletion::Extended, context).await.0 == 0
                    && wait_success(software, LensCompletion::Dma, context).await
            } else { false };
            if succeeded { LifecycleResult::Completed(MediumStatus(0)) } else { failed }
        }
        StorageCommand::SaveAuxiliary => {
            software.prepare_service();
            if !configure_access(software, context).await { return failed; }
            let Ok(plan) = transfer_plan(u32::from(software.auxiliary_size()), LensPageOperation::Bytes1024) else { return failed; };
            software.clear_notifications();
            software.select_auxiliary(plan);
            if !wait_success(software, LensCompletion::Auxiliary, context).await { return failed; }
            put_half(context, 0x18, plan.pages);
            LifecycleResult::Completed(storage_shutdown_pages::write_pages(software, context, ConfigurationAddress(0x3e5000), u32::from(plan.page_bytes), plan.operation, Some(PageBoundary::Auxiliary)).await)
        }
    }
}
