//! Retained services. The adapter owns their addresses; the save module owns
//! the call order.

use super::contract::*;

pub trait FrameAccess {
    fn frame_word(&mut self, field: FrameField) -> u32;
    fn write_save_mode(&mut self, bank: DescriptorBank, mode: SaveMode);
    fn clear_descriptor_word(&mut self, bank: DescriptorBank);
}

/// `0x0073abec`, `0x0073ac60` and `0x0069f8fc`.
pub trait SelectorService {
    fn present(&mut self) -> bool;
    fn configuration_byte(&mut self, index: ConfigByte) -> u8;
    fn profile(&mut self) -> ServiceProfile;
}

/// `0x00e4e0b4`.
pub trait MetadataService {
    fn prepare_metadata(&mut self, pass: MetadataPass);
}

/// The retained extent helpers `0x011d5850`, `0x011d5934` and `0x011d591c`.
pub trait ExtentService {
    fn set_extent_preset(&mut self, preset: ExtentPreset);
    fn name_extent(&mut self, bank: DescriptorBank);
    fn copy_extent_name(&mut self, bank: DescriptorBank);
}

/// `0x007101e4`, `0x011d5600`, `0x011d5688` and `0x011d5714`.
pub trait StorageService {
    fn sample_cached_state(&mut self);
    fn start(&mut self, variant: StorageVariant, bank: DescriptorBank, completion: SaveCompletion);
    fn register_release(&mut self, completion: ReleaseCompletion);
}

/// `0x006fae80`, `0x00723cb8`, `0x007243a8` and `0x00724558`.
pub trait TaskService {
    fn stage(&mut self, point: StagePoint);
    fn current_task(&mut self) -> Option<TaskId>;
    fn wait(&mut self, mask: WaitMask, timeout: WaitTimeout) -> SaveFlags;
    fn set_save_task(&mut self, slot: SaveTaskSlot, task: Option<TaskId>);
}

pub trait CompletionState {
    fn save_status(&mut self) -> RawStatus;
    fn save_completion(&self) -> SaveCompletion;
    fn release_completion(&self) -> ReleaseCompletion;
}

pub trait SaveServices:
    FrameAccess + SelectorService + MetadataService + ExtentService + StorageService + TaskService + CompletionState
{
}

impl<T> SaveServices for T where
    T: FrameAccess
        + SelectorService
        + MetadataService
        + ExtentService
        + StorageService
        + TaskService
        + CompletionState
{
}
