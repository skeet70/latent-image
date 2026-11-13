//! The statistics the pipeline gathers over a run's frames (0xf99874): the area's cell size and
//! rates, the channel scales and weights, the level bounds and the curve table.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::record::group_00;

use crate::adjust::metering::MeteringTables;
use crate::adjust::motion::exposure::FrameExposure;
use crate::adjust::motion::ports::{FrameGeometry, GridArea, StatisticsFrameRequest};
use crate::adjust::motion::readings::RecordReadings;
use crate::adjust::rate_policy;
use crate::adjust::scene_settings::{self, SceneSelection};
use crate::exposure::ports::{ProfileAccess, Properties, Property};
use crate::stock::motion::{self as stock};

/// Profiles above this feature class sample at twice the channel weights.
const DOUBLED_WEIGHT_CLASS: u8 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Statistics {
    pub cell_size: [u16; 2],
    /// Where the pipeline traces the statistics in the dump frame.
    pub trace: SharedAddress,
    /// The configured rate of the area and its tenth, doubled tenth and tenth again.
    pub rates: [u32; 4],
    /// Red, green and blue in 128ths.
    pub channel_scales: [u16; 3],
    pub channel_weights: [u16; 3],
    /// The first frame's exposure less its level.
    pub exposure_offset: i16,
    pub level_bounds: [i16; 2],
    pub curve_table: Property,
    pub class: u8,
    pub area: GridArea,
    pub buffers: [SharedAddress; 5],
}

impl Statistics {
    pub fn set_up(ports: &mut (impl Properties + ProfileAccess + FrameGeometry + MeteringTables), readings: &RecordReadings, first: &FrameExposure, section: SharedAddress) -> Self {
        let [height, width] = ports.statistics_frame(&StatisticsFrameRequest {
            profile: first.profile,
            lens: readings.lens,
            row_scale: readings.row_scale,
            selection_value: readings.selection_value,
            at_0164: readings.at_0164,
            at_0168: readings.at_0168,
        });
        let expanded = ports.expanded_ratio(height.wrapping_mul(width));
        let rate = rate_policy::scaled_by_configured_factor(ports, expanded);
        let [red, blue] = ports.feature_bytes(first.profile);
        let mut channel_weights = ports.channel_weights();
        if ports.feature_class(first.profile) > DOUBLED_WEIGHT_CLASS {
            channel_weights[1] <<= 1;
            channel_weights[2] <<= 1;
        }
        let flags: u16 = ports.profile().read(readings.record, &group_00::AT_002E);
        let selection = SceneSelection { measurement_selection: readings.measurement_selection, scene: u32::from(readings.scene), flags };
        let trace = stock::statistics_trace(section);
        Self {
            cell_size: [16, 16],
            trace,
            rates: [rate, rate / 10, (rate / 10) << 1, rate / 10],
            channel_scales: [u16::from(red), 0x80, u16::from(blue)],
            channel_weights,
            exposure_offset: first.at_06,
            level_bounds: [250, -900],
            curve_table: scene_settings::curve_table(selection),
            class: readings.class,
            area: readings.area,
            buffers: stock::STATISTICS_BUFFERS,
        }
    }
}
