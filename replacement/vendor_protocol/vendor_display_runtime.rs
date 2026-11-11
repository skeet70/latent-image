//! X-M1 selection state at 0x955af8, derived from 6ce910/6ce930/6cfbe8.

use display_scanout_panel::panel_actor::PanelState;
use display_scanout_panel::panel_device::{self, BackendDevice, DeviceRuntime, SetterOutcome, SourceClass};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConfigurationType(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputFormat(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimarySource(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BridgeMode(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSelector(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HardwareSelector(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingSelector(u8);
impl TimingSelector {
    pub const fn parse(value: u32) -> Option<Self> { if value < 12 { Some(Self(value as u8)) } else { None } }
    pub const fn abi(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Selection {
    pub configuration: ConfigurationType,
    pub format: OutputFormat,
    pub primary: PrimarySource,
    pub bridge: BridgeMode,
    pub timing: TimingSelector,
    pub source: SourceSelector,
    pub hardware: HardwareSelector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SelectionState {
    current: Selection,
    previous_primary: PrimarySource,
    previous_bridge: BridgeMode,
    primary_pending: bool,
}
impl SelectionState {
    pub const fn adopt(current: Selection, previous_primary: PrimarySource, previous_bridge: BridgeMode, primary_pending: bool) -> Self {
        Self { current, previous_primary, previous_bridge, primary_pending }
    }

    pub const fn selected_source(bridge: BridgeMode, source: SourceSelector) -> SourceSelector {
        if bridge.0 == 0 { SourceSelector(0) } else { source }
    }

    pub const fn snapshot(&self) -> Selection { self.current }

    pub fn unchanged(&self, requested: Selection) -> bool {
        self.current.configuration == requested.configuration &&
        self.current.format == requested.format &&
        self.current.primary == requested.primary &&
        self.current.bridge == requested.bridge &&
        self.current.timing == requested.timing &&
        self.current.source == Self::selected_source(requested.bridge, requested.source) &&
        self.current.hardware == requested.hardware
    }

    pub fn select(&mut self, requested: Selection) {
        self.previous_primary = if requested.primary.0 == 0 { PrimarySource(1) } else { self.current.primary };
        self.current.primary = requested.primary;
        self.primary_pending = false;
        if requested.configuration.0 != 3 { self.current.configuration = requested.configuration; }
        self.current.format = requested.format;
        self.current.timing = requested.timing;
        self.previous_bridge = self.current.bridge;
        self.current.bridge = requested.bridge;
        self.current.source = Self::selected_source(requested.bridge, requested.source);
        self.current.hardware = HardwareSelector(0);
    }

    pub const fn old_engine_is_one(&self) -> bool { self.previous_bridge.0 == 0 && self.previous_primary.0 == 1 }
    pub const fn new_engine_is_one(&self) -> bool { self.current.bridge.0 == 0 && self.current.primary.0 == 1 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputMode { Disabled, Panel, External, Alternate }
impl OutputMode {
    pub const fn parse(value: u32) -> Option<Self> { match value { 0 => Some(Self::Disabled), 1 => Some(Self::Panel), 2 => Some(Self::External), 3 => Some(Self::Alternate), _ => None } }
    pub const fn abi(self) -> u32 { match self { Self::Disabled => 0, Self::Panel => 1, Self::External => 2, Self::Alternate => 3 } }
    pub const fn immediate(self) -> bool { matches!(self, Self::Panel | Self::Alternate) }
}

pub fn current_output(panel: &PanelState) -> OutputMode {
    let bit = 1u32 << panel.target.code();
    if bit & 0x22a8 != 0 { OutputMode::External }
    else if bit & 0x1154 != 0 { OutputMode::Panel }
    else { OutputMode::Disabled }
}

fn timing_source(source: PrimarySource) -> SourceClass {
    SourceClass::from_stock(match source.0 { 1 | 2 => source.0, 4..=10 => source.0 - 1, _ => 0 })
}

pub fn set_mode(source: PrimarySource, panel: &mut PanelState, runtime: &mut impl DeviceRuntime) -> Result<SetterOutcome, display_scanout_panel::Error> {
    let device = BackendDevice::parse(panel.target.code()).ok_or(display_scanout_panel::Error::InvalidArgument)?;
    Ok(panel_device::set_timing(&mut panel.device_state, device, timing_source(source), runtime))
}

#[cfg(target_arch = "arm")]
unsafe extern "C" {
    fn xm1_scanout_panel_device_set_timing(device: u32, timing: u32) -> i32;
}

#[cfg(target_arch = "arm")]
/// # Safety
/// The canonical panel state at 0x96bd58 and its device providers must be initialized and exclusively owned.
pub unsafe fn set_mode_native(source: PrimarySource) -> i32 {
    let device = unsafe { core::ptr::read_volatile(0x96bd74 as *const u32) };
    unsafe { xm1_scanout_panel_device_set_timing(device, timing_source(source).code()) }
}

#[cfg(test)]
#[path = "vendor_display_runtime_tests.rs"]
mod tests;
