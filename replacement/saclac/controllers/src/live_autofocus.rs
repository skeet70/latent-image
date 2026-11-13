#![forbid(unsafe_code)]

//! Applying an autofocus command's parameters to the profile: deciding whether the
//! running view must restart, and refreshing what the change touched.

use crate::layers::{
    AutofocusMirror, Block, Change, Configuration, Field, Geometry, GeometryCheck, LensSlot, ProfileRecord, Publication,
    Restarting, SharedHelpers,
};
use crate::live_profile::{Layers, LiveProfile};
use crate::live_view::Application;
use xm1_saclac_interface::{Parameters, RecordHandle};

/// The autofocus parameter word the lens pair starts from.
const LENS_WORD: usize = 38;

/// The autofocus changes the profile reports.
#[derive(Clone, Copy, Default)]
struct Changes {
    mode_decides: bool,
    selection_mode: bool,
    selection_owner: bool,
    selection_notice: bool,
    selection_reset: bool,
    view_geometry: bool,
    stop_two: bool,
    stop_four: bool,
    stop_five: bool,
    resource: bool,
    shift: bool,
    offset: bool,
    scale: bool,
    level: bool,
    exposure: bool,
    area: bool,
    forced: bool,
    class: bool,
    geometry: bool,
    property_gated: bool,
    restarting: bool,
    range: bool,
    output: bool,
    lens: bool,
    lens_position: bool,
    lens_range: bool,
}

impl Changes {
    fn read(record: RecordHandle, profile: &mut impl ProfileRecord) -> Self {
        let mut changed = |change| profile.changed(record, change);
        let mut restarting = false;
        for group in [Restarting::First, Restarting::Second, Restarting::Third, Restarting::Fourth, Restarting::Fifth, Restarting::Sixth] {
            restarting |= changed(Change::Restarting(group));
        }
        let mut property_gated = false;
        for group in 0..5 {
            property_gated |= changed(Change::PropertyGated(group));
        }
        Self {
            mode_decides: changed(Change::ModeDecides),
            selection_mode: changed(Change::SelectionMode),
            selection_owner: changed(Change::SelectionOwner),
            selection_notice: changed(Change::SelectionNotice),
            selection_reset: changed(Change::SelectionReset),
            view_geometry: changed(Change::ViewGeometry),
            stop_two: changed(Change::StopTwo),
            stop_four: changed(Change::StopFour),
            stop_five: changed(Change::StopFive),
            resource: changed(Change::Resource),
            shift: changed(Change::Shift),
            offset: changed(Change::Offset),
            scale: changed(Change::Scale),
            level: changed(Change::Level),
            exposure: changed(Change::Exposure),
            area: changed(Change::AutofocusArea) | changed(Change::AutofocusMode),
            forced: changed(Change::Forced),
            class: changed(Change::Class),
            geometry: changed(Change::Geometry),
            property_gated,
            restarting,
            range: changed(Change::Range),
            output: changed(Change::Output),
            lens: changed(Change::Lens),
            lens_position: changed(Change::LensPosition),
            lens_range: changed(Change::LensRange),
        }
    }
}

/// The autofocus change's outcome before its refreshes.
enum Decision { Restart, Keep { mirror: bool } }

pub(crate) fn apply(state: &mut LiveProfile, parameters: Parameters<'_>, record: RecordHandle, layers: &mut impl Layers) -> Application {
    let property = layers.configuration().search_property() != 0;
    let changes = Changes::read(record, layers.profile());
    if changes.selection_owner || changes.selection_notice {
        LiveProfile::notice_owner(record, layers);
    }
    if changes.selection_owner || changes.selection_reset {
        layers.geometry().reset_selection();
    }
    let restore = state.selection_mode_needs_restore(record, layers);
    let decision = decide(&changes, property, restore, record, layers);
    if let Decision::Keep { mirror: true } = decision {
        mirror(state, &changes, record, layers);
    }
    refresh(&changes, parameters, record, layers);
    match decision {
        Decision::Restart => Application::Restart,
        Decision::Keep { .. } => Application::Applied,
    }
}

fn decide(changes: &Changes, property: bool, restore: bool, record: RecordHandle, layers: &mut impl Layers) -> Decision {
    if changes.mode_decides {
        return if layers.profile().read(record, Field::ViewMode) == 3 { Decision::Restart } else { Decision::Keep { mirror: false } };
    }
    let class_hit = |layers: &mut _| changes.class && (4..=6).contains(&read(layers, record, Field::AutofocusClass));
    let gated = property && changes.property_gated;
    let forced = restore || changes.stop_five || changes.stop_two || changes.forced;

    let mut wanted = changes.selection_owner || changes.view_geometry || changes.stop_four || changes.restarting;
    if changes.area {
        wanted = read(layers, record, Field::Selection) != 1;
    }
    wanted |= changes.resource || forced || class_hit(layers) || gated;
    if !changes.geometry && !wanted {
        return Decision::Keep { mirror: true };
    }
    if layers.geometry().outgrown(record, GeometryCheck::Autofocus) {
        return Decision::Restart;
    }
    let mut fired = changes.area || forced;
    if changes.resource && read(layers, record, Field::Selection) == 1 {
        layers.helpers().convert_resource(record);
        fired = true;
    }
    fired |= changes.restarting || changes.geometry || class_hit(layers) || gated;
    if fired || changes.selection_owner { Decision::Restart } else { Decision::Keep { mirror: true } }
}

fn mirror(state: &mut LiveProfile, changes: &Changes, record: RecordHandle, layers: &mut impl Layers) {
    let unselected = read(layers, record, Field::Selection) == 0;
    let mirror = AutofocusMirror {
        selection_mode: changes.selection_mode,
        resource: changes.resource,
        convert_resource: changes.resource && unselected,
        shift: changes.shift || changes.selection_notice,
        offset: changes.offset,
        scale: changes.scale,
        level: changes.level,
        exposure: changes.exposure,
        geometry: changes.view_geometry,
    };
    if let Some(mode) = crate::live_mirror::autofocus(mirror, record, layers) {
        state.set_selection_mode(mode);
    }
}

fn refresh(changes: &Changes, parameters: Parameters<'_>, record: RecordHandle, layers: &mut impl Layers) {
    if changes.range { crate::live_mirror::range(record, layers); }
    if changes.output { crate::live_mirror::output(record, layers); }
    if changes.lens { layers.configuration().publish(record, Publication::Mode); }
    if changes.selection_mode {
        for publication in [Publication::SelectionMode, Publication::Position, Publication::Range] {
            layers.configuration().publish(record, publication);
        }
    } else {
        if changes.lens_position { layers.configuration().publish(record, Publication::Position); }
        if changes.lens_range { layers.configuration().publish(record, Publication::Range); }
    }
    if changes.geometry { layers.configuration().publish(record, Publication::Window); }

    let armed = read(layers, record, Field::SelectionCallbackArmed) == 1;
    if !armed || !changes.selection_mode || changes.selection_owner {
        layers.profile().write(record, Field::AutofocusApplied, 1);
        layers.profile().write(record, Field::AutofocusRelease, 0);
    }

    if !layers.geometry().lens_follows() { return; }
    let start = word(parameters, LENS_WORD);
    if changes.lens_range {
        let range = read(layers, record, Field::LensRange);
        let mut pair = [0; 2];
        layers.profile().read_block(record, Block::Lens(LensSlot::Current), &mut pair);
        if range != layers.configuration().combine_lens_pair(pair[0], pair[1], start) {
            let combined = layers.configuration().combine_lens_pair(start, range, start);
            write_lens_pair(record, [start, combined], layers);
        }
    } else if layers.profile().changed(record, Change::LensWindow) || layers.profile().changed(record, Change::LensValue) {
        let mut pair = [0; 2];
        layers.profile().read_block(record, Block::Lens(LensSlot::Next), &mut pair);
        let held = layers.configuration().combine_lens_pair(pair[0], pair[1], pair[0]);
        let searched = layers.helpers().searched_lens_pair(record);
        if searched != held {
            write_lens_pair(record, [start, searched], layers);
        }
    }
}

fn write_lens_pair(record: RecordHandle, pair: [u32; 2], layers: &mut impl Layers) {
    layers.profile().write_block(record, Block::Lens(LensSlot::Current), &pair);
    layers.profile().write_block(record, Block::Lens(LensSlot::Next), &pair);
}

fn read(layers: &mut impl Layers, record: RecordHandle, field: Field) -> u32 { layers.profile().read(record, field) }

fn word(parameters: Parameters<'_>, index: usize) -> u32 {
    let start = index * 4;
    parameters.bytes().get(start..start + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}
