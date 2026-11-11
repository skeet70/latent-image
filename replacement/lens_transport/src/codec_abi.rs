//! Native qualification adapter. Fixed addresses belong only to the synthetic stock ABI.
use crate::frame::*;

unsafe extern "C" { fn lens_publication_edge(event: u32); }

unsafe fn load(offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((0x0095_d3c0 + offset) as *const u32) }
}
unsafe fn machine(offset: usize) -> u32 {
    unsafe { core::ptr::read_volatile((0x0095_5dd0 + offset) as *const u32) }
}
unsafe fn store(offset: usize, value: u32) {
    unsafe { core::ptr::write_volatile((0x0095_d3c0 + offset) as *mut u32, value) }
}
fn recovery(raw: u32) -> RecoveryMode {
    match raw { 1 => RecoveryMode::Awaiting, 2 => RecoveryMode::Restart, _ => RecoveryMode::Normal }
}
fn recovery_word(mode: RecoveryMode) -> u32 {
    match mode { RecoveryMode::Normal => 0, RecoveryMode::Awaiting => 1, RecoveryMode::Restart => 2 }
}
unsafe fn publish_frame(frame: &[u8]) {
    unsafe { core::ptr::copy_nonoverlapping(frame.as_ptr(), 0x4096_d000 as *mut u8, frame.len()) }
}

/// Arguments follow the retained 0x006bf50c interface.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lens_codec_read(packed: u32, checksum: u32, marker: u32, callback: u32) {
    let Ok(length) = PayloadLength::new((packed >> 16) as usize) else { return };
    let mut frame = OwnedFrame::<MAX_FRAME_LENGTH>::new();
    if frame.build_read(PageId::new(packed as u16), length, if marker == 0 { ReadMarker::Disabled } else { ReadMarker::Enabled }).is_err() { return }
    unsafe {
        publish_frame(frame.as_bytes());
        store(0x68, packed); store(0x6c, checksum); store(0x70, marker); store(0x74, callback); store(0x38, 2);
        lens_publication_edge(3);
    }
}

/// The frame guard excludes the two stock bytes never initialized by writeframe.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lens_codec_write(page: u32, payload: *const u8, length: u32, checksum: u32, callback: u32) {
    let Ok(length) = PayloadLength::new((length as u16) as usize) else { return };
    let payload = unsafe { core::slice::from_raw_parts(payload, length.get()) };
    let mut frame = OwnedFrame::<MAX_FRAME_LENGTH>::new();
    if frame.build_write(PageId::new(page as u16), payload, if checksum == 0 { ChecksumMode::Disabled } else { ChecksumMode::Enabled }).is_err() { return }
    unsafe {
        let initialized = length.get() + 2 + usize::from(checksum != 0);
        publish_frame(&frame.as_bytes()[..initialized]);
        store(0x54, page); store(0x58, payload.as_ptr() as u32); store(0x5c, length.get() as u32); store(0x60, checksum); store(0x64, callback); store(0x38, 1);
        lens_publication_edge(3);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lens_codec_command(raw: u32) {
    unsafe {
        let sequence = Sequence::new(load(0x8c) as u8).unwrap_or_else(|_| unreachable!());
        let command = QueuedCommand::new(CommandWord::new(raw), sequence);
        store(0x8c, sequence.next().get() as u32);
        let queue = 0x0095_b254 as *mut u32;
        let count = core::ptr::read_volatile(queue.add(0xa8 / 4));
        if count < 18 {
            let index = core::ptr::read_volatile(queue.add(0xa0 / 4));
            core::ptr::write_volatile(queue.add(index as usize * 2), command.command.raw());
            core::ptr::write_volatile(queue.add(index as usize * 2 + 1), command.acknowledgement.raw());
            core::ptr::write_volatile(queue.add(0xa0 / 4), if index < 19 { index + 1 } else { 0 });
            core::ptr::write_volatile(queue.add(0xa8 / 4), count + 1);
        }
        lens_publication_edge(3);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lens_codec_ack() -> u32 {
    unsafe {
        let mode = recovery(load(0x9c));
        let bytes = core::slice::from_raw_parts(0x4096_d820 as *const u8, 4);
        let context = ResponseContext {
            current_command: CommandWord::new(load(0x44)), previous_command: CommandWord::new(load(0x48)),
            last_response: Some(CommandWord::new(load(0x4c))), expected_sequence: Sequence::new(load(0x90) as u8).unwrap_or_else(|_| unreachable!()),
            recovery: mode, busy_count: RetryCount::new(load(0x98)), busy_limit: RetryLimit::new(load(0x1c)),
        };
        let result = classify_ack(bytes, context).unwrap_or_else(|_| unreachable!());
        store(0x9c, recovery_word(result.recovery)); store(0x98, result.busy_count.get());
        if result.acknowledges_previous {
            let callback = load(0x50);
            if callback != 0 {
                let callback: unsafe extern "C" fn(u32, u32) -> u32 = core::mem::transmute(callback as usize);
                if callback(load(0x4c), 0) != 0 { store(0x78, 1); }
            }
            store(0x90, result.expected_sequence.get() as u32);
            if result.reset_retry_count { store(0x94, 0); }
            store(0x9c, u32::from(load(0x78) != 1));
        }
        result.outcome.stock_code(mode)
    }
}

/// Stock accepts the invalid magic only while the machine words at 0x00955dd4 and 0x00955dd8 read
/// 6 and 12; see 0x006be418-0x006be438.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lens_codec_response() -> u32 {
    unsafe {
        let mode = recovery(load(0x9c));
        let bytes = core::slice::from_raw_parts(0x4096_d820 as *const u8, 4);
        // The suite's deliberately wrong variant: the invalid-magic gate without the machine
        // words at 0x00955dd4/0x00955dd8. Only the test-only `codec-defect-gate` ELF compiles this.
        #[cfg(feature = "defect-reply-gate")]
        let allow_invalid_magic = false;
        #[cfg(not(feature = "defect-reply-gate"))]
        let allow_invalid_magic = machine(4) == 6 && machine(8) == 12;
        let result = classify_command(bytes, CommandWord::new(load(0x48)), CommandWord::new(load(0x40)), mode, allow_invalid_magic).unwrap_or_else(|_| unreachable!());
        store(0x4c, result.response.raw()); store(0x40, result.acknowledgement.raw()); store(0x9c, recovery_word(result.recovery));
        result.outcome.stock_code(mode)
    }
}
