pub(super) mod shared;
mod affine_math;
mod buffers;
mod crop_process;
mod date_overlay;
pub(super) mod debug_storage;
mod diagnostic_logging;
mod distortion;
mod encoder_task;
mod font_builtin;
mod font_fdl;
mod font_outline_vm;
mod font_raster_handlers;
mod font_raster_vm;
mod font_scanlines;
mod font_scripts;
mod font_script_store;
mod image_geometry;
mod image_operations;
mod image_pixels;
mod image_planes;
mod image_references;
mod image_resize;
mod motion_search;
mod object_allocator;
pub(super) mod operation_objects;
mod operation_predicates;
mod output_coefficients;
mod output_expansion;
mod output_jpeg;
mod output_metadata;
mod output_triplets;
mod outputs;
mod packed_software;
mod preparation;
mod processing;
mod profile_calibration;
mod redeye;
mod redeye_backend;
mod redeye_detector;
mod redeye_mask;
mod scan_numeric;
mod scan_processing;
mod selection;
mod sensor_calibration;
mod sensor_backend;
mod sensor_capture;
mod sensor_configuration;
mod sensor_descriptor;
mod sensor_filters;
mod sensor_geometry;
mod sensor_options;
mod sensor_publication;
mod sensor_register_fields;
mod sensor_registers;
mod sensor_requests;
mod sensor_settings;
mod sensor_snapshot_fields;
mod sensor_template;
mod strip_planner;
mod transforms;
mod warp_engine;

use shared::{OperationCode, StageContext};

#[derive(Clone, Copy)]
pub(super) enum PipelineStep {
    Preparation(preparation::Stage),
    Transform(transforms::Stage),
    Processing(processing::Stage),
    Output(outputs::Stage),
    Stamp(selection::Stage),
}

impl PipelineStep {
    pub(super) fn parse(command: u32) -> Option<Self> {
        let operation = OperationCode(command);
        if let Some(stage) = preparation::Stage::from_command(operation) {
            return Some(Self::Preparation(stage));
        }
        if let Some(stage) = transforms::Stage::from_command(operation) {
            return Some(Self::Transform(stage));
        }
        if let Some(stage) = processing::Stage::from_command(operation) {
            return Some(Self::Processing(stage));
        }
        if let Some(stage) = outputs::Stage::from_command(operation) {
            return Some(Self::Output(stage));
        }
        match command {
            0x13 => Some(Self::Stamp(selection::Stage::First)),
            0x14 => Some(Self::Stamp(selection::Stage::Second)),
            0x15 => Some(Self::Stamp(selection::Stage::Auxiliary)),
            _ => None,
        }
    }

    pub(super) unsafe fn run(self, context: *mut u8) -> u32 {
        let context = StageContext(context);
        unsafe {
            match self {
                Self::Preparation(stage) => preparation::execute(context, stage),
                Self::Transform(stage) => transforms::execute(context, stage),
                Self::Processing(stage) => processing::execute(context, stage),
                Self::Output(stage) => outputs::execute(context, stage),
                Self::Stamp(stage) => selection::execute(context, stage),
            }
        }.0
    }
}
