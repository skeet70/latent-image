#![allow(dead_code)]

pub mod font_core;

#[path = "pipeline_tests/pipeline.rs"]
mod pipeline;

mod develop {
    pub use crate::font_core;
}
