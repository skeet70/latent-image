#![cfg(feature = "scoped-watch")]

use fujimod_emulator::board::bus::{Bus, Mode};
use fujimod_emulator_devices::{
    graph::{DeviceGraph, Mapping},
    parts::nand::{Nand, Options},
};
use fujimod_emulator_exr2_plan::{
    BuiltBoard, Model, Resources,
    construction::{self, Builder, Built},
};
use std::{collections::BTreeMap, sync::Arc};

pub(super) enum Timing {
    BeforeHandoff,
    AfterInstall,
}

pub(super) enum Transfer {
    Installer,
    CompletionGuards,
}

const NAND_BASE: u64 = 0xfff5_2000;
const NAND_WINDOW: u64 = 0x1000;
const STUB: u32 = 0x43000;

fn branch(from: u32, to: u32) -> u32 {
    0xea00_0000 | ((to.wrapping_sub(from + 8) >> 2) & 0x00ff_ffff)
}

fn put_word(memory: &mut [u8], address: usize, word: u32) {
    memory[address..address + 4].copy_from_slice(&word.to_le_bytes());
}

// The guest uses only r8/r9 and polls the model's receive-DMA activity register.
pub(super) fn build_bus(memory: &mut [u8], timing: Timing, transfer: Transfer) -> Bus {
    let units = match transfer {
        Transfer::Installer => 1,
        Transfer::CompletionGuards => 6,
    };
    let (entry, continuation) = match timing {
        Timing::BeforeHandoff => (0x6018, 0x6020),
        Timing::AfterInstall => (0x4008c, 0x40a00),
    };
    put_word(memory, entry, branch(entry as u32, STUB));
    for (index, instruction) in [
        0xe59f80f8, // LDR r8,[pc,#0xf8]: NAND base at 0x43100.
        0xe3a09001, // MOV r9,#1: large-page mode.
        0xe5889000, // STR r9,[r8].
        0xe59f90f0, // LDR r9,[pc,#0xf0]: read setup at 0x43104.
        0xe588901c, // STR r9,[r8,#0x1c].
        0xe3a09000, // MOV r9,#0.
        0xe5889020, // STR r9,[r8,#0x20]: column zero.
        0xe5889024, // STR r9,[r8,#0x24]: row zero.
        0xe59f90e0, // LDR r9,[pc,#0xe0]: RX enable at 0x43108.
        0xe5889884, // STR r9,[r8,#0x884].
        0xe59f90dc, // LDR r9,[pc,#0xdc]: RX configuration at 0x4310c.
        0xe58898a0, // STR r9,[r8,#0x8a0].
        0xe59f90d8, // LDR r9,[pc,#0xd8]: destination at 0x43110.
        0xe58898a4, // STR r9,[r8,#0x8a4].
        0xe59f90d4, // LDR r9,[pc,#0xd4]: RX count at 0x43114.
        0xe58898a8, // STR r9,[r8,#0x8a8].
        0xe59f90d0, // LDR r9,[pc,#0xd0]: read command at 0x43118.
        0xe5889008, // STR r9,[r8,#8]: submit the read.
        0xe5989890, // LDR r9,[r8,#0x890]: RX activity.
        0xe3590000, // CMP r9,#0.
        0x1afffffc, // BNE 0x43048.
        branch(0x43054, continuation),
    ].into_iter().enumerate() {
        put_word(memory, STUB as usize + 4 * index, instruction);
    }
    for (address, value) in [
        (0x43100, NAND_BASE as u32),
        (0x43104, 0x0030_0000 | units),
        (0x43108, 0x0001_0001),
        (0x4310c, 0x0000_1000),
        (0x43110, 0x0004_0000),
        (0x43114, 0x0010_0000),
        (0x43118, 0x0080_23f3),
    ] {
        put_word(memory, address, value);
    }

    // DMA replays the final code bytes, including the AfterInstall branch.
    let medium = Nand::new(
        Options {
            bytes: 2048 * 64,
            page_bytes: 2048,
            pages_per_block: 64,
            spare_bytes: 64,
        },
        Arc::from(&memory[0x40000..0x40000 + 512 * units as usize]),
        Vec::new(),
        BTreeMap::new(),
    ).unwrap();
    let mut resources = Resources::default();
    resources.inputs.nands.insert("composed-fixture".into(), medium);
    let mut built = BuiltBoard {
        graph: DeviceGraph::new(),
        node_ids: BTreeMap::new(),
        core_interrupts: Vec::new(),
        contacts: BTreeMap::new(),
        resources: Resources::default(),
    };
    let windows = [Mapping { base: NAND_BASE, size: NAND_WINDOW }];
    fujimod_emulator_models::register();
    let device = construction::constructors().unwrap().build(
        Builder {
            name: "composed-fixture",
            id: 0,
            windows: &windows,
            resources: &mut resources,
            built: &mut built,
            start_tick: 0,
        },
        Model::Nand {
            options: serde_json::json!({"erase_ticks": 64}),
            medium: "composed-fixture".into(),
        },
    ).unwrap();
    let Built::Device(device) = device else {
        panic!("NAND constructor returned multiple devices");
    };
    let mut bus = Bus::new(Mode::Strict);
    bus.register(NAND_BASE, NAND_WINDOW, device).unwrap();
    bus
}
