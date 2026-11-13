//! The levels an evaluation starts from (0xf624a8).

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::exposure::context::Levels;
use crate::exposure::ports::Ports;
use crate::exposure::settings;
use crate::stock::property;

/// The level the record measures at: step 2 of the level table when the record selects its own
/// measurement, else the record's level (0xf5fc00).
pub fn measured_level(ports: &mut impl Ports, record: RecordHandle) -> i16 {
    let selection = ports.profile().header(record, SharedHeaderByte::MeasurementSelection);
    if selection == 1 { settings::level_step(ports, 2) } else { ports.profile().read(record, &group_00::AT_0022) }
}

/// Derives the start levels for `record`. On the adjustment path the target is step 3 of the
/// level table, else the configured target level.
pub fn start(ports: &mut impl Ports, record: RecordHandle, adjusting: bool) -> Levels {
    let policy = ports.profile().read(record, &group_00::AT_0045);
    let measured_level = measured_level(ports, record);
    let target = if adjusting { settings::level_step(ports, 3) } else { settings::word(ports, property::TARGET_LEVEL) as i16 };
    let configured = ports.property(property::ADJUSTMENT_FLAGS) & 1 != 0;
    let measured = if configured { ports.configured_level() } else { measured_level as u16 };
    let unmeasured = measured == 0;
    let level = if unmeasured { settings::level_step(ports, 3) } else { measured as i16 };
    let curve = if configured { ports.configured_profile(level) } else { ports.record_profile() };
    let offset = ports.level_offset(curve);
    let floor = settings::level_step(ports, 2);
    let above = level.wrapping_sub(offset).max(floor);
    Levels { target, start: offset.wrapping_add(above), measured, curve, unmeasured, policy: u32::from(policy) }
}
