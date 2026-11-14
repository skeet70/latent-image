use xm1_saclac_profile_geometry::{ports::*, memory::Location, profile::{ProfileHandle, Rectangle}};
use xm1_saclac_shared::stock::{Argument::{Buffer, Word}, Machine, Stock};
use crate::geometry::GeometryBindings;
use super::geometry::{address, Memory};

fn word(bytes: &[u8], at: usize) -> u32 { u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) }
fn half(bytes: &[u8], at: usize) -> u16 { u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) }
fn rectangle(m: &impl Machine, entry: u32, profile: ProfileHandle, relative: Option<bool>) -> Rectangle {
    let mut bytes = [0; 16];
    if let Some(relative) = relative { m.enter_with(entry, &[Word(profile.0), Word(u32::from(relative)), Buffer(0)], &mut bytes); }
    else { m.enter_with(entry, &[Word(profile.0), Buffer(0)], &mut bytes); }
    Rectangle { top: word(&bytes, 0), left: word(&bytes, 4), width: word(&bytes, 8), height: word(&bytes, 12) }
}

impl<M: Machine + Clone, P, I> Retained for GeometryBindings<M, P, I> {
    fn limit_triplet(&mut self, triplet: [u16; 3]) -> [u16; 3] {
        let mut bytes = [0; 12];
        for (to, value) in bytes[..6].chunks_exact_mut(2).zip(triplet) { to.copy_from_slice(&value.to_le_bytes()); }
        self.stages.machine.enter_with(0xef_4bf4, &[Buffer(0), Buffer(6), Word(0)], &mut bytes);
        [half(&bytes, 6), half(&bytes, 8), half(&bytes, 10)]
    }
    fn expanded_rectangle(&mut self, profile: ProfileHandle) -> Rectangle { rectangle(&self.stages.machine, 0xef_54fc, profile, None) }
    fn configured_rectangle(&mut self, profile: ProfileHandle) -> Rectangle { rectangle(&self.stages.machine, 0xef_5190, profile, None) }
    fn relative_rectangle(&mut self, profile: ProfileHandle, relative: bool) -> Rectangle { rectangle(&self.stages.machine, 0xef_5d44, profile, Some(relative)) }
    fn encode_thresholds(&mut self, first: i16, second: i16) -> u32 { self.stages.machine.enter(0xef_743c, &[first as usize, second as usize]) }
    fn converted_decade(&mut self, selection: u32) -> i32 { self.stages.machine.enter(0xef_74e4, &[selection as usize]) as i32 }
    fn statistics_offset(&mut self, region: StatisticsRegion) -> u32 { let mut bytes = [0; 4]; self.stages.machine.enter_with(0xed_ae88, &[Word(region.0), Buffer(0)], &mut bytes); word(&bytes, 0) }
    fn selection_limit(&mut self) -> u32 { self.stages.machine.enter(0xee_a014, &[]) }
    fn profile_factors(&mut self, selection: u32) -> [u16; 2] { let mut bytes = [0; 4]; self.stages.machine.enter_with(0xef_4274, &[Word(selection), Buffer(0)], &mut bytes); [half(&bytes, 0), half(&bytes, 2)] }
    fn calibration_factors(&mut self, first: u32, second: u32) -> [u16; 3] { let mut bytes = [0; 6]; self.stages.machine.enter_with(0xef_3f04, &[Word(first), Word(second), Buffer(0)], &mut bytes); [half(&bytes, 0), half(&bytes, 2), half(&bytes, 4)] }
    fn configured_decade(&mut self, offset: u8) -> i16 { self.stages.machine.enter(0xef_74ac, &[offset as usize]) as i16 }
    fn decode_level(&mut self, encoded: u32) -> u8 { self.stages.machine.enter(0xef_73e4, &[encoded as usize]) as u8 }
}
impl<M: Machine, P, I> OutputRoutes for GeometryBindings<M, P, I> {
    fn set_requested_level(&mut self, route: u32, level: u32) -> i32 { self.stages.machine.enter(0x64_982c, &[route as usize, level as usize]) as i32 }
}
impl<M: Machine, P, I> Interface for GeometryBindings<M, P, I> {
    fn read_adc_byte(&mut self, channel: u32) -> AdcReading { let mut bytes = [0]; let status = self.stages.machine.enter_with(0x74_1ee0, &[Word(channel), Buffer(0)], &mut bytes); AdcReading { status, value: bytes[0] } }
    fn wait_ticks(&mut self, ticks: u32) { xm1_saclac_config::services::delay(&mut Delay(&self.stages.machine), ticks); }
    fn configure_task(&mut self, task: u32, priority: u32) -> i32 { self.stages.machine.enter(0x72_3a14, &[task as usize, priority as usize]) as i32 }
}
struct Delay<'a, M>(&'a M);
impl<M: Machine> xm1_saclac_config::services::KernelDelay for Delay<'_, M> { fn delay(&mut self, ticks: u32) -> u32 { self.0.enter(0x72_3ce8, &[ticks as usize]) } }
impl<M: Machine, P, I> DeviceRegisters for GeometryBindings<M, P, I> { fn register(&mut self, register: Register) -> u32 { self.stages.machine.read_u32(register.0) } }
impl<M: Machine, P, I> PeakService for GeometryBindings<M, P, I> {
    fn read_selection(&mut self) { self.stages.machine.enter(0x6b_7830, &[]); }
    fn notify_return(&mut self) { self.stages.machine.enter(0x6b_7840, &[]); }
    fn state(&mut self) -> u32 { self.stages.machine.enter(0x6b_7820, &[]) }
}
impl<M: Machine + Clone, P, I> LensValues for GeometryBindings<M, P, I> { fn lens_value(&mut self, index: u32) -> i16 { xm1_saclac_shared::lens::LensControl::zone_limit(&mut Stock::new(self.stages.machine.clone()), index as u8) } }
impl<M: Machine, P, I> ProfileResources for GeometryBindings<M, P, I> { fn resource_byte(&mut self, record: u32) -> u8 { self.stages.machine.enter(0x75_0474, &[record as usize]) as u8 } }
impl<M: Machine, P, I> ConfiguredRate for GeometryBindings<M, P, I> { fn configured_rate(&mut self) -> u16 { self.stages.machine.enter(0x100_7ce0, &[]) as u16 } }
impl<M: Machine, P, I> DumpRegions for GeometryBindings<M, P, I> {
    fn reserve_region(&mut self, region: u32, address: u32, bytes: u32) { self.stages.machine.enter(0x75_e6d8, &[region as usize, address as usize, bytes as usize]); }
    fn register_region(&mut self, region: u32, address: u32, bytes: u32) { self.stages.machine.enter(0x75_e704, &[region as usize, address as usize, bytes as usize]); }
    fn release_region(&mut self, region: u32) { self.stages.machine.enter(0x75_e6f0, &[region as usize]); }
}
impl<M: Machine, P, I> Handlers for GeometryBindings<M, P, I> {
    fn attach(&mut self, unit: HandlerUnit) -> Result<u32, u32> {
        let unit = match unit { HandlerUnit::First => 0, HandlerUnit::Second => 1 };
        let mut bytes = [0; 4];
        let result = self.stages.machine.enter_with(0x74_11c4, &[Word(unit), Word(super::geometry::frame_handler as *const () as usize as u32), Word(2), Buffer(0)], &mut bytes);
        if result == 0 { Ok(word(&bytes, 0)) } else { Err(result) }
    }
    fn detach(&mut self, unit: HandlerUnit, handle: u32) -> u32 { self.stages.machine.enter(0x74_140c, &[match unit { HandlerUnit::First => 0, HandlerUnit::Second => 1 }, handle as usize]) }
    fn notify_stopped(&mut self, operation: xm1_saclac_profile_geometry::controller::Operation, callback: StockCode) {
        use xm1_saclac_profile_geometry::controller::Operation;
        let fourth = match operation { Operation::Primary | Operation::Third | Operation::Channel => 0, _ => callback.0 as usize };
        let args = [1, 0, 0, fourth, 0];
        self.stages.machine.enter(callback.0, if matches!(operation, Operation::Primary) { &args } else { &args[..4] });
    }
}

fn label(m: &impl Machine, at: u32, limit: usize, into: &mut [u8]) -> usize {
    let mut len = 0;
    for byte in into.iter_mut().take(limit) {
        let at = at + len as u32;
        let value = (m.read_u16(at & !1) >> ((at & 1) * 8)) as u8;
        if value == 0 { break; }
        *byte = value; len += 1;
    }
    len
}
impl<M: Machine, P, I> DiagnosticLabels for GeometryBindings<M, P, I> {
    fn profile_label(&mut self, profile: ProfileHandle, into: &mut [u8]) -> usize { let at = 0xee_b3a8 + profile.0; let index = (self.stages.machine.read_u16(at & !1) >> ((at & 1) * 8)) as u8; label(&self.stages.machine, 0xee_b460 + 9 * u32::from(index), 9, into) }
    fn feature_label(&mut self, feature: u8, into: &mut [u8]) -> usize { label(&self.stages.machine, 0xee_b3d0 + 12 * u32::from(feature), 12, into) }
    fn fixed_label(&mut self, into: &mut [u8]) -> usize { label(&self.stages.machine, 0xee_b2c4, usize::MAX, into) }
    fn rate_separator(&mut self, into: &mut [u8]) -> usize { label(&self.stages.machine, 0xee_b2d0, usize::MAX, into) }
    fn rate_suffix(&mut self, into: &mut [u8]) -> usize { label(&self.stages.machine, 0xee_b2d4, usize::MAX, into) }
}
impl<M: Machine, P, I> StatisticsMemory for GeometryBindings<M, P, I> {
    fn statistics_word(&mut self, buffer: StatisticsBuffer, offset: u32) -> u32 { self.stages.machine.read_u32(buffer.0 + offset) }
    fn low_word(&mut self, at: LowAddress) -> u32 {
        #[cfg(target_arch = "arm")]
        { let value: u32; unsafe { core::arch::asm!("ldr {value}, [{address}]", value = out(reg) value, address = in(reg) u32::from(at.0), options(nostack, readonly)); } value }
        #[cfg(not(target_arch = "arm"))]
        { self.stages.machine.read_u32(u32::from(at.0)) }
    }
}
impl<M: Machine, P, I> FrameDumps for GeometryBindings<M, P, I> {
    fn write_dump(&mut self, buffer: DumpBuffer, offset: u32, bytes: &[u8]) { for (i, byte) in bytes.iter().enumerate() { self.stages.machine.write_u8(buffer.0 + offset + i as u32, *byte); } }
    fn dump_policy(&mut self, buffer: DumpBuffer, dump: PolicyDump, policy: Location) {
        match dump {
            PolicyDump::Pipeline => {
                let mut stream = xm1_saclac_config::stock::StockStreamSnapshot::of_stream(buffer.0);
                xm1_saclac_config::compiled::snapshot_options(&mut stream, &mut super::geometry::Hardware(&self.stages.machine), xm1_saclac_config::stock::StreamSource(address(policy)));
            }
            PolicyDump::Result => { self.stages.machine.enter(0xd6_b3f8, &[buffer.0 as usize, address(policy) as usize]); }
        }
    }
}
impl<M: Machine, P, I> DisplayDriver for GeometryBindings<M, P, I> {
    fn write_selected_quad(&mut self, channel: u32, values: Location) -> bool { self.stages.machine.enter(0x6c_fed0, &[channel as usize, address(values) as usize]) == 0 }
    fn write_selected_channel(&mut self, channel: u32, value: Location) -> bool { self.stages.machine.enter(0x6c_fe14, &[channel as usize, address(value) as usize]) == 0 }
    fn apply_scale_layout(&mut self, value: Location) -> bool { self.stages.machine.enter(0x6d_0000, &[address(value) as usize]) == 0 }
    fn graphic_service(&mut self, value: u32) -> bool { self.stages.machine.enter(0x6d_1b24, &[value as usize]) == 0 }
    fn update_configuration_bits(&mut self, value: u32) -> bool { self.stages.machine.enter(0x6c_ffbc, &[value as usize]) == 0 }
    fn forward_selection(&mut self, value: Location) -> bool { self.stages.machine.enter(0x6d_0244, &[address(value) as usize]) == 0 }
    fn apply_selected(&mut self, value: u32) -> bool { self.stages.machine.enter(0x6d_1520, &[value as usize]) == 0 }
    fn set_selected_colour(&mut self, rgb: u32) { self.stages.machine.enter(0x6d_0ee4, &[rgb as usize]); }
    fn forward_selected_value(&mut self, value: u32) -> bool { self.stages.machine.enter(0x6d_01bc, &[value as usize]) == 0 }
    fn on_selected_event(&mut self, callback: StockCode) -> bool { self.stages.machine.enter(0x6c_fcf4, &[callback.0 as usize]) == 0 }
}
fn completion(completion: FileCompletion) -> usize { match completion { FileCompletion::Opened => file_opened as *const () as usize, FileCompletion::Done => file_done as *const () as usize } }
impl<M: Machine, P, I> DumpFiles for GeometryBindings<M, P, I> {
    fn next_file_name(&mut self, name: Location) { self.stages.machine.enter(0x70_fbfc, &[address(name) as usize, 0, 0]); }
    fn copy_text(&mut self, to: Location, from: Location) { self.stages.machine.enter(0x74_2250, &[address(to) as usize, address(from) as usize]); }
    fn append_text(&mut self, to: Location, text: &'static [u8]) { self.stages.machine.enter(0x74_2260, &[address(to) as usize, text.as_ptr() as usize]); }
    fn open_file(&mut self, callback: FileCompletion, path: Location) -> u32 { self.stages.machine.enter(0x71_c648, &[completion(callback), address(path) as usize, 1]) }
    fn write_file(&mut self, callback: FileCompletion, handle: u32, length: u32, data: StockData) { self.stages.machine.enter(0x71_c75c, &[completion(callback), handle as usize, length as usize, data.0 as usize]); }
    fn close_file(&mut self, callback: FileCompletion, handle: u32) { self.stages.machine.enter(0x71_c7bc, &[completion(callback), handle as usize, 0, 0]); }
    fn finish_file(&mut self, callback: FileCompletion, handle: u32) { self.stages.machine.enter(0x71_c938, &[completion(callback), handle as usize]); }
    fn read_file_numbers(&mut self, numbers: StockData) { self.stages.machine.enter(0x70_fb68, &[numbers.0 as usize, (numbers.0 + 2) as usize]); }
}
pub extern "C" fn file_opened(status: u32, handle: u32) { xm1_saclac_profile_geometry::services::file_opened(&mut Memory(xm1_saclac_shared::stock::Guest), status, handle); }
pub extern "C" fn file_done(status: u32) { xm1_saclac_profile_geometry::services::file_done(&mut Memory(xm1_saclac_shared::stock::Guest), status); }
