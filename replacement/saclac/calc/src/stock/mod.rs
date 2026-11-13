//! X-M1 1.50 (id5) layouts and addresses the library's owned code reaches: configuration ids,
//! the work memory shared with calc at 0x5ffb0000, the client's dump frame and the record bytes
//! the exposure engine parses.

pub mod adjust;
pub mod calculation;
pub mod capture;
pub mod context_trace;
pub mod dump;
pub mod lists;
pub mod motion;
pub mod property;
pub mod rate_curve;
pub mod rate_samples;
pub mod record;
pub mod scene;
pub mod scene_recognition;
pub mod shared;
pub mod shot_dump;
pub mod tables;
