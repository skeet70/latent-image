#![allow(dead_code)]

pub mod font_core;
pub mod calibration_core;

#[path = "pipeline_tests/pipeline.rs"]
mod pipeline;

mod develop {
    pub use crate::font_core;
    pub use crate::calibration_core;
}
