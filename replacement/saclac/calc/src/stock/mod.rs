//! X-M1 1.50 (id5) layouts and addresses the library's owned code reaches: configuration ids,
//! the work memory shared with calc at 0x5ffb0000, the client's dump frame and the record bytes
//! the exposure engine parses.

pub mod calculation;
pub mod dump;
pub mod property;
pub mod record;
pub mod scene;
pub mod shared;
pub mod shot_dump;
