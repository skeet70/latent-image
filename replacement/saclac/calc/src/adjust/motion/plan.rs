//! The run's plan record, which the pipeline keeps between the plan's calls.

/// The S1 focus result the plan carries (plan +0x108).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FocusResult {
    /// 0 when a result stands, 2 when the run searches, 3 before either.
    pub kind: u32,
    pub position: u16,
    /// Row times seven plus column.
    pub area: u32,
    pub at_0c: u32,
    pub at_10: u32,
    pub at_18: u32,
    pub at_1c: u32,
}

/// What the plan tells the pipeline about the result (plan +0x168).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResultFlags {
    pub found: bool,
    pub searching: bool,
    pub at_08: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Plan {
    pub at_9c: u32,
    pub at_a0: u32,
    pub at_ac: u32,
    pub at_b0: u32,
    pub at_b4: u32,
    pub at_bc: u32,
    pub at_c0: u32,
    pub at_c8: u32,
    pub at_cc: u32,
    pub result: FocusResult,
    pub flags: ResultFlags,
    pub at_174: u32,
    pub at_178: u32,
    pub at_17c: u32,
}

impl Default for Plan {
    /// 0xf9c8e0.
    fn default() -> Self {
        Self {
            at_9c: 2,
            at_a0: 0,
            at_ac: 0x61,
            at_b0: 4,
            at_b4: 3,
            at_bc: 2,
            at_c0: 2,
            at_c8: 2,
            at_cc: 2,
            result: FocusResult { kind: 3, position: 0, area: 0x18, at_0c: 4, at_10: 3, at_18: 0, at_1c: 0 },
            flags: ResultFlags { found: false, searching: false, at_08: 2 },
            at_174: 2,
            at_178: 2,
            at_17c: 0,
        }
    }
}
