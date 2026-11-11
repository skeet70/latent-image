#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AudioAction {
    Noop, Notify13, Notify0e, Notify0c, Notify08, Notify07, Notify15, Notify0a, Notify09, Notify05,
    PanelHold, PanelRelease, ClockInput, ClockOutput, ClockFixed, Disable, Output0309, Output030a,
    Enable13, Input0303, Enable1b, SelectChannels, Gain, Format8, SelectFormat, Pattern5e,
    Settings0345, Bank0332, Output032f, Input032d, Register0332c, Gain032d, Pattern5b,
    ClearLatch, ClearEnable2, Pattern1b, Pattern1e, Enable2, SetLatch, ClearEnable40,
    Settings031f, Bank030c, Input0307, Register0306, Gain0307, Enable40, SelectMode,
    ClearEnable20, Enable0c, Bank0321, Output030b, Input0302, Power20, Mode26,
    Mask10, Register0b, Register0a, Register09, Register08, Enable20, Enable0b, ClearRegister07,
    TransferReady, DelaySignal, BankComplete, BankNotify, BankNotifyUnchanged,
}
impl AudioAction {
    pub const fn from_abi(address: u32) -> Option<Self> {
        Some(match address {
            0x6e6de4 | 0x6e6df8 => Self::Noop,
            0x6e6e0c => Self::Notify13, 0x6e6e58 => Self::Notify0e, 0x6e6ea4 => Self::Notify0c,
            0x6e6ef0 => Self::Notify08, 0x6e6f3c => Self::Notify07, 0x6e6f88 => Self::Notify15,
            0x6e6fd4 => Self::Notify0a, 0x6e7020 => Self::Notify09, 0x6e70b8 => Self::Notify05,
            0x6e7104 => Self::PanelHold, 0x6e7114 => Self::PanelRelease,
            0x6e7124 => Self::ClockInput, 0x6e745c => Self::ClockOutput, 0x6e75fc => Self::ClockFixed,
            0x6e78e4 => Self::Disable, 0x6e7920 => Self::Output0309, 0x6e7954 => Self::Output030a,
            0x6e7988 => Self::Enable13, 0x6e79ac => Self::Input0303, 0x6e79e0 => Self::Enable1b,
            0x6e7a04 => Self::SelectChannels, 0x6e7a48 => Self::Gain, 0x6e7bb0 => Self::Format8,
            0x6e7c44 | 0x6e7c9c => Self::SelectFormat, 0x6e7cf4 => Self::Pattern5e,
            0x6e7d1c => Self::Settings0345, 0x6e7d78 => Self::Bank0332,
            0x6e8024 => Self::Output032f, 0x6e8058 => Self::Input032d, 0x6e808c => Self::Register0332c,
            0x6e80c0 => Self::Gain032d, 0x6e8110 => Self::Pattern5b, 0x6e8160 => Self::ClearLatch,
            0x6e81a8 => Self::ClearEnable2, 0x6e81d0 => Self::Pattern1b, 0x6e81f8 => Self::Pattern1e,
            0x6e8220 => Self::Enable2, 0x6e8268 => Self::SetLatch, 0x6e82a0 => Self::ClearEnable40,
            0x6e82c8 => Self::Settings031f, 0x6e8320 => Self::Bank030c, 0x6e85cc => Self::Output0309,
            0x6e8600 => Self::Input0307, 0x6e8634 => Self::Register0306, 0x6e8668 => Self::Gain0307,
            0x6e86b8 => Self::Enable40, 0x6e8704 => Self::SelectMode, 0x6e87f0 => Self::ClearEnable20,
            0x6e8818 => Self::Enable0c, 0x6e883c => Self::Bank0321, 0x6e8994 => Self::Output030b,
            0x6e89cc => Self::Input0302, 0x6e8a04 => Self::Power20, 0x6e8a3c => Self::Mode26,
            0x6e8a68 => Self::Mask10, 0x6e8a90 => Self::Register0b, 0x6e8acc => Self::Register0a,
            0x6e8b08 => Self::Register09, 0x6e8b40 => Self::Register08, 0x6e8b7c => Self::Enable20,
            0x6e8ba0 => Self::Enable0b, 0x6e8bc4 => Self::ClearRegister07,
            0x6e50ac => Self::TransferReady, 0x6ed81c => Self::DelaySignal,
            0x6e97c0 => Self::BankComplete,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CodecRegister(pub u8);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ConfigurationKey(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CacheKeep(pub u16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CacheBits(pub u16);
pub const fn edit_value(previous: u16, keep: CacheKeep, bits: CacheBits) -> u16 { previous & keep.0 | bits.0 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BankNotificationContext(pub u32);
pub trait BankCompletion {
    fn notification_context(&self) -> BankNotificationContext;
    fn publish(&mut self, context: BankNotificationContext);
    fn saved_phase(&self) -> super::AudioPhase;
    fn restore_phase(&mut self, phase: super::AudioPhase);
}
pub fn complete_bank(completion: &mut impl BankCompletion) {
    completion.publish(completion.notification_context());
    completion.restore_phase(completion.saved_phase());
}

#[path = "audio_callback_cpu.rs"]
mod cpu;
pub use cpu::{invoke, panel_control, BankPublication, CallbackAddress, ClockPort, PanelOperation, PhysicalIo, ResourceHandle};

#[cfg(target_arch = "arm")]
#[path = "audio_callback_native.rs"]
pub mod native;

#[cfg(test)]
#[path = "audio_callback_cpu_tests.rs"]
mod cpu_tests;
