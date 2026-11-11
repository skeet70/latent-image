use core::{marker::PhantomData, ptr::{read_volatile, write_volatile}};
use crate::shutdown::{post_lifecycle, KernelStatus, LifecycleRequest, ShutdownRtos, TransitionToken};

pub const FRAME_BYTES: usize = 0x117ec;

#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleOperation { Save = 0x1040, Terminal = 0x1080 }

#[repr(u32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModalController { Save = 0x25, Terminal = 0x28 }

#[repr(u16)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModalEvent { Enter = 0, Complete = 1 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModalSelection { pub controller: ModalController, pub event: ModalEvent }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowerOffGate { NotSelected, Suppressed, Selected }

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingMode(pub u32);

pub fn power_off_gate(outer: crate::shutdown::OuterState, pending: PendingMode) -> PowerOffGate {
    if outer.abi() != 10 { PowerOffGate::NotSelected }
    else if matches!(pending.0, 12 | 13 | 14) { PowerOffGate::Suppressed }
    else { PowerOffGate::Selected }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModalResult(pub u32);

pub struct ModeFrontend<'a> {
    frame: *mut u8,
    lifetime: PhantomData<&'a mut [u8; FRAME_BYTES]>,
}

impl<'a> ModeFrontend<'a> {
    pub fn new(frame: &'a mut [u8; FRAME_BYTES]) -> Self {
        Self { frame: frame.as_mut_ptr(), lifetime: PhantomData }
    }

    /// The shared frame at 0x944580 must be mapped and exclusively accessible.
    pub unsafe fn native() -> Self {
        Self { frame: 0x944580 as *mut u8, lifetime: PhantomData }
    }

    fn byte(&self, offset: usize) -> u8 { unsafe { read_volatile(self.frame.add(offset)) } }
    fn put_byte(&mut self, offset: usize, value: u8) { unsafe { write_volatile(self.frame.add(offset), value) } }
    fn half(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.byte(offset), self.byte(offset + 1)])
    }
    fn put_half(&mut self, offset: usize, value: u16) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.put_byte(offset + index, byte); }
    }
    fn word(&self, offset: usize) -> u32 {
        u32::from_le_bytes([self.byte(offset), self.byte(offset + 1), self.byte(offset + 2), self.byte(offset + 3)])
    }
    fn put_word(&mut self, offset: usize, value: u32) {
        for (index, byte) in value.to_le_bytes().into_iter().enumerate() { self.put_byte(offset + index, byte); }
    }

    pub fn normalized_save(&self) -> Option<ModalSelection> {
        let event = if self.word(0xb0cc) == 2 {
            ModalEvent::Enter
        } else if self.word(0) == 0x816 && self.word(4) == u32::from(self.half(0xb15a)) {
            ModalEvent::Complete
        } else { return None; };
        Some(ModalSelection { controller: ModalController::Save, event })
    }

    pub fn normalized_terminal(&self) -> Option<ModalSelection> {
        (self.word(0xb0cc) == 5).then_some(ModalSelection {
            controller: ModalController::Terminal, event: ModalEvent::Enter,
        })
    }

    pub fn issue_token(&mut self, operation: LifecycleOperation) -> TransitionToken {
        let code = operation as u16;
        let index = usize::from((code >> 6) & 0x3f);
        let offset = 0xb158 + index * 2;
        let previous = self.half(offset);
        let sequence = ((previous & 0x3f) + 1) & 0x3f;
        self.put_half(offset, (previous & !0x3f) + sequence);
        let bitmap_offset = 0xb308 + (index / 32) * 4;
        self.put_word(bitmap_offset, self.word(bitmap_offset) | (1u32 << (index % 32)));
        self.put_byte(0xb278 + index, self.byte(0xb278 + index).wrapping_add(1));
        TransitionToken(u32::from(code | sequence))
    }

    pub fn save_action(
        &mut self,
        rtos: &mut impl ShutdownRtos,
        caller_payload: &mut [u8; 16],
    ) -> Result<ModalResult, KernelStatus> {
        let token = self.issue_token(LifecycleOperation::Save);
        let result = post_lifecycle(rtos, LifecycleRequest::Save(token), caller_payload);
        self.record_status(&result);
        result.map(|()| ModalResult(2))
    }

    pub fn terminal_action(
        &mut self,
        rtos: &mut impl ShutdownRtos,
        caller_payload: &mut [u8; 16],
    ) -> Result<ModalResult, KernelStatus> {
        let token = self.issue_token(LifecycleOperation::Terminal);
        let result = post_lifecycle(rtos, LifecycleRequest::Terminal { argument: token.0 }, caller_payload);
        self.record_status(&result);
        result.map(|()| ModalResult(u32::MAX))
    }

    fn record_status(&mut self, result: &Result<(), KernelStatus>) {
        self.put_word(0x107e8, match result { Ok(()) => 0, Err(status) => status.0 as u32 });
    }
}

unsafe extern "C" { fn rb_modal_dispatch(controller: u32) -> u32; }

/// The native shared frame, modal tables, and owned modal dispatcher must be mapped.
#[no_mangle]
pub unsafe extern "C" fn rb_power_normalize_save() -> u32 {
    let frontend = unsafe { ModeFrontend::native() };
    let Some(selection) = frontend.normalized_save() else { return 0; };
    unsafe { write_volatile((0x14676e4 + 0x25 * 2) as *mut u16, selection.event as u16) };
    unsafe { rb_modal_dispatch(selection.controller as u32) }
}

/// The requirements of `rb_power_normalize_save` apply.
#[no_mangle]
pub unsafe extern "C" fn rb_power_normalize_terminal() -> u32 {
    let frontend = unsafe { ModeFrontend::native() };
    let Some(selection) = frontend.normalized_terminal() else { return 0; };
    unsafe { write_volatile((0x14676e4 + 0x28 * 2) as *mut u16, selection.event as u16) };
    unsafe { rb_modal_dispatch(selection.controller as u32) }
}

#[cfg(target_arch = "arm")]
pub unsafe fn full_power_off(
    _: crate::shutdown::PowerOffInput,
    outer: crate::shutdown::OuterState,
) -> PowerOffGate {
    let mut frontend = unsafe { ModeFrontend::native() };
    let gate = power_off_gate(outer, PendingMode(frontend.word(0xbb78)));
    if gate != PowerOffGate::Selected { return gate; }
    unsafe { crate::mode_initialization::request_power_off_mode() };
    let _ = unsafe { crate::mode_events::post_application(
        crate::mode_events::ApplicationRequest(0xa05),
        crate::mode_events::ApplicationArgument(0),
    ) };
    frontend.put_byte(0xbb90, 1);
    unsafe {
        let token = crate::mode_initialization::active_mode_token();
        crate::mode_initialization::store_mode_token(token);
    }
    for offset in 0..80 {
        frontend.put_byte(0xbb1c + offset, frontend.byte(offset));
    }
    PowerOffGate::Selected
}

#[cfg(target_arch = "arm")]
core::arch::global_asm!(include_str!("mode_frontend_residue.S"), options(raw));

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn rb_power_save_action_backed(payload: *mut [u8; 16]) -> u32 {
    let _ = unsafe { crate::mode_initialization::setting(crate::mode_initialization::SettingId::Setting175) };
    let result = unsafe { ModeFrontend::native() }.save_action(
        &mut crate::shutdown_rtos::NativeShutdownRtos, unsafe { &mut *payload },
    );
    if result.is_err() { unsafe { crate::mode_events::xm1_power_frontend_failure() }; }
    2
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn rb_power_terminal_action_backed(payload: *mut [u8; 16]) -> u32 {
    let result = unsafe { ModeFrontend::native() }.terminal_action(
        &mut crate::shutdown_rtos::NativeShutdownRtos, unsafe { &mut *payload },
    );
    if result.is_err() { unsafe { crate::mode_events::xm1_power_frontend_failure() }; }
    u32::MAX
}

#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn rb_power_mode_action() -> u32 {
    if let Some(input) = crate::shutdown::PowerOffInput::from_key_value(6, 0x80) {
        let _ = unsafe { full_power_off(input, crate::shutdown::OuterState::from_abi(10)) };
    }
    1
}

