#![no_std]

// A file named by #[path] looks up its child modules beside itself; the directory form keeps calibration_core/ as the child directory.
#[path = "../develop/src"]
mod develop {
    pub mod calibration_core;
}

pub use develop::calibration_core::*;
