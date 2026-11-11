//! CPU callback state from X-M1 1.50, pin 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! This module does not implement the display, image-service, or lens queue actors.

use crate::configuration_actions_low::{ActionCompletion, ActionNotification, CpuReturn, ResultFlags};

#[path = "configuration_actions_high_lens.rs"]
pub mod lens;

#[path = "configuration_actions_high_eeprom.rs"]
pub mod eeprom;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackStatus { Success, Failure }
impl CallbackStatus {
    const fn result(self) -> CpuReturn {
        match self { Self::Success => CpuReturn::SUCCESS, Self::Failure => CpuReturn::REJECTED }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackNotification { Transfer, Level }
impl CallbackNotification {
    pub const fn mask(self) -> u32 {
        match self { Self::Transfer => 0x800, Self::Level => 0x0400_0000 }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageDisposition { Continue, Finished, Failed }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageIdentifier(pub i16);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageBufferAddress(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayCompletionCode(pub i32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackState {
    pub result: CpuReturn,
    pub high_byte: u8,
    pub low_byte: u8,
    pub page: PageIdentifier,
    pub page_buffer: PageBufferAddress,
    pub write_page: PageIdentifier,
    pub display_result: DisplayCompletionCode,
}

impl CallbackState {
    /// X-M1 roots dc4dc0 and dc50a4.
    pub fn read_page(&mut self, status: CallbackStatus, page: PageIdentifier, buffer: PageBufferAddress) -> (CallbackNotification, PageDisposition) {
        self.result = status.result();
        let disposition = if status == CallbackStatus::Failure {
            PageDisposition::Failed
        } else {
            self.page = page;
            self.page_buffer = buffer;
            if page.0 == -1 { PageDisposition::Finished } else { PageDisposition::Continue }
        };
        (CallbackNotification::Transfer, disposition)
    }

    /// X-M1 root dc5030.
    pub fn wrote_page(&mut self, status: CallbackStatus, page: PageIdentifier) -> (CallbackNotification, PageDisposition) {
        self.write_page = page;
        self.result = status.result();
        let disposition = if status == CallbackStatus::Failure {
            PageDisposition::Failed
        } else if page.0 == -256 {
            PageDisposition::Finished
        } else {
            PageDisposition::Continue
        };
        (CallbackNotification::Transfer, disposition)
    }

    /// X-M1 roots dc4e44 and dc4fa4.
    pub fn transfer_complete(&mut self, status: CallbackStatus) -> CallbackNotification {
        self.result = status.result();
        CallbackNotification::Transfer
    }

    /// X-M1 roots dc4e94 and dc4fe0.
    pub fn transfer_word(&mut self, status: CallbackStatus, word: u16) -> CallbackNotification {
        if status == CallbackStatus::Failure { self.result = CpuReturn::REJECTED; }
        [self.high_byte, self.low_byte] = word.to_be_bytes();
        CallbackNotification::Transfer
    }

    /// X-M1 root dc4ee0.
    pub fn transfer_bits(&mut self, status: CallbackStatus, bits: &[u8; 16]) -> CallbackNotification {
        if status == CallbackStatus::Failure { self.result = CpuReturn::REJECTED; }
        self.low_byte = pack_bits(&bits[..8]);
        self.high_byte = pack_bits(&bits[8..]);
        CallbackNotification::Transfer
    }

    /// X-M1 root dc5128.
    pub fn transfer_result(&mut self, result: CpuReturn) -> CallbackNotification {
        self.result = result;
        CallbackNotification::Transfer
    }

    pub fn display_complete(&mut self, result: DisplayCompletionCode) -> CallbackNotification {
        self.display_result = result;
        CallbackNotification::Transfer
    }

    /// X-M1 roots dc6314 and dc63b0.
    pub fn level_word(&mut self, status: CallbackStatus, word: u16) -> CallbackNotification {
        self.result = status.result();
        [self.high_byte, self.low_byte] = word.to_be_bytes();
        CallbackNotification::Level
    }

    pub fn level_complete(&mut self, status: CallbackStatus) -> CallbackNotification {
        self.result = status.result();
        CallbackNotification::Level
    }
}

fn pack_bits(bits: &[u8]) -> u8 {
    bits.iter().enumerate().fold(0, |packed, (shift, byte)| packed | byte.wrapping_shl(shift as u32))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImmediateAction {
    Action120, Action127, Action128, Action129, Action130, Action131,
    Action133, Action134, Action135, Action136, Action137,
    Action138, Action139, Action140, Action141, Action142,
}
impl ImmediateAction {
    pub const fn from_wrapper(address: u32) -> Option<Self> {
        Some(match address {
            0x00eccf2c => Self::Action120,
            0x00eccfd4 => Self::Action127,
            0x00eccfec => Self::Action128,
            0x00ecd004 => Self::Action129,
            0x00ecd01c => Self::Action130,
            0x00ecd034 => Self::Action131,
            0x00ecd064 => Self::Action133,
            0x00ecd07c => Self::Action134,
            0x00ecd094 => Self::Action135,
            0x00ecd0ac => Self::Action136,
            0x00ecd0c4 => Self::Action137,
            0x00ecd0dc => Self::Action138,
            0x00ecd0f4 => Self::Action139,
            0x00ecd10c => Self::Action140,
            0x00ecd124 => Self::Action141,
            0x00ecd13c => Self::Action142,
            _ => return None,
        })
    }

    /// The caller must execute dba464's route update and event-4 delivery.
    pub const fn completion(self) -> ActionCompletion {
        let flags = match self {
            Self::Action133 | Self::Action134 | Self::Action135 | Self::Action136 | Self::Action137 => ResultFlags::FAILURE,
            _ => ResultFlags::SUCCESS,
        };
        ActionCompletion { return_code: CpuReturn::SUCCESS, notification: ActionNotification::ServiceResult(flags) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> CallbackState {
        CallbackState { result: CpuReturn::REJECTED, high_byte: 0xa1, low_byte: 0xb2, page: PageIdentifier(7), page_buffer: PageBufferAddress(0x1234), write_page: PageIdentifier(8), display_result: DisplayCompletionCode(-10) }
    }

    #[test]
    fn failed_read_preserves_last_page_and_success_publishes_terminal_page() {
        let mut state = state();
        assert_eq!(state.read_page(CallbackStatus::Failure, PageIdentifier(-1), PageBufferAddress(0x5678)), (CallbackNotification::Transfer, PageDisposition::Failed));
        assert_eq!(state.page, PageIdentifier(7));
        assert_eq!(state.page_buffer, PageBufferAddress(0x1234));
        assert_eq!(state.read_page(CallbackStatus::Success, PageIdentifier(-1), PageBufferAddress(0x5678)).1, PageDisposition::Finished);
        assert_eq!(state.page, PageIdentifier(-1));
        assert_eq!(state.page_buffer, PageBufferAddress(0x5678));
        assert_eq!(state.result.get(), 0);
    }

    #[test]
    fn write_termination_differs_from_read_termination() {
        let mut state = state();
        assert_eq!(state.wrote_page(CallbackStatus::Success, PageIdentifier(-1)).1, PageDisposition::Continue);
        assert_eq!(state.wrote_page(CallbackStatus::Success, PageIdentifier(-256)).1, PageDisposition::Finished);
        assert_eq!(state.wrote_page(CallbackStatus::Failure, PageIdentifier(3)).1, PageDisposition::Failed);
        assert_eq!(state.write_page, PageIdentifier(3));
        assert_eq!(state.result.get(), -255);
    }

    #[test]
    fn transfer_word_preserves_error_but_level_word_clears_it() {
        let mut state = state();
        assert_eq!(state.transfer_word(CallbackStatus::Success, 0x1234).mask(), 0x800);
        assert_eq!((state.result.get(), state.high_byte, state.low_byte), (-255, 0x12, 0x34));
        assert_eq!(state.level_word(CallbackStatus::Success, 0x5678).mask(), 0x0400_0000);
        assert_eq!((state.result.get(), state.high_byte, state.low_byte), (0, 0x56, 0x78));
        state.level_word(CallbackStatus::Failure, 0x9abc);
        assert_eq!((state.result.get(), state.high_byte, state.low_byte), (-255, 0x9a, 0xbc));
    }

    #[test]
    fn bit_callback_packs_without_boolean_coercion() {
        let mut state = state();
        state.transfer_bits(CallbackStatus::Success, &[2, 1, 0, 0, 0, 0, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]);
        assert_eq!((state.high_byte, state.low_byte, state.result.get()), (0xaa, 0x82, -255));
    }

    #[test]
    fn callbacks_publish_raw_results_and_success_overwrites_error() {
        let mut state = state();
        state.transfer_result(CpuReturn::from_abi(-17));
        assert_eq!(state.result.get(), -17);
        state.display_complete(DisplayCompletionCode(-10));
        assert_eq!(state.display_result, DisplayCompletionCode(-10));
        state.transfer_complete(CallbackStatus::Success);
        assert_eq!(state.result.get(), 0);
        state.level_complete(CallbackStatus::Failure);
        assert_eq!(state.result.get(), -255);
    }

    #[test]
    fn immediate_error_signals_still_return_success_to_wrapper() {
        for wrapper in [0x00ecd064, 0x00ecd07c, 0x00ecd094, 0x00ecd0ac, 0x00ecd0c4] {
            let action = ImmediateAction::from_wrapper(wrapper).unwrap();
            assert_eq!(action.completion(), ActionCompletion { return_code: CpuReturn::SUCCESS, notification: ActionNotification::ServiceResult(ResultFlags::FAILURE) });
        }
        for wrapper in [0x00eccf2c, 0x00eccfd4, 0x00eccfec, 0x00ecd004, 0x00ecd01c, 0x00ecd034, 0x00ecd0dc, 0x00ecd0f4, 0x00ecd10c, 0x00ecd124, 0x00ecd13c] {
            let action = ImmediateAction::from_wrapper(wrapper).unwrap();
            assert_eq!(action.completion(), ActionCompletion::service_success());
        }
        assert_eq!(ImmediateAction::from_wrapper(0x00ecd04c), None);
        assert_eq!(ImmediateAction::from_wrapper(0x00ecd154), None);
    }
}
