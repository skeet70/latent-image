//! Actual source preparation at 0x0120b40c and mode preparation at 0x0120cab0.
//! The dispatch caller consumes the returned machine selector.
//!
//! Bound to x70view 55436fc85c0560dd252b0410af6f2427fe198cd3 and the
//! authenticated overlay47 image identified in `playback_consumer`.
//! Preparation does not dispatch machines 0 or 11 or publish completion.
//! Root/source/mode action execution and asynchronous cancellation remain open.

use core::ptr::{copy, read_volatile, write_volatile};
use super::playback_consumer::{PlaybackMessageCode, PlaybackModeCommand, PlaybackSourceKind};

const MESSAGE: usize = 0x014e_a6e4;
const MODE: usize = 0x014e_a678;
const CANCEL_FLAG: usize = 0x0151_14f1;

unsafe extern "C" {
    fn xm1_power_output_route_set_requested_level(slot: u32, level: u32, unused: u32) -> i32;
    fn xm1_scanout_mode_preset(selector: u32) -> i32;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum RootSelector {
    Selection100 = 0,
    Selection101 = 1,
    Selection102 = 2,
    Request103To119 = 3,
    Selection11e = 4,
    Request145Or146 = 5,
    Request128To141 = 6,
    Request11aOr11b = 7,
    Completion2a09 = 8,
    Completion560c = 9,
    Completion4f08 = 10,
    Other = 11,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ModeSelector {
    Request142 = 0,
    Request144Kind87 = 1,
    Request144Kind88 = 2,
    Request143 = 3,
    Request103To119 = 4,
    Request102 = 5,
    Completion2a09 = 6,
    Completion2a0a = 7,
    Other = 8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayloadTooShort {
    pub required: usize,
    pub actual: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMachine(u8);

impl PlaybackMachine {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value < 96 { Some(Self(value as u8)) } else { None }
    }

    pub const fn index(self) -> usize { self.0 as usize }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMachineEvent(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMachineCursor(pub u16);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackMessageOffset(usize);

impl PlaybackMessageOffset {
    pub const fn from_abi(offset: usize) -> Option<Self> {
        if offset & 3 == 0 && offset <= 0x25c { Some(Self(offset)) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackModeOffset(usize);

impl PlaybackModeOffset {
    pub const fn from_abi(offset: usize) -> Option<Self> {
        if offset & 3 == 0 && offset <= 0x54 { Some(Self(offset)) } else { None }
    }
}

pub struct NativePlaybackContext {
    _mapped: (),
}

impl NativePlaybackContext {
    /// # Safety
    /// Playback RAM, the output route state and its device registers must be mapped and initialized. The caller must serialize all access to the playback context. Source payloads must remain readable throughout each call.
    pub const unsafe fn new() -> Self {
        Self { _mapped: () }
    }

    pub fn message_code(&self) -> PlaybackMessageCode {
        PlaybackMessageCode::from_wire(unsafe { read_volatile(MESSAGE as *const u16) })
    }

    pub fn message_word(&self, offset: PlaybackMessageOffset) -> u32 {
        Self::word(MESSAGE + offset.0)
    }

    pub fn mode_word(&self, offset: PlaybackModeOffset) -> u32 {
        Self::word(MODE + offset.0)
    }

    pub fn set_mode_word(&mut self, offset: PlaybackModeOffset, value: u32) {
        Self::set_word(MODE + offset.0, value);
    }

    pub fn machine_event(&self, machine: PlaybackMachine) -> PlaybackMachineEvent {
        PlaybackMachineEvent(unsafe { read_volatile((0x014e_a4ec + machine.index() * 2) as *const u16) })
    }

    pub fn set_machine_event(&mut self, machine: PlaybackMachine, event: PlaybackMachineEvent) {
        unsafe { write_volatile((0x014e_a4ec + machine.index() * 2) as *mut u16, event.0) };
    }

    pub fn machine_cursor(&self, machine: PlaybackMachine) -> PlaybackMachineCursor {
        PlaybackMachineCursor(unsafe { read_volatile((0x014e_a5ac + machine.index() * 2) as *const u16) })
    }

    pub fn set_machine_cursor(&mut self, machine: PlaybackMachine, cursor: PlaybackMachineCursor) {
        unsafe { write_volatile((0x014e_a5ac + machine.index() * 2) as *mut u16, cursor.0) };
    }

    /// # Safety
    /// The incoming payload pointer must cover `length` readable bytes, and the destination must cover `length` writable bytes.
    pub unsafe fn copy_message_payload(&mut self, destination: PlaybackMessageOffset, length: usize) {
        let source = Self::word(MESSAGE + 8) as *const u8;
        unsafe { copy(source, (MESSAGE + destination.0) as *mut u8, length) };
    }

    fn latch_mode_preset(&mut self) {
        if Self::word(0x014e_a6d4) == 0 {
            unsafe { xm1_scanout_mode_preset(1) };
            Self::set_word(0x014e_a6d4, 1);
        }
    }

    fn word(address: usize) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }

    fn set_word(address: usize, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }

    fn copy_payload(payload: &[u8], offset: usize, length: usize) -> Result<(), PayloadTooShort> {
        let source = payload.get(..length).ok_or(PayloadTooShort { required: length, actual: payload.len() })?;
        unsafe { copy(source.as_ptr(), (MESSAGE + offset) as *mut u8, length) };
        Ok(())
    }

    fn request_output(&mut self) {
        unsafe { xm1_power_output_route_set_requested_level(2, 8, 0) };
    }

    fn primary_value(&mut self) {
        Self::set_word(MODE + 0x1c, Self::word(MESSAGE + 0xd4));
    }

    fn secondary_value(&mut self) {
        Self::set_word(MODE + 0x20, Self::word(MESSAGE + 0xd4));
    }

    fn apply_source_kind(&mut self) {
        let command = PlaybackModeCommand::from_wire(Self::word(MODE + 0x28));
        let source = PlaybackSourceKind::from_wire(Self::word(MESSAGE + 0xe8));
        Self::set_word(0x014e_a6d8, source.work_mode(command) as u32);
    }

    #[must_use]
    pub fn prepare_source(&mut self, code: PlaybackMessageCode, payload: &[u8]) -> Result<Option<RootSelector>, PayloadTooShort> {
        let code = code.wire();
        if code & 0x7f00 == 0x0100 {
            Self::copy_payload(payload, 0xd0, 0xc4)?;
            self.request_output();
            let event = match code {
                0x0100 => {
                    self.primary_value();
                    Self::set_word(MODE + 0x18, 0);
                    Self::set_word(MODE + 0x28, Self::word(MESSAGE + 0xec));
                    Self::set_word(MODE + 0x34, Self::word(MESSAGE + 0x12c));
                    self.apply_source_kind();
                    RootSelector::Selection100
                }
                0x0101 => {
                    self.secondary_value();
                    Self::set_word(MODE + 0x18, 0);
                    let cancel = u8::from(Self::word(MESSAGE + 0xe4) == 0x80);
                    unsafe { write_volatile(CANCEL_FLAG as *mut u8, cancel) };
                    RootSelector::Selection101
                }
                0x0102 => RootSelector::Selection102,
                0x0103..=0x0119 => {
                    self.primary_value();
                    RootSelector::Request103To119
                }
                0x011e => {
                    self.primary_value();
                    Self::set_word(MODE + 0x18, 8);
                    Self::set_word(MODE + 0x28, Self::word(MESSAGE + 0xec));
                    self.apply_source_kind();
                    RootSelector::Selection11e
                }
                0x0145..=0x0146 => {
                    self.primary_value();
                    RootSelector::Request145Or146
                }
                0x0128..=0x0141 => {
                    self.primary_value();
                    RootSelector::Request128To141
                }
                0x011a..=0x011b => {
                    self.primary_value();
                    RootSelector::Request11aOr11b
                }
                _ => return Ok(None),
            };
            Ok(Some(event))
        } else {
            let event = match code {
                0x2a09 => {
                    Self::copy_payload(payload, 0x194, 0x3c)?;
                    RootSelector::Completion2a09
                }
                0x560c => {
                    Self::copy_payload(payload, 0x1d0, 0x24)?;
                    RootSelector::Completion560c
                }
                0x4f08 => {
                    Self::copy_payload(payload, 0x254, 4)?;
                    RootSelector::Completion4f08
                }
                _ => RootSelector::Other,
            };
            Ok(Some(event))
        }
    }

    #[must_use]
    pub fn prepare_mode(&mut self, code: PlaybackMessageCode, payload: &[u8]) -> Result<Option<ModeSelector>, PayloadTooShort> {
        let code = code.wire();
        if (0x0142..=0x0144).contains(&code) {
            Self::copy_payload(payload, 0xd0, 0xc4)?;
            self.request_output();
            Self::set_word(MODE + 0x18, 0);
            let event = match code {
                0x0142 => {
                    self.primary_value();
                    ModeSelector::Request142
                }
                0x0143 => {
                    self.secondary_value();
                    ModeSelector::Request143
                }
                _ => match Self::word(MESSAGE + 0x34) {
                    0x87 => {
                        self.primary_value();
                        ModeSelector::Request144Kind87
                    }
                    0x88 => {
                        self.primary_value();
                        ModeSelector::Request144Kind88
                    }
                    _ => return Ok(None),
                },
            };
            Ok(Some(event))
        } else if code & 0x7f00 == 0x0100 {
            match code {
                0x0103..=0x0119 => {
                    Self::copy_payload(payload, 0xd0, 0xc4)?;
                    self.primary_value();
                    self.request_output();
                    self.latch_mode_preset();
                    Ok(Some(ModeSelector::Request103To119))
                }
                0x0102 => {
                    Self::copy_payload(payload, 0xd0, 0xc4)?;
                    self.request_output();
                    Ok(Some(ModeSelector::Request102))
                }
                _ => Ok(None),
            }
        } else {
            let event = match code {
                0x2a09 => {
                    Self::copy_payload(payload, 0x194, 0x3c)?;
                    ModeSelector::Completion2a09
                }
                0x2a0a => {
                    Self::copy_payload(payload, 0x194, 0x3c)?;
                    ModeSelector::Completion2a0a
                }
                _ => ModeSelector::Other,
            };
            Ok(Some(event))
        }
    }
}
