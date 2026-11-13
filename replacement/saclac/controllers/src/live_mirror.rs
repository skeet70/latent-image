#![forbid(unsafe_code)]

//! The live view's copies of the profile into the profile geometry layer's display parameters
//! and status.

use crate::layers::{
    AutofocusMirror, DisplayRecords, Field, Geometry, GeometryValue, LensControl, MirrorSources, ProfileRecord,
};
use crate::live_profile::{Configured, Layers};
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_geometry::stock::{Parameters, StatusSnapshot};

/// Copies the applied primary profile into both records and returns the selection mode it
/// copied.
pub(crate) fn primary(record: RecordHandle, layers: &mut impl Layers) -> u32 {
    let mut parameters = layers.display().parameters();
    let mut status = layers.display().status();
    let ratio = read(layers, record, Field::FormatRatio);
    parameters.set_format_ratio(halves(ratio));
    status.set_output_mode(read(layers, record, Field::Selection));
    let selection_mode = layers.geometry().value(record, GeometryValue::SelectionMode);
    status.set_processing_option(selection_mode);
    status.set_geometry_enabled(read(layers, record, Field::ExposureFirst));
    status.set_geometry_kind(read(layers, record, Field::ExposureSecond));
    capture_window(&mut status, ratio, record, layers);
    status.set_scan_selection(layers.geometry().value(record, GeometryValue::ScanSelection));
    status.set_query_mode(read(layers, record, Field::ExposureSixth));
    status.set_format(layers.geometry().value(record, GeometryValue::Format));
    status.set_record_kinds([read(layers, record, Field::ExposureSeventh), read(layers, record, Field::ExposureEighth)]);
    status.set_at_a0(read(layers, record, Field::DisplayStatusByte) as u8);
    modes(&mut parameters, &mut status, record, layers);
    apply(&parameters, &status, layers);
    selection_mode
}

/// Copies the view configuration and the profile into both records.
pub(crate) fn configured(configured: &Configured, record: RecordHandle, layers: &mut impl Layers) {
    let mut parameters = layers.display().parameters();
    let mut status = layers.display().status();
    let values = &configured.values;
    let group = &values.group;
    let restored = if values.restorable != 0 { u32::from(values.level as u8) } else { 0 };
    parameters.set_profile_word(group.first.wrapping_add(restored));
    parameters.set_limit(group.sixth as i16);
    parameters.set_secondary_limit(group.seventh as i16);
    parameters.set_index(layers.mirror_sources().collapse_index(group.fifth));
    parameters.set_index_scale(layers.lens().configured_channel());
    parameters.set_area(values.windows[0]);
    parameters.set_mode_shift(layers.lens().position() as i16);
    parameters.set_format_ratio(values.converted);
    parameters.set_quad_mode(1);
    parameters.set_scan_thresholds([0, 0]);
    parameters.set_output(read(layers, record, Field::DisplayOutput));
    status.set_area(values.windows[1]);
    status.set_output_mode(configured.selection);
    status.set_processing_option(configured.selection_mode);
    status.set_geometry_enabled(read(layers, record, Field::ExposureFirst));
    status.set_geometry_kind(read(layers, record, Field::ExposureSecond));
    let ratio = u32::from(values.converted[0]) | u32::from(values.converted[1]) << 16;
    capture_window(&mut status, ratio, record, layers);
    status.set_at_a0(read(layers, record, Field::DisplayStatusByte) as u8);
    let linear = i32::from(group.eighth as i16) as u32;
    status.set_rate(layers.mirror_sources().linearize(linear) as u16);
    status.set_first_of(read(layers, record, Field::Command1dRestarts));
    status.set_multiple(read(layers, record, Field::PreparationRestorable) as u8);
    modes(&mut parameters, &mut status, record, layers);
    apply(&parameters, &status, layers);
}

/// Copies the groups an autofocus change kept running into the status, and returns the
/// selection mode when it copied one.
pub(crate) fn autofocus(mirror: AutofocusMirror, record: RecordHandle, layers: &mut impl Layers) -> Option<u32> {
    let mut status = layers.display().status();
    let selection_mode = mirror.selection_mode.then(|| layers.geometry().value(record, GeometryValue::SelectionMode));
    if let Some(mode) = selection_mode {
        status.set_processing_option(mode);
    }
    if mirror.convert_resource {
        let ratio = read(layers, record, Field::FormatRatio);
        capture_window(&mut status, ratio, record, layers);
    }
    if mirror.shift {
        status.set_format(layers.geometry().value(record, GeometryValue::Format));
    }
    if mirror.offset {
        status.set_query_mode(read(layers, record, Field::ExposureSixth));
    }
    let [mut first_kind, mut second_kind] = status.record_kinds();
    if mirror.scale {
        first_kind = read(layers, record, Field::ExposureSeventh);
    }
    if mirror.level {
        second_kind = read(layers, record, Field::ExposureEighth);
    }
    status.set_record_kinds([first_kind, second_kind]);
    if mirror.exposure {
        status.set_geometry_enabled(read(layers, record, Field::ExposureFirst));
    }
    if mirror.geometry {
        status.set_first_of(read(layers, record, Field::Command1dRestarts));
    }
    if mirror.any() {
        layers.display().apply_status(&status);
    }
    selection_mode
}

pub(crate) fn range(record: RecordHandle, layers: &mut impl Layers) {
    let mut status = layers.display().status();
    status.set_range(read(layers, record, Field::CaptureSecond));
    layers.display().apply_status(&status);
}

pub(crate) fn output(record: RecordHandle, layers: &mut impl Layers) {
    let mut parameters = layers.display().parameters();
    parameters.set_output(read(layers, record, Field::DisplayOutput));
    layers.display().apply_parameters(&parameters);
}

fn read(layers: &mut impl Layers, record: RecordHandle, field: Field) -> u32 { layers.profile().read(record, field) }

const fn halves(word: u32) -> [u16; 2] { [word as u16, (word >> 16) as u16] }

/// Sets the status dimensions to the capture window of the profile's format mode, when the
/// ratio has one there.
fn capture_window(status: &mut StatusSnapshot, ratio: u32, record: RecordHandle, layers: &mut impl Layers) {
    let mode = read(layers, record, Field::FormatMode);
    if let Some(window) = layers.mirror_sources().capture_window(mode, ratio) {
        status.set_dimensions(window);
    }
}

fn modes(parameters: &mut Parameters, status: &mut StatusSnapshot, record: RecordHandle, layers: &mut impl Layers) {
    parameters.set_mode(read(layers, record, Field::DisplayMode));
    parameters.set_mode_first(read(layers, record, Field::DisplayModeFirst));
    parameters.set_mode_second(read(layers, record, Field::DisplayModeSecond));
    parameters.set_mode_byte(read(layers, record, Field::DisplayModeByte) as i8);
    status.set_scan_index(read(layers, record, Field::DisplayScanIndex));
}

fn apply(parameters: &Parameters, status: &StatusSnapshot, layers: &mut impl Layers) {
    layers.display().apply_parameters(parameters);
    layers.display().apply_status(status);
}
