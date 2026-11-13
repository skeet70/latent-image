//! Readings of a focus profile record that the controllers, the measurement round, the search
//! and the shot share: the record's scene, its level, its adjustment word, and the area setup
//! the search and the round program for the lens's current resource.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_27;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::exposure::ports::{ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::property::{self, OffsetTable};

/// The level step the shared level settings are read at for every record.
pub const RECORD_LEVEL_STEP: u8 = 3;

/// When armed, the shared block carries the scene in place of the record.
fn armed(ports: &mut impl ProfileAccess, record: RecordHandle) -> bool { ports.profile().header(record, SharedHeaderByte::At63) == 1 }

fn measurement_selection(ports: &mut impl ProfileAccess, record: RecordHandle) -> MeasurementSelection { ports.profile().measurement_selection(record) }

fn scene_kind(ports: &mut impl ProfileAccess, record: RecordHandle) -> u32 { u32::from(ports.profile().header(record, SharedHeaderByte::At31)) }

fn shared_scene(ports: &mut impl ProfileAccess, record: RecordHandle) -> u32 { ports.profile().read_shared(record, &group_27::AT_073C) }

/// The scene the record's areas use: the shared scene while the callback is armed and the record
/// is selected, none while it is armed and not selected, else the record's own scene kind.
pub fn area_scene(ports: &mut impl ProfileAccess, record: RecordHandle) -> u32 {
    if armed(ports, record) {
        if measurement_selection(ports, record) == MeasurementSelection::Record { shared_scene(ports, record) } else { 0 }
    } else {
        scene_kind(ports, record)
    }
}

/// The shared scene while the callback is armed, else the record's own scene kind.
pub fn scene(ports: &mut impl ProfileAccess, record: RecordHandle) -> u32 {
    if armed(ports, record) { shared_scene(ports, record) } else { scene_kind(ports, record) }
}

/// The record's scene kind as the auxiliary request's adjustment word; zero while the callback is armed.
pub fn adjustment_word(ports: &mut impl ProfileAccess, record: RecordHandle) -> u32 {
    if armed(ports, record) { 0 } else { scene_kind(ports, record) }
}

/// The shared follow word while the callback is armed, else the record's.
pub fn follow_word(ports: &mut impl ProfileAccess, record: RecordHandle) -> u16 {
    if armed(ports, record) {
        ports.profile().read_shared(record, &group_27::AT_0740)
    } else {
        ports.profile().read(record, &group_00::AT_002E)
    }
}

/// The record's level, or the shared level step's when the record is selected for measurement or
/// sets none.
pub fn level(ports: &mut (impl ProfileAccess + Properties), record: RecordHandle) -> i16 {
    let own: i16 = ports.profile().read(record, &group_00::AT_0022);
    if measurement_selection(ports, record) == MeasurementSelection::Record || own == 0 { settings::level_step(ports, RECORD_LEVEL_STEP) } else { own }
}

/// The record's priority; 1 when the record is selected for nothing or sets none.
pub fn priority(ports: &mut impl ProfileAccess, record: RecordHandle) -> u8 {
    if measurement_selection(ports, record) == MeasurementSelection::Unselected {
        return 1;
    }
    match ports.profile().read(record, &group_00::AT_003D) {
        0 => 1,
        own => own,
    }
}

/// The category of a dispatch context: 4 while active, else 2 for a profile at aspect index 0
/// and 3 for any other.
pub const fn context_category(active: bool, aspect_index: u8) -> u8 {
    if active { 4 } else if aspect_index == 0 { 2 } else { 3 }
}

/// What decides the exposure kind the shot and the engine take.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KindInputs {
    pub level: i16,
    /// The proposed kind stands as it is.
    pub fixed: bool,
    pub scene: u32,
    pub mode: u32,
    pub proposed: u32,
}

/// The level-step band a level falls in: 0 for level 0, else the step below the first of steps 2
/// to 10 above it, 10 from step 10 up (0x772ff8).
pub(crate) fn level_band(properties: &mut impl Properties, level: i16) -> u8 {
    if level == 0 {
        return 0;
    }
    (2..=10).find(|&step| level < settings::level_step(properties, step)).map_or(10, |step| step - 1)
}

/// 1 for a level in bands 1, 2, 9 or 10, 2 for scene 57, 1 for mode 2 or 3, else the proposed
/// kind (0xfa67a8).
pub fn exposure_kind(properties: &mut impl Properties, inputs: &KindInputs) -> u32 {
    if inputs.fixed {
        return inputs.proposed;
    }
    if matches!(level_band(properties, inputs.level), 1 | 2 | 9 | 10) {
        return 1;
    }
    if inputs.scene == 57 {
        return 2;
    }
    if (2..=3).contains(&inputs.mode) {
        return 1;
    }
    inputs.proposed
}

/// The lens state the area setup is programmed against.
pub trait LensResources {
    /// The lens's current resource; `None` while it has no valid one.
    fn resource(&mut self) -> Option<u32>;
    fn reference(&mut self) -> u32;
    fn flags(&mut self) -> u32;
    fn channel_bias(&mut self, resource: u32) -> i32;
}

/// One area as the configuration service programs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaSetup {
    pub resource: u32,
    pub scene: u32,
    pub kind: u8,
    /// 6 when the record's own kind applies, else 0.
    pub class: u8,
    /// The caller's two halfwords, which the configuration reads and rewrites.
    pub values: [i16; 2],
    pub flags: u32,
    pub auxiliary: u8,
    pub last: u32,
}

/// The second area of a pair, which also carries the lens reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferencedArea {
    pub area: AreaSetup,
    pub reference: u32,
}

/// Each call returns the two halfwords it leaves in place of `values` (0x74bd64, 0x74c568).
pub trait AreaConfiguration {
    fn configure(&mut self, area: &AreaSetup) -> [i16; 2];
    fn configure_referenced(&mut self, area: &ReferencedArea) -> [i16; 2];
}

/// What the search or the round asks of a programmed area pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaRequest {
    pub primary: [i16; 2],
    pub referenced: [i16; 2],
    /// Programs the areas without the record's kind.
    pub forced: bool,
    pub last: u32,
}

/// Kind 2 narrows to kind 1 for a resource at or beyond the configured resource count.
fn effective_kind(ports: &mut impl Properties, kind: u8, resource: u32) -> u8 {
    if kind == 2 && resource as i32 >= ports.property(property::AREA_RESOURCES) as i32 { 1 } else { kind }
}

/// What programming an area pair leaves: the area's output offset and both areas' halfwords.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgrammedPair {
    pub offset: u16,
    pub primary: [i16; 2],
    pub referenced: [i16; 2],
}

/// Programs the area pair for the lens's current resource; `None`, programming nothing, while the
/// lens has no valid resource.
pub fn program_area_pair(
    ports: &mut (impl ProfileAccess + Properties),
    lens: &mut impl LensResources,
    configuration: &mut impl AreaConfiguration,
    record: RecordHandle,
    request: AreaRequest,
) -> Option<ProgrammedPair> {
    let record_kind = ports.profile().header(record, SharedHeaderByte::At46);
    let scene = area_scene(ports, record);
    let reference = lens.reference();
    let flags = lens.flags();
    let resource = lens.resource()?;
    let kind = effective_kind(ports, record_kind, resource);
    let bias = lens.channel_bias(resource);
    let auxiliary = ports.profile().header(record, SharedHeaderByte::At61);
    let (kind_used, class) = if armed(ports, record) || request.forced { (0, 0) } else { (kind, 6) };
    let area = |values| AreaSetup { resource, scene, kind: kind_used, class, values, flags, auxiliary, last: request.last };
    let primary = configuration.configure(&area(request.primary));
    let referenced = configuration.configure_referenced(&ReferencedArea { area: area(request.referenced), reference });
    let table = if armed(ports, record) {
        OffsetTable::Standard
    } else {
        match kind {
            1 => OffsetTable::First,
            2 => OffsetTable::Second,
            _ => OffsetTable::Standard,
        }
    };
    let offset = settings::word(ports, property::area_offset(table, resource));
    Some(ProgrammedPair { offset: offset.wrapping_add(bias as u16), primary, referenced })
}

/// Programs the round's primary area for `resource` with the record's own kind and scene kind,
/// returning the halfwords it leaves.
pub fn program_round_area(
    ports: &mut (impl ProfileAccess + Properties),
    lens: &mut impl LensResources,
    configuration: &mut impl AreaConfiguration,
    record: RecordHandle,
    resource: u32,
    values: [i16; 2],
) -> [i16; 2] {
    let record_kind = ports.profile().header(record, SharedHeaderByte::At46);
    let scene = scene_kind(ports, record);
    let flags = lens.flags();
    let auxiliary = ports.profile().header(record, SharedHeaderByte::At61);
    let kind = effective_kind(ports, record_kind, resource);
    configuration.configure(&AreaSetup { resource, scene, kind, class: 0, values, flags, auxiliary, last: 0 })
}
