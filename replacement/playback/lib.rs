#![no_std]

mod jpeg_transaction;

#[path = "../vendor_protocol/playback_consumer.rs"]
mod playback_consumer;
#[path = "../vendor_protocol/playback_source_workspace.rs"]
mod playback_source_workspace;

use playback_consumer::PlaybackMessageCode;
use playback_source_workspace::{NativePlaybackContext, PlaybackMachine, PlaybackMachineEvent};

unsafe extern "C" {
    fn xm1_playback_retained_dispatch(machine: u32) -> i32;
}

const SOURCE_MACHINE: PlaybackMachine = match PlaybackMachine::from_abi(0) {
    Some(machine) => machine,
    None => unreachable!(),
};
const MODE_MACHINE: PlaybackMachine = match PlaybackMachine::from_abi(11) {
    Some(machine) => machine,
    None => unreachable!(),
};

#[derive(Clone, Copy)]
enum Preparation { Source, Mode }

fn payload_bytes(preparation: Preparation, code: PlaybackMessageCode) -> usize {
    let code = code.wire();
    match preparation {
        Preparation::Source if code & 0x7f00 == 0x0100 => 0xc4,
        Preparation::Source => match code { 0x2a09 => 0x3c, 0x560c => 0x24, 0x4f08 => 4, _ => 0 },
        Preparation::Mode => match code { 0x0102..=0x0119 | 0x0142..=0x0144 => 0xc4, 0x2a09..=0x2a0a => 0x3c, _ => 0 },
    }
}

unsafe fn prepare(preparation: Preparation) -> i32 {
    let mut context = unsafe { NativePlaybackContext::new() };
    let code = context.message_code();
    let length = payload_bytes(preparation, code);
    let payload = if length == 0 { &[] } else {
        let pointer = unsafe { core::ptr::read_volatile(0x014e_a6ec as *const u32) };
        unsafe { core::slice::from_raw_parts(pointer as *const u8, length) }
    };
    let (machine, selected) = match preparation {
        Preparation::Source => (SOURCE_MACHINE, context.prepare_source(code, payload).map(|event| event.map(|event| event as u16))),
        Preparation::Mode => (MODE_MACHINE, context.prepare_mode(code, payload).map(|event| event.map(|event| event as u16))),
    };
    match selected {
        Ok(Some(event)) => {
            context.set_machine_event(machine, PlaybackMachineEvent(event));
            unsafe { xm1_playback_retained_dispatch(machine.index() as u32) }
        }
        Ok(None) => 0,
        Err(_) => panic!("playback payload length invariant"),
    }
}

/// # Safety
/// Queue 4 must serialize entry. Playback globals must be initialized, and the received payload must cover its source-bound extent without overlapping the preparation destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_source_entry() -> i32 {
    unsafe { prepare(Preparation::Source) }
}

/// # Safety
/// Queue 4 must serialize entry. Playback globals must be initialized, and the received payload must cover its mode-bound extent without overlapping the preparation destination.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_mode_entry() -> i32 {
    unsafe { prepare(Preparation::Mode) }
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_information: &core::panic::PanicInfo<'_>) -> ! {
    loop { core::hint::spin_loop(); }
}
