//! Configuration actions 18 and 20 composed with owned scanout CPU exports.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, 0x00dbb74c and 0x00dbbfcc.

use crate::backend::{BackendError, ConfigImages};
use crate::configuration_actions_low::{ActionCompletion, ConfigurationKey};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayIdentity { category: u8, timing: u8, active: bool }

impl DisplayIdentity {
    pub const fn from_configuration(category: u8, timing: u8, active: u8) -> Self { Self { category, timing, active: active != 0 } }

    pub fn configuration(self) -> DisplayConfiguration {
        let route = match self.category { 0xca => 1, 0xcb => 2, _ => 3 };
        let mode = if matches!(self.category, 0xca | 0xcb | 0xcd) { u32::from(self.active) }
            else { match self.timing { 0xd2 => 3, 0xd3 => 4, 0xd4 => 2, 0xd6 => 6, 0xd7 => 7, 0xd8 => 8, 0xd9 => 9, _ => 5 } };
        DisplayConfiguration { engine: u32::from(self.category == 0xca), route, mode, alternate: if matches!(self.category, 0xca | 0xcb | 0xcd) { mode } else { 0 } }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisplayConfiguration { engine: u32, route: u32, mode: u32, alternate: u32 }

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_scanout_engine_initialize(engine: u32, route: u32, mode: u32);
    fn xm1_scanout_timing_configuration(engine: u32, configuration: *mut u32) -> i32;
    fn xm1_scanout_bootstrap(configuration: *const u32) -> i32;
    fn xm1_scanout_formatter_slot(engine: u32, configuration: *const u32) -> i32;
    fn xm1_scanout_wait(engine: u32) -> i32;
    fn xm1_scanout_formatter_strobe(engine: u32) -> i32;
    fn xm1_scanout_completion_event(engine: u32, event: u32, callback: u32) -> i32;
    fn xm1_scanout_panel_program_frontend();
    fn xm1_scanout_panel_transfer_frontend(register: u32, value: u32) -> i32;
}

#[cfg(target_arch = "arm")]
pub fn action18(images: &ConfigImages<'_>, key: ConfigurationKey, identity: DisplayIdentity) -> Result<ActionCompletion, BackendError> {
    if key.read(images)? == 0 {
        let config = identity.configuration();
        let mut timing = [config.mode, config.route, 1];
        let bootstrap = [config.mode, config.route, 0];
        let formatter = [3, config.alternate, config.route, 0];
        unsafe {
            xm1_scanout_engine_initialize(config.engine, config.route, config.mode);
            xm1_scanout_timing_configuration(config.engine, timing.as_mut_ptr());
            if config.engine == 0 { xm1_scanout_bootstrap(bootstrap.as_ptr()); }
            xm1_scanout_formatter_slot(config.engine, formatter.as_ptr());
            xm1_scanout_wait(config.engine);
            xm1_scanout_formatter_strobe(config.engine);
            xm1_scanout_completion_event(config.engine, 5, 0);
        }
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(target_arch = "arm")]
pub fn action20(images: &ConfigImages<'_>, keys: &[ConfigurationKey; 4]) -> Result<ActionCompletion, BackendError> {
    if keys[0].read(images)? == 0 {
        match keys[1].read(images)? {
            0 => unsafe { xm1_scanout_panel_program_frontend(); },
            1 => {
                let register = keys[2].read(images)?;
                let value = keys[3].read(images)?;
                unsafe { xm1_scanout_panel_transfer_frontend(u32::from(register), u32::from(value)); }
            }
            _ => (),
        }
    }
    Ok(ActionCompletion::service_success())
}

#[cfg(test)]
mod tests {
    use super::DisplayIdentity;

    #[test]
    fn known_categories_use_active_mode_and_their_actual_route() {
        let ca = DisplayIdentity::from_configuration(0xca, 0xd8, 7).configuration();
        assert_eq!((ca.engine, ca.route, ca.mode, ca.alternate), (1, 1, 1, 1));
        let cb = DisplayIdentity::from_configuration(0xcb, 0xd8, 0).configuration();
        assert_eq!((cb.engine, cb.route, cb.mode, cb.alternate), (0, 2, 0, 0));
    }

    #[test]
    fn other_categories_use_timing_mode_but_zero_formatter_alternate() {
        let config = DisplayIdentity::from_configuration(0xcf, 0xd4, 1).configuration();
        assert_eq!((config.engine, config.route, config.mode, config.alternate), (0, 3, 2, 0));
    }
}
