#![forbid(unsafe_code)]

//! The focus phase: hold the lens and watch the contrast for a reason to search again
//! (0xf400fc).

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Focus {
    pub contrast: u32,
    pub count: u8,
    pub flags: u8,
}

impl Focus {
    pub(crate) const fn new() -> Self { Self { contrast: 0, count: 0, flags: 0 } }
}
