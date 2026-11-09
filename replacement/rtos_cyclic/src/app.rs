//! App-side callback registration over the cyclic creation record.
//!
//! The stock five words map to `CyclicCreate`: mode = attributes, slot = argument, completion =
//! handler, argument = period, repeated_argument = phase.
//!
//! Retained seams: the interface method table and the app dispatcher completion; remove the
//! interface seam when the app caller and the cyclic service are linked in one owned image.

use crate::id::EncodedId;
use crate::layout::GuestAddress;
use crate::types::{
    CyclicArgument, CyclicAttributes, CyclicCreate, CyclicHandler, CyclicPhase, PeriodWord,
};

/// 1..=64.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[repr(transparent)]
pub struct CallbackSlot(u16);

impl CallbackSlot {
    pub const COUNT: u32 = 64;

    #[inline(always)]
    pub const fn new(slot: u32) -> Option<Self> {
        if slot >= 1 && slot <= Self::COUNT {
            Some(Self(slot as u16))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0 as u32
    }

    #[inline(always)]
    const fn from_slot(slot: u32) -> Self {
        Self(slot as u16)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CallbackToken(i32);

impl CallbackToken {
    pub const INVALID: Self = Self(-1);
    /// The first candidate of the stock token sequence.
    pub const FIRST: i32 = 30001;
    /// The last candidate of the stock token sequence.
    pub const LAST: i32 = 39999;

    #[inline(always)]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }

    #[inline(always)]
    pub const fn get(self) -> i32 {
        self.0
    }

    #[inline(always)]
    pub const fn next(self) -> Self {
        if self.0 >= Self::LAST {
            Self(Self::FIRST)
        } else {
            Self(self.0 + 1)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CallbackHandler(u32);

impl CallbackHandler {
    #[inline(always)]
    pub const fn new(address: u32) -> Self {
        Self(address)
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }

    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct TokenOutput(GuestAddress);

impl TokenOutput {
    #[inline(always)]
    pub const fn at(address: GuestAddress) -> Self {
        Self(address)
    }

    #[inline(always)]
    pub const fn address(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    pub const fn is_null(self) -> bool {
        self.0 .0 == 0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum CallbackMode {
    Event = 0,
}

impl CallbackMode {
    #[inline(always)]
    pub const fn from_word(word: i32) -> Option<Self> {
        if word == 0 {
            Some(Self::Event)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CallbackStatus(i32);

impl CallbackStatus {
    pub const OK: Self = Self(0);
    /// Unsupported mode, null callback or null token.
    pub const INVALID: Self = Self(-30);
    /// No free slot or no token candidate.
    pub const NO_SLOT: Self = Self(-34);

    #[inline(always)]
    pub const fn from_code(code: i32) -> Self {
        Self(code)
    }

    #[inline(always)]
    pub const fn code(self) -> i32 {
        self.0
    }

    #[inline(always)]
    pub const fn is_ok(self) -> bool {
        self.0 == 0
    }
}

/// The stock app callback table at `0x409c3500`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CallbackSlots(GuestAddress);

impl CallbackSlots {
    pub const NEXT_TOKEN: u32 = 0x000;
    pub const CALLBACKS: u32 = 0x004;
    pub const TOKENS: u32 = 0x104;
    pub const ARGUMENTS: u32 = 0x204;
    pub const BUSY: u32 = 0x304;
    pub const SIZE: u32 = 0x404;

    #[inline(always)]
    pub const fn at(base: GuestAddress) -> Self {
        Self(base)
    }

    #[inline(always)]
    pub const fn base(self) -> GuestAddress {
        self.0
    }

    #[inline(always)]
    const fn at_index(self, offset: u32, index: u32) -> GuestAddress {
        self.0.offset(offset + index * 4)
    }

    #[inline(always)]
    fn next_token<M: CallbackMachine>(self, m: &M) -> CallbackToken {
        CallbackToken(m.load_word(self.0.offset(Self::NEXT_TOKEN)) as i32)
    }

    #[inline(always)]
    fn set_next_token<M: CallbackMachine>(self, m: &M, token: CallbackToken) {
        m.store_word(self.0.offset(Self::NEXT_TOKEN), token.get() as u32);
    }

    #[inline(always)]
    fn set_callback<M: CallbackMachine>(self, m: &M, slot: CallbackSlot, callback: CallbackHandler) {
        m.store_word(self.at_index(Self::CALLBACKS, slot.get() - 1), callback.get());
    }

    #[inline(always)]
    fn token_index<M: CallbackMachine>(self, m: &M, index: u32) -> CallbackToken {
        CallbackToken(m.load_word(self.at_index(Self::TOKENS, index)) as i32)
    }

    #[inline(always)]
    fn set_token_index<M: CallbackMachine>(self, m: &M, index: u32, token: CallbackToken) {
        m.store_word(self.at_index(Self::TOKENS, index), token.get() as u32);
    }

    #[inline(always)]
    fn set_argument<M: CallbackMachine>(self, m: &M, slot: CallbackSlot, argument: PeriodWord) {
        m.store_word(self.at_index(Self::ARGUMENTS, slot.get() - 1), argument.0);
    }

    #[inline(always)]
    fn busy_index<M: CallbackMachine>(self, m: &M, index: u32) -> bool {
        m.load_word(self.at_index(Self::BUSY, index)) != 0
    }

    #[inline(always)]
    fn set_busy_index<M: CallbackMachine>(self, m: &M, index: u32, busy: bool) {
        m.store_word(self.at_index(Self::BUSY, index), busy as u32);
    }

    /// Slot 64 first, then 63 down to 1. `None` when all 64 are busy.
    #[inline(always)]
    fn find_free<M: CallbackMachine>(self, m: &M) -> Option<CallbackSlot> {
        let mut index = CallbackSlot::COUNT - 1;
        if self.busy_index(m, index) {
            loop {
                if index == 0 {
                    return None;
                }
                index -= 1;
                if !self.busy_index(m, index) {
                    break;
                }
            }
        }
        Some(CallbackSlot::from_slot(index + 1))
    }

    #[inline(always)]
    fn token_in_use<M: CallbackMachine>(self, m: &M, token: CallbackToken) -> bool {
        let mut index = 0;
        while index != CallbackSlot::COUNT {
            if self.token_index(m, index) == token {
                return true;
            }
            index += 1;
        }
        false
    }
}

/// The five-word creation record built for one reserved slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackRequest {
    pub slot: CallbackSlot,
    pub completion: CyclicHandler,
    pub period: PeriodWord,
    pub phase: CyclicPhase,
}

impl CallbackRequest {
    #[inline(always)]
    pub const fn cyclic_create(self) -> CyclicCreate {
        CyclicCreate {
            attributes: CyclicAttributes(0),
            argument: CyclicArgument(self.slot.get()),
            handler: self.completion,
            period: self.period,
            phase: self.phase,
        }
    }
}

/// The selected interface index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct InterfaceId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct CriticalToken(pub u32);

/// The interface operations the app caller needs.
pub trait CallbackMachine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    fn current_interface(&self) -> InterfaceId;
    fn create_callback(
        &self,
        interface: InterfaceId,
        id: EncodedId,
        request: CyclicCreate,
    ) -> CallbackStatus;
    fn start_callback(&self, interface: InterfaceId, id: EncodedId) -> CallbackStatus;
    fn stop_callback(&self, interface: InterfaceId, id: EncodedId);
    fn delete_callback(&self, interface: InterfaceId, id: EncodedId);
    fn enter_critical(&self, interface: InterfaceId) -> CriticalToken;
    fn leave_critical(&self, interface: InterfaceId, previous: CriticalToken);
    fn record_error(&self, operation: u32, mode: i32, status: CallbackStatus);
}

/// The stock app error-record operation.
pub const ERROR_OPERATION: u32 = 0x20;

/// The stock token-candidate attempts.
pub const TOKEN_ATTEMPTS: u32 = 9999;

/// The app dispatcher used as the cyclic handler for mode 0. Retained seam: remove when the
/// app dispatcher is owned.
pub const DISPATCH_EVENT: CyclicHandler = CyclicHandler(0x0072_337c);

#[cfg_attr(feature = "stage-app", inline(never))]
#[cfg_attr(feature = "stage-app", link_section = ".text.app_register_core")]
pub fn register_callback<M: CallbackMachine>(
    m: &M,
    slots: CallbackSlots,
    argument: PeriodWord,
    mode: i32,
    completion: CyclicHandler,
    callback: CallbackHandler,
    token: TokenOutput,
) -> CallbackStatus {
    let interface = m.current_interface();
    if token.is_null() {
        return report(m, mode, CallbackStatus::INVALID);
    }
    m.store_word(token.address(), CallbackToken::INVALID.get() as u32);
    if CallbackMode::from_word(mode).is_none() {
        return report(m, mode, CallbackStatus::INVALID);
    }
    if callback.is_null() {
        return report(m, mode, CallbackStatus::INVALID);
    }

    let previous = m.enter_critical(interface);
    let slot = match slots.find_free(m) {
        Some(slot) => slot,
        None => {
            m.leave_critical(interface, previous);
            return report(m, mode, CallbackStatus::NO_SLOT);
        }
    };
    let request = CallbackRequest {
        slot,
        completion,
        period: argument,
        phase: CyclicPhase(argument.0),
    };
    let id = EncodedId::of(1, slot.get() as u16);
    let status = m.create_callback(interface, id, request.cyclic_create());
    if !status.is_ok() {
        m.leave_critical(interface, previous);
        return report(m, mode, status);
    }
    slots.set_busy_index(m, slot.get() - 1, true);
    let allocated = match allocate_token(m, slots, slot, callback, argument) {
        Ok(token) => token,
        Err(status) => {
            slots.set_busy_index(m, slot.get() - 1, false);
            m.leave_critical(interface, previous);
            return report(m, mode, status);
        }
    };
    m.store_word(token.address(), allocated.get() as u32);
    m.leave_critical(interface, previous);
    let status = m.start_callback(interface, id);
    if status.is_ok() {
        status
    } else {
        report(m, mode, status)
    }
}

#[inline(always)]
fn allocate_token<M: CallbackMachine>(
    m: &M,
    slots: CallbackSlots,
    slot: CallbackSlot,
    callback: CallbackHandler,
    argument: PeriodWord,
) -> Result<CallbackToken, CallbackStatus> {
    let mut remaining = TOKEN_ATTEMPTS;
    loop {
        let candidate = slots.next_token(m);
        if !slots.token_in_use(m, candidate) {
            slots.set_callback(m, slot, callback);
            slots.set_token_index(m, slot.get() - 1, candidate);
            slots.set_argument(m, slot, argument);
            slots.set_next_token(m, candidate.next());
            return Ok(candidate);
        }
        slots.set_next_token(m, candidate.next());
        remaining -= 1;
        if remaining == 0 {
            return Err(CallbackStatus::NO_SLOT);
        }
    }
}

#[inline(never)]
#[cfg_attr(feature = "stage-app", link_section = ".text.app_register_core")]
fn report<M: CallbackMachine>(m: &M, mode: i32, status: CallbackStatus) -> CallbackStatus {
    m.record_error(ERROR_OPERATION, mode, status);
    status
}

#[cfg_attr(feature = "stage-app", inline(never))]
#[cfg_attr(feature = "stage-app", link_section = ".text.app_unregister_core")]
pub fn unregister_callback<M: CallbackMachine>(
    m: &M,
    slots: CallbackSlots,
    token: CallbackToken,
) -> CallbackStatus {
    let interface = m.current_interface();
    let mut index = CallbackSlot::COUNT - 1;
    if slots.token_index(m, index) != token {
        loop {
            if index == 0 {
                return CallbackStatus::OK;
            }
            index -= 1;
            if slots.token_index(m, index) == token {
                break;
            }
        }
    }
    let id = EncodedId::of(1, (index + 1) as u16);
    m.stop_callback(interface, id);
    m.delete_callback(interface, id);
    let previous = m.enter_critical(interface);
    slots.set_token_index(m, index, CallbackToken::new(0));
    slots.set_busy_index(m, index, false);
    m.leave_critical(interface, previous);
    CallbackStatus::OK
}
