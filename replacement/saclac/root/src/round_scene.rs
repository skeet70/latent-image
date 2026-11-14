#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::{configuration::ProfileStore, lens::{BiasTable, LensConfiguration}, motion::ports::LensTables, profile_adjustment::{LensSteps, StepTable}}, exposure::ports::{Lens, ProfileAccess, Properties, Property, StockMemory}, profile::Profile, scene_recognition::{ports::{LensServices, Overlay, SceneEvents, SceneTrace, Steadiness, SubjectQuery, SubjectRequest}, Recognition, Scene, SceneFlags}};
use xm1_saclac_config::channel;
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, Calc, Calc1, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::{controller::Tables, geometry::AreaSource};
use xm1_saclac_profile_records::{Memory, ProfileRecords, StockEntries, stock::shared::group_20};
use xm1_saclac_shared::{settings::Channel, stock::{Argument::{Buffer, Word}, Machine}};
use crate::{lens_calibration::Log, round_ports::{forward, RoundPorts}, services::Services, stock::acquisition::Aligned};

macro_rules! implement {
    ($contract:path { $($methods:tt)* }) => {
        impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc, I: Isp, T: Tables>
            $contract for RoundPorts<'_, M, R, E, P, I, T> { $($methods)* }
    };
}
implement!(ProfileAccess {
    type Records = R;
    type Entries = E;
    fn profile(&mut self) -> Profile<'_, R, E> { ProfileAccess::profile(self.services) }
});
forward!(ProfileStore { fn records(&mut self) -> &mut ProfileRecords<R>; });
forward!(Properties { fn property(&mut self, property: Property) -> u32; });
forward!(StockMemory {
    fn read_u8(&mut self, at: SharedAddress) -> u8;
    fn read_u16(&mut self, at: SharedAddress) -> u16;
    fn read_u32(&mut self, at: SharedAddress) -> u32;
    fn write_u8(&mut self, at: SharedAddress, value: u8);
    fn write_u16(&mut self, at: SharedAddress, value: u16);
    fn write_u32(&mut self, at: SharedAddress, value: u32);
    fn fill(&mut self, at: SharedAddress, bytes: usize, value: u8);
});
forward!(Lens {
    fn level(&mut self, lens: u32, reference: u32) -> i16;
    fn base_level(&mut self, lens: u32) -> i16;
    fn reference_offset(&mut self, reference: u32) -> i16;
    fn calibration_index(&mut self, lens: u32) -> u32;
});
forward!(LensSteps {
    fn near_step(&mut self, reference: u32, limit: u16, which: StepTable) -> u32;
    fn far_step(&mut self, reference: u32, limit: u16, which: StepTable) -> u32;
    fn lens_step(&mut self, step: u32) -> u32;
});
forward!(LensConfiguration {
    fn channel_value(&mut self, channel: i32) -> u32;
    fn channel_curve(&mut self, channel: i32) -> u32;
    fn bias(&mut self, resource: i32, table: BiasTable) -> i16;
    fn biased_position(&mut self, resource: i32, table: BiasTable) -> i16;
    fn resource_count(&mut self) -> u8;
    fn selection_word(&mut self, channel: i32, flags: u32, offset: i16) -> u32;
    fn remap(&mut self, word: u32, channel: i32, mode: u32) -> i32;
    fn position_limits(&mut self, key: u32) -> [i16; 2];
    fn mapped_value(&mut self, key: u32) -> u32;
});
forward!(LensTables {
    fn channel_positions(&mut self, lens: u32, step: u32, reference: u32, mode: u32) -> [i16; 2];
    fn side_adjusted(&mut self, value: i16, from: u32, to: u32) -> i16;
    fn table_offset(&mut self, lens: u32, step: u32, delta: i16) -> i32;
    fn channel_start(&mut self, lens: u32, channel: u8, reference: u32) -> i16;
    fn period_position(&mut self, period: u16, lens: u32, step: u32, reference: u32) -> i32;
});
implement!(SubjectQuery {
    fn subjects(&mut self, request: &SubjectRequest) -> Option<u32> {
        self.geometry.standard_areas(&AreaSource { area: [request.chosen, request.at_0164, request.at_0168], ratio: request.row_scale }).ok().map(|areas| areas.count)
    }
});
implement!(Steadiness { fn steady(&mut self) -> bool { self.geometry.peak_active() } });
implement!(LensServices {
    fn lens_reference(&mut self) -> u32 { self.services.machine.enter(0x76_f340, &[]) }
    fn primary_tenths(&mut self, lens: u32) -> u32 { channel::base(&self.services.stock, Channel(lens as i32)) }
    fn response(&mut self, value: u16, lens: u32, step: u32, reference: u32) -> i32 { self.services.machine.enter(0x74_99a0, &[usize::from(value), lens as usize, step as usize, reference as usize, 1]) as i32 }
    fn converted_difference(&mut self, first: u32, second: u32) -> i16 { self.geometry.converted_difference(first, second) }
    fn level_class(&mut self, lens: u32) -> u32 { channel::channel_class(&self.services.stock, Channel(lens as i32)) }
    fn class_level(&mut self, lens: u32, setting: u32) -> i32 { i32::from(channel::level(&self.services.stock, &mut Log(&self.services.machine), Channel(lens as i32), setting, false)) }
});
implement!(Overlay {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) {
        overlay_text(&self.services.machine, text, column, row, foreground, background);
    }
});
pub(crate) fn overlay_text(machine: &impl Machine, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) {
    let mut line = Aligned([0; 25]);
    let length = text.len().min(24);
    line[..length].copy_from_slice(&text[..length]);
    machine.enter_with(0x111_0918, &[Buffer(0), Word(u32::from(column)), Word(u32::from(row)), Word(u32::from(foreground)), Word(u32::from(background))], &mut line.0);
}
implement!(SceneEvents {
    fn scene_changed(&mut self, handle: RecordHandle, recognition: Recognition) {
        let callback = self.services.shared(handle, &group_20::AT_016C);
        if callback != 0 { self.services.machine.enter(callback, &[recognition.scene.0 as usize, usize::from(recognition.flags.0), 0, 0, 1]); }
    }
    fn snapshot_adjustments(&mut self) {
        let machine = &self.services.machine;
        machine.enter(0x72_4b68, &[0xa8, 0xffff_ffff]);
        for (entry, offset) in [(0xfb_2438, 0), (0xfb_259c, 300), (0xfb_26a4, 0x192c), (0xfb_27dc, 0x1c2c), (0xfb_35d8, 0x1d58)] {
            machine.enter(entry, &[(0x016c_9164 + offset) as usize]);
        }
        machine.enter(0x72_4c00, &[0xa8]);
    }
});

const LOG_SOURCE: usize = 0x00ff_b104;
const SCENE_NAMES: [usize; 58] = [
    0xf52f50, 0xf52f58, 0xf52f68, 0xf52f84, 0xf52f90, 0xf52fb0, 0xf52fc0, 0xf52fdc, 0xf52ffc,
    0xf53008, 0xf53018, 0xf5302c, 0xf53048, 0xf53060, 0xf53080, 0xf530a0, 0xf530c4, 0xf530d8,
    0xf530f0, 0xf53100, 0xf53110, 0xf53124, 0xf53134, 0xf53144, 0xf53158, 0xf53168, 0xf53178,
    0xf53188, 0xf5319c, 0xf531b0, 0xf531c4, 0xf531d4, 0xf531e4, 0xf531f4, 0xf53204, 0xf5320c,
    0xf53218, 0xf53228, 0xf5322c, 0xf53238, 0xf53248, 0xf53250, 0xf53260, 0xf53274, 0xf53294,
    0xf532a4, 0xf532b8, 0xf532c4, 0xf532d4, 0xf532e4, 0xf532f4, 0xf53300, 0xf5330c, 0xf53318,
    0xf5332c, 0xf53340, 0xf53354, 0xf53368,
];
fn append(machine: &impl Machine, address: u32, line: &mut [u8; 64], length: &mut usize) {
    let mut offset = 0;
    while *length < line.len() - 1 {
        let byte = machine.read_u8(address + offset);
        if byte == 0 { break; }
        line[*length] = byte;
        *length += 1;
        offset += 1;
    }
}
implement!(SceneTrace {
    fn scene_name(&mut self, scene: Scene) {
        let format = SCENE_NAMES.get(scene.0 as usize).copied().unwrap_or(SCENE_NAMES[0]);
        self.services.machine.enter(0x64_744c, &[LOG_SOURCE, format]);
    }
    fn scene_flags(&mut self, flags: SceneFlags) {
        let mut line = Aligned([0; 64]);
        let mut length = 0;
        if flags.0 & 1 != 0 { append(&self.services.machine, 0xf5_3378, &mut line.0, &mut length); }
        if flags.0 & 2 != 0 { append(&self.services.machine, 0xf5_2f38, &mut line.0, &mut length); }
        if flags.0 == 0 { append(&self.services.machine, 0xf5_3380, &mut line.0, &mut length); }
        self.services.machine.enter_with(0x64_744c, &[Word(LOG_SOURCE as u32), Buffer(0)], &mut line.0);
    }
    fn measurements(&mut self, values: &[i32; 21]) {
        let mut arguments = [0; 23];
        arguments[..2].copy_from_slice(&[LOG_SOURCE, 0xf5_3390]);
        for (argument, value) in arguments[2..].iter_mut().zip(values) { *argument = *value as usize; }
        self.services.machine.enter(0x64_744c, &arguments);
    }
});
