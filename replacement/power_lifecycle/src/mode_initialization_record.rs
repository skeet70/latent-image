#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct CommandToken(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct SettingRecord {
    pub mode: CommandToken,
    pub selector: CommandToken,
    pub flip: CommandToken,
    pub source: CommandToken,
    pub output: CommandToken,
    pub recording: CommandToken,
    pub enabled: CommandToken,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitialSettingInputs {
    pub capture_mode: bool,
    pub selector: u32,
    pub capture_enabled: bool,
    pub category_22c0: bool,
    pub alternate_output: bool,
    pub category_2080: bool,
    pub recording_disabled: bool,
    pub recording_selector: u32,
    pub playback_recording: CommandToken,
}

pub fn initial_setting_record(input: InitialSettingInputs) -> SettingRecord {
    SettingRecord {
        mode: CommandToken(if input.capture_mode && input.capture_enabled { 128 } else { 129 }),
        selector: CommandToken(match input.selector { 1 => 276, 2 => 275, 3 => 274, 4 => 278, _ => 277 }),
        flip: CommandToken(284),
        source: CommandToken(285),
        output: CommandToken(if !input.capture_mode { 21 } else if input.category_22c0 { 259 } else if input.alternate_output { 260 } else if input.category_2080 { 261 } else { 21 }),
        recording: CommandToken(if input.capture_mode {
            if input.recording_disabled { 21 } else { match input.recording_selector { 2 => 204, 1 => 203, _ => 21 } }
        } else if input.playback_recording.0 == 203 { 203 } else { 21 }),
        enabled: CommandToken(129),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(C)]
pub struct ControllerConfiguration {
    pub mode: u32,
    pub selector: u32,
    pub flip: u32,
    pub output: u32,
    pub recording: u32,
    pub enabled: u32,
    pub source: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeResult { Disabled, Configuration(ControllerConfiguration) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidSettingRecord;

pub fn decode_record(record: SettingRecord) -> Result<DecodeResult, InvalidSettingRecord> {
    let mode = match record.mode.0 { 128 => 1, 173 => 2, 131 => 3, 207 => 4, value @ 302..=310 => value - 297, _ => 0 };
    let selector = match record.selector.0 { 274 => return Ok(DecodeResult::Disabled), 276 => 1, 275 => 2, 278 => 3, 277 => 0, _ => return Err(InvalidSettingRecord) };
    let flip = match record.flip.0 { 283 => 1, 284 => 0, _ => return Err(InvalidSettingRecord) };
    let source = match record.source.0 { 286 => 1, 287 => 2, 285 => 0, _ => return Err(InvalidSettingRecord) };
    let output = match record.output.0 { 259 => 0, 260 => 1, 261 => 2, 21 => 3, _ => return Err(InvalidSettingRecord) };
    let recording = match record.recording.0 { 204 => 0, 203 => 1, 21 => 2, _ => return Err(InvalidSettingRecord) };
    let enabled = match record.enabled.0 { 128 => 1, 129 => 0, _ => return Err(InvalidSettingRecord) };
    Ok(DecodeResult::Configuration(ControllerConfiguration { mode, selector, flip, output, recording, enabled, source }))
}

pub trait InitialRecordSink {
    fn configure(&mut self, configuration: ControllerConfiguration) -> i32;
    fn record_source(&mut self, source: u32) -> i32;
}

pub fn publish_record(sink: &mut impl InitialRecordSink, record: SettingRecord) -> i32 {
    match decode_record(record) {
        Err(_) => -63,
        Ok(DecodeResult::Disabled) => 0,
        Ok(DecodeResult::Configuration(configuration)) => {
            let configured = sink.configure(configuration);
            let source = sink.record_source(configuration.source);
            if configured | source == 0 { 0 } else { -63 }
        }
    }
}

mod native {
    use super::*;
    use core::ptr::{read_volatile, write_volatile};
    unsafe extern "C" {
        fn xm1_scanout_panel_device_configuration(kind: u32, mode: u32, selector: u32, flip: u32, output: u32, recording: u32, enabled: u32) -> i32;
        fn xm1_scanout_panel_initial_live_configuration(live: u32, mode: u32, selector: u32, flip: u32) -> i32;
        fn xm1_scanout_panel_record_source(source: u32);
        fn xm1_power_frontend_failure();
    }
    fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
    fn category_2000(token: u32) -> bool {
        let bit = ((token >> 6) & 63) as usize;
        word(0x94f890 + bit / 32 * 4) & (1 << (bit % 32)) != 0
    }
    struct NativeSink;
    impl InitialRecordSink for NativeSink {
        fn configure(&mut self, c: ControllerConfiguration) -> i32 {
            let status = if c.mode > 13 || c.mode == 2 || c.selector >= 4 || c.flip >= 2 || c.output >= 4 || c.recording >= 3 || c.enabled > 1 {
                -30
            } else {
                unsafe { xm1_scanout_panel_device_configuration(word(0x96bd74), c.mode, c.selector, c.flip, c.output, c.recording, c.enabled) }
            };
            let normalized = if c.mode == 2 || (5..=13).contains(&c.mode) { 1 } else { c.mode };
            let live = u32::from(word(0x955b20) == 0 && word(0x955b08) == 1);
            let live_status = unsafe { xm1_scanout_panel_initial_live_configuration(live, normalized, c.selector, c.flip) };
            status | live_status
        }
        fn record_source(&mut self, source: u32) -> i32 {
            unsafe { xm1_scanout_panel_record_source(source) };
            0
        }
    }
    /// # Safety
    /// The UI frame, input state and owned panel controller must be initialized and mapped.
    pub unsafe fn write_initial_setting_record() -> i32 {
        let capture_mode = word(0x9500ec) == 3;
        let record = initial_setting_record(InitialSettingInputs {
            capture_mode,
            selector: word(0x95039c),
            capture_enabled: unsafe { read_volatile(0x9503b4 as *const u8) } == 1,
            category_22c0: capture_mode && category_2000(0x22c0),
            alternate_output: capture_mode && word(0x951560) == 1,
            category_2080: capture_mode && category_2000(0x2080),
            recording_disabled: capture_mode && word(0x956938) == 1,
            recording_selector: word(0x956934),
            playback_recording: CommandToken(word(0x950470)),
        });
        let status = publish_record(&mut NativeSink, record);
        unsafe { write_volatile(0x954d68 as *mut i32, status) };
        if status != 0 { unsafe { xm1_power_frontend_failure() }; }
        status
    }
    #[no_mangle]
    pub unsafe extern "C" fn xm1_power_write_initial_setting_record() -> i32 { unsafe { write_initial_setting_record() } }
}
pub use native::write_initial_setting_record;

#[cfg(test)]
#[path = "mode_initialization_record_tests.rs"]
mod tests;
