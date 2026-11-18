#![forbid(unsafe_code)]

//! X-M1 1.50 device registers the frame dumps copy and where each lands in the dump buffer.

/// `count` words from `register` on, each from its own register.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Run {
    pub register: u32,
    pub count: u16,
    pub offset: u32,
}

/// `count` words that each read `register` and add their byte offset to it (0xedbe90).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Repeated {
    pub register: u32,
    pub count: u16,
    pub offset: u32,
}

const fn run(register: u32, count: u16, offset: u32) -> Run { Run { register, count, offset } }

const fn repeated(register: u32, count: u16, offset: u32) -> Repeated { Repeated { register, count, offset } }

/// The pipeline dump (0xeddbc4).
pub mod pipeline {
    use super::{run, Run};

    /// The register whose bits enable each bank.
    pub const STATUS: u32 = 0xfff8_802c;
    pub const MARKS: [(u32, [u8; 4]); 2] = [(0xb_1520, *b"TMTS"), (0xb_16cc, *b"TMAL")];

    /// A bank: nine registers, thirty-one more, then thirteen chosen ones.
    pub struct Bank {
        pub enable: u32,
        pub runs: [Run; 2],
        pub chosen: [u32; 13],
        pub chosen_offset: u32,
    }

    const CHOSEN: [u32; 13] = [0xb0, 0xb4, 0xb8, 0xc0, 0xc4, 0xd0, 0xd4, 0xd8, 0xdc, 0xe0, 0xe4, 0xf0, 0xf4];

    const fn bank(enable: u32, base: u32, offset: u32) -> Bank {
        let mut chosen = CHOSEN;
        let mut index = 0;
        while index < chosen.len() {
            chosen[index] += base;
            index += 1;
        }
        Bank { enable, runs: [run(base, 9, offset), run(base + 0x30, 31, offset + 0x24)], chosen, chosen_offset: offset + 0xa0 }
    }

    pub const BANKS: [Bank; 2] = [bank(0x4, 0xfff1_2000, 0xb_1524), bank(0x4000, 0xfff1_3000, 0xb_15f8)];
}

/// The result dump (0xedde08).
pub mod result {
    use super::{repeated, run, Repeated, Run};

    const BASE: u32 = 0xd_f284;
    /// 0xede87c.
    pub const MARK: (u32, [u8; 4]) = (BASE + 0x374, *b"S-R0");
    /// Copied in order: `RUNS[0]`, `REPEATED[0]`, `RUNS[1]`, then the rest of `REPEATED`.
    pub const RUNS: [Run; 2] = [run(0xfff2_8000, 8, BASE + 0x378), run(0xfff2_8040, 18, BASE + 0x3b8)];
    pub const REPEATED: [Repeated; 19] = [
        repeated(0xfff2_8020, 8, BASE + 0x398),
        repeated(0xfff2_8088, 14, BASE + 0x400),
        repeated(0xfff2_80c0, 14, BASE + 0x438),
        repeated(0xfff2_8100, 0x400, BASE + 0x470),
        repeated(0xfff2_9100, 64, BASE + 0x1470),
        repeated(0xfff2_9200, 64, BASE + 0x1570),
        repeated(0xfff2_9300, 64, BASE + 0x1670),
        repeated(0xfff2_9400, 64, BASE + 0x1770),
        repeated(0xfff2_9500, 64, BASE + 0x1870),
        repeated(0xfff2_9600, 64, BASE + 0x1970),
        repeated(0xfff2_9700, 64, BASE + 0x1a70),
        repeated(0xfff2_9800, 64, BASE + 0x1b70),
        repeated(0xfff2_9900, 64, BASE + 0x1c70),
        repeated(0xfff2_9c00, 256, BASE + 0x1d70),
        repeated(0xfff2_a000, 256, BASE + 0x2170),
        repeated(0xfff2_a400, 256, BASE + 0x2570),
        repeated(0xfff2_a800, 256, BASE + 0x2970),
        repeated(0xfff2_ac00, 256, BASE + 0x2d70),
        repeated(0xfff2_b000, 256, BASE + 0x3170),
    ];
}
