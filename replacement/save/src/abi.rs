//! X-M1 image-5 adapter for the save facade and its completion callbacks.

use super::contract::*;
use super::facade;
use super::ports::*;

const FRAME_STATE: usize = 0x275c;
const FRAME_SUBFRAME_COMMAND: usize = 0x604;
const FRAME_EXTENT_PRESET_SOURCE: usize = 0x5f0;
const FRAME_EXTENT_LAYOUT: usize = 0x578;
const DESCRIPTOR_BASE: usize = 0x27d0;
const DESCRIPTOR_STRIDE: usize = 0x150;
const DESCRIPTOR_SAVE_MODE: usize = 0x04;
const DESCRIPTOR_ZERO_WORD: usize = 0x148;
const EXTENT_NAME: usize = 0x2f64;

const SAVE_STATUS_WORD: usize = 0x0154_d970;
const SAVE_PHASE_ONE_TASK: usize = 0x0155_ccb8;
const SAVE_PHASE_TWO_TASK: usize = 0x0155_ccbc;

extern "C" {
    fn rb_save_service_present() -> u32;
    fn rb_save_service_profile() -> u32;
    fn rb_save_service_byte(index: u32) -> u32;
    fn rb_save_prepare_metadata(frame: *mut u32, pass: u32);
    fn rb_save_extent_preset(preset: u32);
    fn rb_save_extent_name(name: *mut u8, descriptor: *mut u32);
    fn rb_save_extent_copy(name: *mut u8, descriptor: *mut u32);
    fn rb_save_sample_cached_state(selector: u32, out: *mut u32);
    fn rb_save_storage_direct(mode: u32, descriptor: *mut u32, completion: unsafe extern "C" fn(i32));
    fn rb_save_storage_kind_three(mode: u32, descriptor: *mut u32, completion: unsafe extern "C" fn(i32));
    fn rb_save_register_release(completion: unsafe extern "C" fn());
    fn rb_save_current_task() -> u32;
    fn rb_save_wait(mask: u32, out: *mut u32, timeout: u32) -> i32;
    fn rb_save_signal(task: u32, mask: u32) -> i32;
    fn rb_save_stage(a: u32, b: u32);
}

unsafe fn read_word(address: usize) -> u32 {
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

unsafe fn write_word(address: usize, value: u32) {
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

pub struct AbiServices {
    frame: usize,
}

impl AbiServices {
    /// # Safety
    /// `frame` points to the live frame record and the stock save words are mapped.
    pub unsafe fn new(frame: usize) -> Self {
        Self { frame }
    }

    fn frame(&self) -> *mut u32 {
        self.frame as *mut u32
    }

    fn descriptor(&self, bank: DescriptorBank) -> *mut u32 {
        let offset = bank.index() as usize * DESCRIPTOR_STRIDE;
        (self.frame + DESCRIPTOR_BASE + offset) as *mut u32
    }
}

impl FrameAccess for AbiServices {
    fn frame_state(&mut self) -> FrameState {
        FrameState::from_word(unsafe { read_word(self.frame + FRAME_STATE) })
    }

    fn subframe_command(&mut self) -> SubframeCommand {
        SubframeCommand::from_word(unsafe { read_word(self.frame + FRAME_SUBFRAME_COMMAND) })
    }

    fn extent_preset(&mut self) -> ExtentPreset {
        ExtentPreset::from_source(unsafe { read_word(self.frame + FRAME_EXTENT_PRESET_SOURCE) })
    }

    fn extent_layout(&mut self) -> ExtentLayout {
        ExtentLayout::from_word(unsafe { read_word(self.frame + FRAME_EXTENT_LAYOUT) })
    }

    fn write_save_mode(&mut self, bank: DescriptorBank, mode: SaveMode) {
        let address = self.descriptor(bank) as usize + DESCRIPTOR_SAVE_MODE;
        unsafe { write_word(address, mode.word()) }
    }

    fn clear_descriptor_word(&mut self, bank: DescriptorBank) {
        let address = self.descriptor(bank) as usize + DESCRIPTOR_ZERO_WORD;
        unsafe { write_word(address, 0) }
    }
}

impl SelectorService for AbiServices {
    fn present(&mut self) -> bool {
        unsafe { rb_save_service_present() != 0 }
    }

    fn save_dispatch(&mut self) -> SaveDispatch {
        SaveDispatch::from_byte(unsafe { rb_save_service_byte(0xc3) as u8 })
    }

    fn profile(&mut self) -> ServiceProfile {
        ServiceProfile(unsafe { rb_save_service_profile() })
    }
}

impl MetadataService for AbiServices {
    fn prepare_metadata(&mut self, pass: MetadataPass) {
        unsafe { rb_save_prepare_metadata(self.frame(), pass.word()) }
    }
}

impl ExtentService for AbiServices {
    fn set_extent_preset(&mut self, preset: ExtentPreset) {
        unsafe { rb_save_extent_preset(preset.word()) }
    }

    fn name_extent(&mut self, bank: DescriptorBank) {
        let name = (self.frame + EXTENT_NAME) as *mut u8;
        unsafe { rb_save_extent_name(name, self.descriptor(bank)) }
    }

    fn copy_extent_name(&mut self, bank: DescriptorBank) {
        let name = (self.frame + EXTENT_NAME) as *mut u8;
        unsafe { rb_save_extent_copy(name, self.descriptor(bank)) }
    }
}

impl StorageService for AbiServices {
    fn sample_cached_state(&mut self) {
        let mut state = core::mem::MaybeUninit::<u32>::uninit();
        unsafe { rb_save_sample_cached_state(2, state.as_mut_ptr()) }
    }

    fn start(&mut self, variant: StorageVariant, bank: DescriptorBank, completion: SaveCompletion) {
        let descriptor = self.descriptor(bank);
        match variant {
            StorageVariant::Direct => unsafe {
                rb_save_storage_direct(1, descriptor, completion.0)
            },
            StorageVariant::KindThree => unsafe {
                rb_save_storage_kind_three(1, descriptor, completion.0)
            },
        }
    }

    fn register_release(&mut self, completion: ReleaseCompletion) {
        unsafe { rb_save_register_release(completion.0) }
    }
}

impl TaskService for AbiServices {
    fn stage(&mut self, point: StagePoint) {
        unsafe { rb_save_stage(1, point.word()) }
    }

    fn current_task(&mut self) -> Option<TaskId> {
        TaskId::from_word(unsafe { rb_save_current_task() })
    }

    fn wait(&mut self, mask: WaitMask, timeout: WaitTimeout) -> SaveFlags {
        let mut flags = 0u32;
        unsafe {
            rb_save_wait(mask.word(), &mut flags, timeout.word());
        }
        SaveFlags::from_word(flags)
    }

    fn set_save_task(&mut self, slot: SaveTaskSlot, task: Option<TaskId>) {
        let address = save_task_address(slot);
        unsafe { write_word(address, task.map_or(0, TaskId::word)) }
    }
}

fn save_task_address(slot: SaveTaskSlot) -> usize {
    match slot {
        SaveTaskSlot::PhaseOne => SAVE_PHASE_ONE_TASK,
        SaveTaskSlot::PhaseTwo => SAVE_PHASE_TWO_TASK,
    }
}

impl CompletionState for AbiServices {
    fn save_status(&mut self) -> RawStatus {
        RawStatus(unsafe { read_word(SAVE_STATUS_WORD) as i32 })
    }

    fn save_completion(&self) -> SaveCompletion {
        SaveCompletion(save_completion)
    }

    fn release_completion(&self) -> ReleaseCompletion {
        ReleaseCompletion(release_completion)
    }
}

unsafe extern "C" fn save_completion(code: i32) {
    let status = RawStatus::from_completion(CompletionCode(code));
    unsafe { write_word(SAVE_STATUS_WORD, status.0 as u32) };
    let task = unsafe { read_word(SAVE_PHASE_ONE_TASK) };
    if task != 0 {
        unsafe {
            rb_save_signal(task, SignalMask::COMPLETION.word());
        }
    }
}

unsafe extern "C" fn release_completion() {
    let task = unsafe { read_word(SAVE_PHASE_TWO_TASK) };
    if task != 0 {
        unsafe {
            rb_save_signal(task, SignalMask::COMPLETION.word());
        }
    }
}

/// # Safety
/// `frame` is the admitted live frame record; `status_out` is null or one writable `i32`.
#[cfg(not(xm1_save_faulty))]
#[no_mangle]
pub unsafe extern "C" fn xm1_save_facade(
    frame: *mut u32,
    kind: i32,
    status_out: *mut i32,
) -> i32 {
    let mut services = unsafe { AbiServices::new(frame as usize) };
    let out = unsafe { StatusOut::new(status_out) };
    let outcome = facade::run(&mut services, SaveKind::from_abi(kind), out);
    match outcome.returned {
        SaveReturn::Success => 1,
        SaveReturn::Interrupted => 0,
    }
}

/// # Safety
/// Same contract as the positive entry.
#[cfg(xm1_save_faulty)]
#[no_mangle]
pub unsafe extern "C" fn xm1_save_facade(
    frame: *mut u32,
    kind: i32,
    status_out: *mut i32,
) -> i32 {
    let mut services = unsafe { AbiServices::new(frame as usize) };
    let out = unsafe { StatusOut::new(status_out) };
    let outcome = facade::run(&mut services, SaveKind::from_abi(kind), out);
    if outcome.status.is_saved() {
        if let Some(out) = out {
            out.write(RawStatus::SAVED);
        }
        1
    } else {
        0
    }
}
