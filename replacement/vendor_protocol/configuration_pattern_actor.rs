//! Configuration action 19 composed with owned display CPU and scan publication.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x00dbb9c4.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey, ResultFlags};
use crate::configuration_display_pattern::PatternGeometry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternAction { Pending, Busy, Complete(ActionCompletion) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase { Idle, PanelStart, Frame, PanelFinish }

pub struct DisplayPatternActor { phase: Phase, cancelled: bool }

impl DisplayPatternActor {
    pub const fn new() -> Self { Self { phase: Phase::Idle, cancelled: false } }

    pub fn cancel(&mut self) -> Option<ActionCompletion> {
        if self.phase == Phase::Idle || self.cancelled { return None; }
        self.cancelled = true;
        Some(ActionCompletion::service_rejected())
    }
}

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_scanout_mode_completion(callback: u32) -> i32;
    fn xm1_scanout_mode_window(selector: u32, packet: *const u16) -> i32;
    fn xm1_scanout_mode_backing(value: u32) -> i32;
    fn xm1_scanout_mode_scale(packet: *const u8) -> i32;
    fn xm1_scanout_mode_enabled(value: u32) -> i32;
    fn xm1_scanout_mode_range(group: u32, item: u32, enabled: u32) -> i32;
    fn xm1_scanout_mode_colour(value: u32) -> i32;
    fn xm1_scanout_panel_command(command: u32, completion: u32) -> i32;
}

#[cfg(target_arch = "arm")]
fn publish_scan(flags: u32) {
    let _ = crate::result_publication::scan_send_event4(ResultFlags::from_abi(flags), &mut xm1_power_services::shutdown_rtos::NativeShutdownRtos);
}
#[cfg(target_arch = "arm")]
extern "C" fn frame_complete() { publish_scan(1); }
#[cfg(target_arch = "arm")]
extern "C" fn panel_started() { publish_scan(8); }
#[cfg(target_arch = "arm")]
extern "C" fn panel_finished() { publish_scan(0x20_0000); }

#[cfg(target_arch = "arm")]
impl DisplayPatternActor {
    /// # Safety
    /// The caller must hold exclusive ownership of the scanout buffer at 0x42e40000.
    pub unsafe fn begin(&mut self, images: &ConfigImages<'_>, keys: &[ConfigurationKey; 5], geometry: PatternGeometry) -> Result<PatternAction, BackendError> {
        if self.phase != Phase::Idle { return Ok(PatternAction::Busy); }
        self.cancelled = false;
        match keys[0].read(images)? {
            0 => {
                unsafe { xm1_scanout_panel_command(0, panel_started as *const () as u32); }
                self.phase = Phase::PanelStart;
                Ok(PatternAction::Pending)
            }
            1 => {
                let layout = geometry.layout();
                let buffer = unsafe { core::slice::from_raw_parts_mut(0x42e4_0000 as *mut u32, layout.stride as usize * layout.height as usize / 2) };
                if geometry.fill(buffer).is_err() { return Ok(PatternAction::Complete(ActionCompletion::service_rejected())); }
                let window = [0, 0, layout.width, layout.height];
                let mut scale = [0; 24];
                for (index, half) in [0, 0, layout.width, layout.height, layout.stride, layout.height, layout.width].iter().enumerate() {
                    scale[index * 2..index * 2 + 2].copy_from_slice(&half.to_le_bytes());
                }
                scale[20] = 0x80;
                unsafe {
                    xm1_scanout_mode_window(0, window.as_ptr());
                    xm1_scanout_mode_backing(0x42e4_0000);
                    xm1_scanout_mode_scale(scale.as_ptr());
                    xm1_scanout_mode_enabled(1);
                    xm1_scanout_mode_range(0, 0xff, 0);
                    xm1_scanout_mode_range(1, 0xff, 0);
                    xm1_scanout_mode_completion(frame_complete as *const () as u32);
                }
                self.phase = Phase::Frame;
                Ok(PatternAction::Pending)
            }
            2 => {
                let colour = crate::configuration_display_pattern::solid_pattern(keys[2].read(images)?, keys[3].read(images)?, keys[4].read(images)?);
                unsafe {
                    xm1_scanout_mode_enabled(0);
                    xm1_scanout_mode_range(0, 0xff, 0);
                    xm1_scanout_mode_range(1, 0xff, 0);
                    xm1_scanout_mode_colour(colour);
                    xm1_scanout_mode_completion(0);
                }
                Ok(PatternAction::Complete(ActionCompletion::service_success()))
            }
            _ => Ok(PatternAction::Complete(ActionCompletion::service_success())),
        }
    }

    pub fn consume_scan(&mut self, flags: ResultFlags) -> Option<PatternAction> {
        let expected = match self.phase { Phase::Idle => return None, Phase::PanelStart => 8, Phase::Frame => 1, Phase::PanelFinish => 0x20_0000 };
        let value = flags.get();
        if value & expected == 0 && value & 0x8000_0000 == 0 { return None; }
        if self.cancelled { self.phase = Phase::Idle; return None; }
        if value & 0x8000_0000 != 0 { self.phase = Phase::Idle; return Some(PatternAction::Complete(ActionCompletion::service_rejected())); }
        if self.phase == Phase::Frame {
            let result = unsafe { xm1_scanout_panel_command(2, panel_finished as *const () as u32) };
            if result != 0 { self.phase = Phase::Idle; return Some(PatternAction::Complete(ActionCompletion::service_rejected())); }
            self.phase = Phase::PanelFinish;
            Some(PatternAction::Pending)
        } else {
            self.phase = Phase::Idle;
            Some(PatternAction::Complete(ActionCompletion::service_success()))
        }
    }
}
