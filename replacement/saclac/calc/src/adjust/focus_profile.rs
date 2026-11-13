//! Readings of a focus profile record that the controllers, the measurement round, the search
//! and the shot share: the record's scene, its level, its adjustment word, and the area setup
//! the search and the round program for the lens's current resource.

use xm1_saclac_interface::RecordHandle;

/// The record header byte that names what the record is selected for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeasurementSelection(pub u8);

impl MeasurementSelection {
    pub const fn is_none(self) -> bool { self.0 == 0 }
    pub const fn is_record(self) -> bool { self.0 == 1 }
}

/// The record and shared-block fields these readings combine.
pub trait RecordFields {
    /// When armed, the shared block carries the scene in place of the record.
    fn selection_callback_armed(&self, record: RecordHandle) -> bool;
    fn measurement_selection(&self, record: RecordHandle) -> MeasurementSelection;
    fn scene_kind(&self, record: RecordHandle) -> u8;
    fn shared_scene(&self) -> u32;
    fn follow_word(&self, record: RecordHandle) -> u16;
    fn shared_follow_word(&self) -> u16;
    /// Zero when the record sets no level of its own.
    fn level(&self, record: RecordHandle) -> i16;
    /// Zero when the record sets no priority of its own.
    fn priority(&self, record: RecordHandle) -> u8;
    /// The area kind the record asks for: 1 and 2 select their own offset tables.
    fn area_kind(&self, record: RecordHandle) -> u8;
    fn area_auxiliary(&self, record: RecordHandle) -> u8;
}

/// The level row the shared level settings are read at for every record.
pub const RECORD_LEVEL_ROW: u8 = 3;

/// The scene the record's areas use: the shared scene while the callback is armed and the record
/// is selected, none while it is armed and not selected, else the record's own scene kind.
pub fn area_scene(fields: &impl RecordFields, record: RecordHandle) -> u32 {
    if fields.selection_callback_armed(record) {
        if fields.measurement_selection(record).is_record() { fields.shared_scene() } else { 0 }
    } else {
        u32::from(fields.scene_kind(record))
    }
}

/// The shared scene while the callback is armed, else the record's own scene kind.
pub fn scene(fields: &impl RecordFields, record: RecordHandle) -> u32 {
    if fields.selection_callback_armed(record) { fields.shared_scene() } else { u32::from(fields.scene_kind(record)) }
}

/// The record's scene kind as the auxiliary request's adjustment word; zero while the callback is armed.
pub fn adjustment_word(fields: &impl RecordFields, record: RecordHandle) -> u32 {
    if fields.selection_callback_armed(record) { 0 } else { u32::from(fields.scene_kind(record)) }
}

/// The shared follow word while the callback is armed, else the record's.
pub fn follow_word(fields: &impl RecordFields, record: RecordHandle) -> u16 {
    if fields.selection_callback_armed(record) { fields.shared_follow_word() } else { fields.follow_word(record) }
}

/// The record's level, or `default_level` (the level row's setting) when the record is selected
/// for measurement or sets none.
pub fn level(fields: &impl RecordFields, record: RecordHandle, default_level: i16) -> i16 {
    let own = fields.level(record);
    if fields.measurement_selection(record).is_record() || own == 0 { default_level } else { own }
}

/// The record's priority; 1 when the record is selected for nothing or sets none.
pub fn priority(fields: &impl RecordFields, record: RecordHandle) -> u8 {
    if fields.measurement_selection(record).is_none() {
        return 1;
    }
    match fields.priority(record) {
        0 => 1,
        own => own,
    }
}

/// The category of a dispatch context: 4 while active, else 2 for a profile at aspect index 0
/// and 3 for any other.
pub const fn context_category(active: bool, aspect_index: u8) -> u8 {
    if active { 4 } else if aspect_index == 0 { 2 } else { 3 }
}

/// The lens state the area setup is programmed against.
pub trait LensResources {
    /// The lens's current resource; `None` while it has no valid one.
    fn resource(&self) -> Option<u32>;
    fn reference(&self) -> u32;
    fn flags(&self) -> u32;
    fn channel_bias(&self, resource: u32) -> i32;
}

/// Which offset table an area's output offset is read from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OffsetTable { Standard, First, Second }

pub trait AreaSettings {
    /// Resources at or above this count use kind 1 where the record asks for kind 2.
    fn resource_count(&self) -> u8;
    fn area_offset(&self, table: OffsetTable, resource: u32) -> u16;
}

/// One area as the configuration service programs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaSetup {
    pub resource: u32,
    pub scene: u32,
    pub kind: u8,
    /// 6 when the record's own kind applies, else 0.
    pub class: u8,
    pub values: [u32; 2],
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

pub trait AreaConfiguration {
    fn configure(&mut self, area: &AreaSetup);
    fn configure_referenced(&mut self, area: &ReferencedArea);
}

/// What the search or the round asks of a programmed area pair.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AreaRequest {
    pub primary: [u32; 2],
    pub referenced: [u32; 2],
    /// Programs the areas without the record's kind.
    pub forced: bool,
    pub last: u32,
}

fn effective_kind(kind: u8, resource: u32, settings: &impl AreaSettings) -> u8 {
    if kind == 2 && resource as i32 >= i32::from(settings.resource_count()) { 1 } else { kind }
}

/// Programs the area pair for the lens's current resource and returns the area's output offset;
/// `None`, programming nothing, while the lens has no valid resource.
pub fn program_area_pair(
    fields: &impl RecordFields,
    lens: &impl LensResources,
    settings: &impl AreaSettings,
    configuration: &mut impl AreaConfiguration,
    record: RecordHandle,
    request: AreaRequest,
) -> Option<u16> {
    let record_kind = fields.area_kind(record);
    let scene = area_scene(fields, record);
    let reference = lens.reference();
    let flags = lens.flags();
    let resource = lens.resource()?;
    let kind = effective_kind(record_kind, resource, settings);
    let bias = lens.channel_bias(resource);
    let auxiliary = fields.area_auxiliary(record);
    let (kind_used, class) = if fields.selection_callback_armed(record) || request.forced { (0, 0) } else { (kind, 6) };
    let area = |values| AreaSetup { resource, scene, kind: kind_used, class, values, flags, auxiliary, last: request.last };
    configuration.configure(&area(request.primary));
    configuration.configure_referenced(&ReferencedArea { area: area(request.referenced), reference });
    let table = if fields.selection_callback_armed(record) {
        OffsetTable::Standard
    } else {
        match kind {
            1 => OffsetTable::First,
            2 => OffsetTable::Second,
            _ => OffsetTable::Standard,
        }
    };
    Some(settings.area_offset(table, resource).wrapping_add(bias as u16))
}

/// Programs the round's primary area for `resource` with the record's own kind and scene kind.
pub fn program_round_area(
    fields: &impl RecordFields,
    lens: &impl LensResources,
    settings: &impl AreaSettings,
    configuration: &mut impl AreaConfiguration,
    record: RecordHandle,
    resource: u32,
    values: [u32; 2],
) {
    let kind = effective_kind(fields.area_kind(record), resource, settings);
    configuration.configure(&AreaSetup {
        resource,
        scene: u32::from(fields.scene_kind(record)),
        kind,
        class: 0,
        values,
        flags: lens.flags(),
        auxiliary: fields.area_auxiliary(record),
        last: 0,
    });
}
