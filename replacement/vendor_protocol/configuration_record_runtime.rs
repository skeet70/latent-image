//! Configuration record actors for X-M1 1.50, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::ActionCompletion;
use xm1_power_lifecycle::record_address_space::{GuestAddress, GuestAddressSpace, MemoryError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigurationParameters {
    EnabledCallerTail(u32),
    DisabledAddressZero([u32; 7]),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationCallerFrame(GuestAddress);
impl ConfigurationCallerFrame {
    pub const fn new(frame_pointer: GuestAddress) -> Self { Self(frame_pointer) }
}

impl ConfigurationParameters {
    pub fn read(enabled: bool, memory: &mut impl GuestAddressSpace, caller: ConfigurationCallerFrame) -> Result<Self, MemoryError> {
        if enabled {
            let address = caller.0.get().checked_sub(12).ok_or(MemoryError::AddressOverflow)?;
            Ok(Self::EnabledCallerTail(memory.read_u32(GuestAddress::new(address))?))
        } else {
            let mut words = [0; 7];
            for (index, word) in words.iter_mut().enumerate() {
                *word = memory.read_u32(GuestAddress::new(index as u32 * 4))?;
            }
            Ok(Self::DisabledAddressZero(words))
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request { Configure(ConfigurationParameters), Restart }
impl Request {
    pub fn from_queue_action(action: u16, configuration: &ConfigImages<'_>, parameters: ConfigurationParameters) -> Result<Option<Self>, BackendError> {
        Ok(match action {
            96 => {
                let enabled = configuration.byte(0x81).ok_or(BackendError::Capacity)? != 0;
                if enabled != matches!(parameters, ConfigurationParameters::EnabledCallerTail(_)) { return Err(BackendError::InvalidCommand); }
                Some(Self::Configure(parameters))
            }
            103 => Some(Self::Restart),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command { Configure(ConfigurationParameters), Start, Stop, Suspend, Resume }
impl Command {
    pub const fn operation(self) -> u32 {
        match self { Self::Configure(_) => 0x17, Self::Start => 2, Self::Stop | Self::Suspend => 3, Self::Resume => 0x29 }
    }
    pub const fn length(self) -> usize {
        match self { Self::Configure(_) => 10, Self::Start => 7, Self::Stop | Self::Suspend => 6, Self::Resume => 2 }
    }
    pub const fn arguments(self) -> [u32; 9] {
        match self {
            Self::Configure(ConfigurationParameters::EnabledCallerTail(tail)) => [5, 5, 0, 0, 0, 0, 0, 0, tail],
            Self::Configure(ConfigurationParameters::DisabledAddressZero(words)) => [15, 0, words[0], words[1], words[2], words[3], words[4], words[5], words[6]],
            Self::Start => [2, 0, 0, 0, 0, 0, 0, 0, 0],
            Self::Stop => [0, 2, 0, 0, 0, 0, 0, 0, 0],
            Self::Suspend => [3, 2, 0, 0, 0, 0, 0, 0, 0],
            Self::Resume => [1, 0, 0, 0, 0, 0, 0, 0, 0],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompletionStatus(pub i32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error { Busy, NotWaiting }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Progress { Submit(Command), Waiting, Completed(ActionCompletion) }

pub struct RecordRuntime { pending: Option<Command> }
impl RecordRuntime {
    pub const fn new() -> Self { Self { pending: None } }
    pub fn begin(&mut self, request: Request) -> Result<Progress, Error> {
        if self.pending.is_some() { return Err(Error::Busy); }
        let command = match request {
            Request::Configure(parameters) => Command::Configure(parameters),
            Request::Restart => Command::Suspend,
        };
        self.pending = Some(command);
        Ok(Progress::Submit(command))
    }
    pub fn publication(&mut self, status: CompletionStatus) -> Result<Progress, Error> {
        let command = self.pending.ok_or(Error::NotWaiting)?;
        let next = match command {
            Command::Configure(ConfigurationParameters::EnabledCallerTail(_)) => Command::Start,
            Command::Configure(ConfigurationParameters::DisabledAddressZero(_)) => Command::Stop,
            Command::Suspend => Command::Resume,
            Command::Resume if status.0 != 0 => return Ok(Progress::Waiting),
            Command::Resume => Command::Start,
            Command::Start | Command::Stop => {
                self.pending = None;
                return Ok(Progress::Completed(ActionCompletion::service_success()));
            }
        };
        self.pending = Some(next);
        Ok(Progress::Submit(next))
    }
}
impl Default for RecordRuntime { fn default() -> Self { Self::new() } }

#[cfg(target_arch = "arm")]
mod native {
    use super::*;
    use core::sync::atomic::{AtomicBool, AtomicI32, Ordering};

    static CLAIMED: AtomicBool = AtomicBool::new(false);
    static READY: AtomicBool = AtomicBool::new(false);
    static STATUS: AtomicI32 = AtomicI32::new(0);
    unsafe extern "C" { fn xm1_power_record_helper_ef9a94(command: u32, payload: u32, length: u32); }

    unsafe extern "C" fn completed(status: i32) {
        STATUS.store(status, Ordering::Relaxed);
        READY.store(true, Ordering::Release);
    }

    unsafe fn submit(command: Command) {
        let mut packet = [0u32; 10];
        packet[0] = completed as *const () as u32;
        packet[1..].copy_from_slice(&command.arguments());
        unsafe { xm1_power_record_helper_ef9a94(command.operation(), packet.as_ptr() as u32, (command.length() * 4) as u32) };
    }

    impl RecordRuntime {
        /// # Safety
        /// The canonical Task43 receiver, all selected machine providers and mapped record state must be live. Poll this runtime until its final completion before releasing it.
        pub unsafe fn begin_native(&mut self, request: Request) -> Result<Progress, Error> {
            if self.pending.is_some() || CLAIMED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() { return Err(Error::Busy); }
            READY.store(false, Ordering::Relaxed);
            let progress = self.begin(request)?;
            if let Progress::Submit(command) = progress { unsafe { submit(command) }; }
            Ok(Progress::Waiting)
        }
        /// # Safety
        /// This runtime must own the active native record transaction.
        pub unsafe fn poll_native(&mut self) -> Result<Option<ActionCompletion>, Error> {
            if self.pending.is_none() { return Err(Error::NotWaiting); }
            if !READY.swap(false, Ordering::Acquire) { return Ok(None); }
            match self.publication(CompletionStatus(STATUS.load(Ordering::Relaxed)))? {
                Progress::Submit(command) => { unsafe { submit(command) }; Ok(None) }
                Progress::Waiting => Ok(None),
                Progress::Completed(completion) => {
                    CLAIMED.store(false, Ordering::Release);
                    Ok(Some(completion))
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "configuration_record_runtime_tests.rs"]
mod tests;
