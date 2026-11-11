#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Address(pub u32);

pub const RECORDING: Address = Address(0x0152_6a78);
pub const RESULT: Address = Address(0x0154_58f0);
pub const SELECTION: Address = Address(0x0154_5830);
pub const REQUEST: Address = Address(RECORDING.0 + 0xc248);
pub const OUTPUT: Address = Address(RECORDING.0 + 0xc250);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EngineOperation { Resume, Suspend }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchError { InvalidState(u16) }

pub trait Runtime {
    fn read16(&self, address: Address) -> u16;
    fn read32(&self, address: Address) -> u32;
    fn write16(&mut self, address: Address, value: u16);
    fn write32(&mut self, address: Address, value: u32);
    fn submit_engine(&mut self, operation: EngineOperation);
    fn defer_current(&mut self);
}

#[derive(Clone, Copy)]
enum Callback { Complete, Pending, Resume, Defer, Suspend }
#[derive(Clone, Copy)]
struct Cell { callback: Callback, next: Option<u16> }

const fn cell(callback: Callback, next: Option<u16>) -> Cell { Cell { callback, next } }
use Callback::{Complete, Pending, Resume, Defer, Suspend};

// X-M1 image5 0x00e3ec90, six selector rows with four states each.
const TRANSITIONS: [[Cell; 4]; 6] = [
    [cell(Complete, None), cell(Pending, None), cell(Complete, None), cell(Pending, None)],
    [cell(Resume, Some(1)), cell(Defer, None), cell(Complete, None), cell(Defer, None)],
    [cell(Complete, None), cell(Defer, None), cell(Suspend, Some(3)), cell(Defer, None)],
    [cell(Defer, None), cell(Complete, Some(2)), cell(Defer, None), cell(Defer, None)],
    [cell(Defer, None), cell(Defer, None), cell(Defer, None), cell(Complete, Some(0))],
    [cell(Complete, None), cell(Pending, None), cell(Complete, None), cell(Pending, None)],
];

pub fn dispatch(runtime: &mut impl Runtime) -> Result<i32, DispatchError> {
    let selection = match runtime.read32(REQUEST) {
        0 => 0,
        1 => 1,
        2 => 2,
        3 => match runtime.read16(RECORDING) { 0x13e5 => 3, 0x13e6 => 4, _ => 5 },
        _ => return Ok(0),
    };
    runtime.write16(SELECTION, selection as u16);
    let state = runtime.read16(RESULT);
    let transition = *TRANSITIONS[selection].get(usize::from(state)).ok_or(DispatchError::InvalidState(state))?;
    let result = match transition.callback {
        Complete => 1,
        Pending => 0,
        Resume => { runtime.submit_engine(EngineOperation::Resume); 0 },
        Suspend => { runtime.submit_engine(EngineOperation::Suspend); 0 },
        Defer => { runtime.defer_current(); 0 },
    };
    if let Some(next) = transition.next { runtime.write16(RESULT, next); }
    Ok(result)
}

pub fn request(runtime: &mut impl Runtime, words: [u32; 2]) -> Result<i32, DispatchError> {
    runtime.write32(REQUEST, words[0]);
    runtime.write32(Address(REQUEST.0 + 4), words[1]);
    dispatch(runtime)
}

#[cfg(target_arch = "arm")]
#[path = "recording_engine80_native.rs"]
mod native;

#[cfg(test)]
#[path = "recording_engine80_tests.rs"]
mod tests;
