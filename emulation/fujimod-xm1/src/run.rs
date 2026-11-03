//! The X-M1 run document and the strict FujiMod session it opens.

use crate::guest::FirmwareFacts;
use crate::provenance::{Guest, Provenance, RamFact, plan_windows};
use fujimod_catalog::configuration::{
    GeneratedSceneAsset, ResolvedConfiguration, RunEngine, RunExecution, RunExecutionMode,
};
use fujimod_catalog::reference::{MemoryKind, RegionSize, Soc};
use fujimod_emulator::{
    board::{
        bus::{Bus, Mode},
        factory,
    },
    session::{Session, preparation},
};
use fujimod_emulator_cpu::arm::Coprocessor;
use crate::adapters::Xm1Adapters;
use crate::sd_host;
use fujimod_emulator_exr2_plan::{Adapter, MemoryChoices, Model, Plan, Resources};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::collections::BTreeMap;

/// A physical guest address.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct Address(pub u64);

#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub struct NodeName(pub String);

/// A bundled FujiMod run configuration whose SoC and body select the
/// processor and memory graph.
#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub struct CatalogConfiguration(pub String);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootRom {
    pub address: Address,
    pub bytes: usize,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Policy {
    /// Stop at exact instruction boundaries.
    Exact,
    /// Let workers overshoot to block boundaries.
    Interactive,
}

impl From<Policy> for fujimod_emulator_cpu::ExecutionPolicy {
    fn from(policy: Policy) -> Self {
        match policy {
            Policy::Exact => Self::Exact,
            Policy::Interactive => Self::Interactive,
        }
    }
}

/// Everything a run needs besides the firmware and boot-ROM bytes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunDocument {
    pub schema: u32,
    pub catalog_configuration: CatalogConfiguration,
    pub execution: RunExecution,
    pub execution_policy: Policy,
    /// DAT offset of the complemented NAND image.
    pub update_flash_offset: usize,
    pub boot_rom: BootRom,
    /// Cold-boot budget before the first route step.
    pub boot_instructions: u64,
    /// A FujiMod EXR II device plan.
    pub devices: Box<RawValue>,
    /// A FujiMod session preparation spec naming the assets below.
    pub scene: Box<RawValue>,
    pub generated_assets: Vec<GeneratedSceneAsset>,
    /// The GPIO node whose pads carry the GPIO keys.
    pub gpio: NodeName,
    pub nand: NodeName,
    pub usb: NodeName,
    /// The logic-level wire of the USB cable's presence.
    pub usb_cable: NodeName,
    pub adc: NodeName,
    pub scanout: NodeName,
    /// The SD0 host adapter node.
    pub sd: NodeName,
    pub firmware: FirmwareFacts,
    /// Where each plan window and RAM fact address comes from.
    pub provenance: Provenance,
}

fn guest(address: Address) -> Result<Guest, String> {
    u32::try_from(address.0).map(Guest).map_err(|_| "RAM fact address exceeds 32 bits".into())
}

impl RunDocument {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let document: Self = serde_json::from_slice(bytes).map_err(|e| format!("run document: {e}"))?;
        if document.schema != 1 {
            return Err("run document schema must be 1".into());
        }
        let firmware = &document.firmware;
        let ram = [
            (RamFact::CurrentMenu, guest(firmware.current_menu)?),
            (RamFact::CalibrationPointer, guest(firmware.calibration_pointer)?),
            (RamFact::ScannerRegistered, guest(firmware.scanner_registered)?),
            (RamFact::KeyTable, guest(firmware.key_table.base)?),
        ];
        let registers: Vec<(&str, Guest)> =
            sd_host::REGISTERS.iter().map(|address| (document.sd.0.as_str(), Guest(*address))).collect();
        document.provenance.validate(&plan_windows(document.devices.get())?, &ram, &registers)?;
        document.sd_host()?;
        Ok(document)
    }

    pub fn sd_host(&self) -> Result<Adapter, String> {
        Plan::parse(self.devices.get().as_bytes())?
            .nodes
            .into_iter()
            .find(|node| node.name == self.sd.0)
            .and_then(|node| match node.model {
                Model::Adapter { adapter } if adapter.implementation == sd_host::IMPLEMENTATION => Some(adapter),
                _ => None,
            })
            .ok_or_else(|| format!("run document node {} is not the {} adapter", self.sd.0, sd_host::IMPLEMENTATION))
    }
}

/// An update container the caller has already validated as X-M1
/// 1.50-compatible; its identity is not compared with a catalog entry.
pub struct ValidatedUpdate(pub Vec<u8>);

pub struct NandBase(Vec<u8>);
impl AsRef<[u8]> for NandBase {fn as_ref(&self)->&[u8]{&self.0}}
impl NandBase {pub fn into_bytes(self)->Vec<u8>{self.0}}

pub fn nand_base(document:&RunDocument,update:&ValidatedUpdate)->Result<NandBase,String>{
    let flash=update.0.get(document.update_flash_offset..).filter(|rest|!rest.is_empty()).ok_or("update flash offset exceeds input")?;
    Ok(NandBase(flash.iter().map(|byte|!byte).collect()))
}

/// Physical extent over which the X-M1 boot ROM decode returns its 16 KiB image repeated every
/// `0x4000`: body reads of `0x80004000-0x80007fff` return the whole image, and 256-byte reads at
/// `0x80008000`, `0x80010000` and `0x80100000` return its first 256 bytes
/// (E-BOOTROM-DISCRIMINATOR-150, E-STRAP-READ-150). The end is the last observed read rounded up
/// to the period; beyond it is unmeasured.
pub const BOOT_ROM_DECODE_BYTES: u64 = 0x10_4000;

/// CP14 identities of cores 0 and 1 in each model a receipt can come from: the catalog's, and the
/// camera's (`E-RAM-PUSH-INSTALL-150`), which `open` configures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreIdentities {
    Catalog,
    Xm1,
}

impl CoreIdentities {
    pub const RECORDED: [Self; 2] = [Self::Catalog, Self::Xm1];

    pub const fn words(self) -> [u32; 2] {
        match self {
            Self::Catalog => [0x4003, 0x5003],
            Self::Xm1 => [0x4003, 0x8003],
        }
    }

    /// The model whose cores read `pair`.
    pub fn of(pair: [u32; 2]) -> Option<Self> {
        Self::RECORDED.into_iter().find(|model| model.words() == pair)
    }

    /// Whether `identity` is core `core`'s identity in a recorded model.
    pub fn recorded(core: usize, identity: u32) -> bool {
        Self::RECORDED.iter().any(|model| model.words().get(core) == Some(&identity))
    }

    /// Kernel domain (0 for A, 1 for B) of a core reading `identity` in a recorded model; stock
    /// code selects A only for `0x4003`.
    pub fn domain(identity: u32) -> Option<usize> {
        (0..2).find(|&core| Self::recorded(core, identity))
    }
}

/// The catalog configuration with the X-M1 boot-ROM decode and core identities applied.
pub fn resolved(document: &RunDocument) -> Result<ResolvedConfiguration, String> {
    let mut resolved = ResolvedConfiguration::with_overlays(&document.catalog_configuration.0, &[])?;
    extend_boot_rom_decode(&mut resolved.reference.soc, document.boot_rom.address.0)?;
    set_core_identities(&mut resolved.reference.soc)?;
    Ok(resolved)
}

pub(crate) fn graph(document: &RunDocument) -> Result<(ResolvedConfiguration, factory::Graph), String> {
    let resolved = resolved(document)?;
    let graph = fujimod_emulator::boot::graph::assemble_with_starts(
        &resolved.reference,
        document.execution.instructions_per_tick,
        None,
        &[],
    )?;
    graph.validate()?;
    Ok((resolved, graph))
}

fn set_core_identities(soc: &mut Soc) -> Result<(), String> {
    let catalog = CoreIdentities::Catalog.words();
    let cores: Vec<_> = soc.processors.iter_mut().filter_map(|core| core.rprofile.as_mut()).collect();
    if cores.len() != 2 || cores.iter().map(|core| core.cp14_identity).ne(catalog) {
        return Err("catalog CP14 identities differ from the two-core catalog model".into());
    }
    for (core, identity) in cores.into_iter().zip(CoreIdentities::Xm1.words()) {
        core.cp14_identity = identity;
    }
    Ok(())
}

/// Widens the catalog's 16 KiB boot-ROM region, its physical-bus map and each core's identity map
/// of it to `BOOT_ROM_DECODE_BYTES`; `open` fills the widened region with the repeated image.
fn extend_boot_rom_decode(soc: &mut Soc, base: u64) -> Result<(), String> {
    let rom = RegionSize::Bytes { bytes: 0x4000 };
    let widened = RegionSize::Bytes { bytes: BOOT_ROM_DECODE_BYTES };
    let mut regions = soc.memory_regions.iter_mut().filter(|region| region.base == base);
    match (regions.next(), regions.next()) {
        (Some(region), None) if region.kind == MemoryKind::Rom && region.size == rom => region.size = widened,
        _ => return Err("catalog boot-ROM region differs from the 16 KiB X-M1 decode base".into()),
    }
    let mut bus = soc.dma_mappings.iter_mut().filter(|mapping| mapping.address == base && mapping.physical == base);
    match (bus.next(), bus.next()) {
        (Some(mapping), None) if mapping.size == rom => mapping.size = widened,
        _ => return Err("catalog lacks one 16 KiB physical-bus boot-ROM map".into()),
    }
    for processor in &soc.processors {
        let mut maps = soc.mappings.iter_mut()
            .filter(|mapping| mapping.core == processor.id && mapping.address == base && mapping.physical == base);
        match (maps.next(), maps.next()) {
            (Some(mapping), None) if mapping.size == rom => mapping.size = widened,
            _ => return Err(format!("catalog lacks one 16 KiB boot-ROM identity map for {}", processor.id)),
        }
    }
    Ok(())
}

/// The processors' memory maps of the session `open` builds.
pub fn memory_maps(document: &RunDocument) -> Result<Vec<factory::MemoryMap>, String> {
    Ok(graph(document)?.1.mappings)
}

pub fn memory_backings(document: &RunDocument) -> Result<Vec<factory::Ram>, String> {
    Ok(graph(document)?.1.ram)
}

/// Construct an unexecuted strict board. Mirrors `fujimod_emulator::boot::open_prepared`
/// with the catalog DAT identity check replaced by the caller's validation.
pub fn open(document: &RunDocument, update: &ValidatedUpdate, boot_rom: &[u8]) -> Result<Session, String> {
    if boot_rom.len() != document.boot_rom.bytes {
        return Err(format!("boot ROM must be {} bytes", document.boot_rom.bytes));
    }
    let RunExecution { mode: RunExecutionMode::Strict, engine, .. } = document.execution;
    let (resolved, graph) = graph(document)?;
    let plan = Plan::parse(document.devices.get().as_bytes())?;
    plan.validate_reset_outputs(&graph.processors.iter().map(|p| p.released).collect::<Vec<_>>())?;
    plan.validate_memory_outputs(
        &graph
            .decoders
            .iter()
            .map(|d| MemoryChoices { initial: d.initial, choices: d.choices.len() })
            .collect::<Vec<_>>(),
    )?;
    let mut assets = BTreeMap::from([("nand_base".to_owned(), nand_base(document, update)?.into_bytes())]);
    for asset in &document.generated_assets {
        if asset.pattern.is_empty() || asset.size > 64 << 20 || !asset.size.is_multiple_of(asset.pattern.len()) {
            return Err(format!("scene asset {} has an invalid extent", asset.id));
        }
        if assets.insert(asset.id.clone(), asset.pattern.repeat(asset.size / asset.pattern.len())).is_some() {
            return Err(format!("scene asset {} repeats", asset.id));
        }
    }
    let spec: preparation::Spec =
        serde_json::from_str(document.scene.get()).map_err(|e| format!("scene: {e}"))?;
    let mut resources = Resources::default();
    resources.inputs = spec.build(&assets)?;
    let built = plan.build(resources, 0, &mut Xm1Adapters)?;
    let engine = match engine {
        RunEngine::Unicorn => factory::Engine::Unicorn,
        RunEngine::Hypervisor => factory::Engine::Hypervisor,
    };
    let bus = Bus::from_graph(Mode::Strict, built.graph)?;
    let mut board = graph.build(factory::Options { engine, translation_cache_bytes: 16 << 20 }, bus, None)?;
    for controller in built.core_interrupts {
        board.configure_core_interrupts(controller.core, controller.device, Box::new(controller.inputs))?;
    }
    if !BOOT_ROM_DECODE_BYTES.is_multiple_of(boot_rom.len() as u64) {
        return Err("boot ROM image does not tile the X-M1 decode extent".into());
    }
    for offset in (0..BOOT_ROM_DECODE_BYTES).step_by(boot_rom.len()) {
        board.write_physical(document.boot_rom.address.0 + offset, boot_rom)?;
    }
    let cores: BTreeMap<&str, usize> =
        resolved.reference.soc.processors.iter().enumerate().map(|(index, p)| (p.id.as_str(), index)).collect();
    for write in &resolved.reference.soc.reset.coprocessor_writes {
        let core = *cores
            .get(write.core.as_str())
            .ok_or_else(|| format!("reset write targets absent core {}", write.core))?;
        let register = Coprocessor {
            cp: write.cp.into(),
            opc1: write.opc1.into(),
            crn: write.crn.into(),
            crm: write.crm.into(),
            opc2: write.opc2.into(),
            secure: write.secure.into(),
            wide: write.wide,
        };
        let cpu = board.cpu(core)?;
        let old = cpu.coprocessor(register, None)?;
        cpu.coprocessor(register, Some(old | write.set_bits))?;
    }
    Ok(Session {
        definition: serde_json::Value::Null,
        board,
        controls: None,
        contact_driver: None,
        resources: built.resources,
        nodes: built.node_ids,
        contacts: built.contacts,
        recording_frames: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(rom: &[u8]) -> Session {
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = RunDocument::parse(include_bytes!("../data/xm1-150.json")).unwrap();
        let update = ValidatedUpdate(vec![0xff; document.update_flash_offset + 2048]);
        open(&document, &update, rom).unwrap()
    }

    #[test]
    fn boot_rom_reads_repeat_every_16_kib_through_the_observed_extent() {
        let rom: Vec<u8> = (0..0x4000u32).map(|index| (index * 7 + index / 251) as u8).collect();
        let session = session(&rom);
        let window = |address: u64| {
            let mut bytes = vec![0; 0x100];
            session.board.read_physical(address, &mut bytes).unwrap();
            bytes
        };
        for base in [0x8000_4000, 0x8000_8000, 0x8001_0000, 0x8010_0000] {
            assert_eq!(window(base), rom[..0x100], "{base:#x}");
        }
        assert_eq!(window(0x8000_7f00), rom[0x3f00..], "footer repeats");
        let end = 0x8000_0000 + BOOT_ROM_DECODE_BYTES;
        for core in 0..2 {
            let mut straddle = [0; 32];
            session.board.read_memory(core, 0x8000_3ff0, &mut straddle).unwrap();
            assert_eq!(straddle[..16], rom[0x3ff0..], "core {core}");
            assert_eq!(straddle[16..], rom[..16], "core {core}");
            let mut last = [0; 4];
            session.board.read_memory(core, end - 4, &mut last).unwrap();
            assert_eq!(last, rom[0x3ffc..], "core {core}");
            assert!(session.board.read_memory(core, end, &mut [0; 4]).is_err(), "core {core}");
        }
        assert!(session.board.read_physical(end, &mut [0; 4]).is_err());
    }

    #[test]
    fn cores_read_the_camera_cp14_identities() {
        let session = session(&[0; 0x4000]);
        for (core, identity) in [(0, 0x4003), (1, 0x8003)] {
            assert_eq!(session.board.cpu(core).unwrap().rprofile_identity(None).unwrap(), identity, "core {core}");
        }
    }

    #[test]
    fn recorded_identities_select_kernel_domains() {
        assert_eq!([0x4003, 0x5003, 0x8003, 0x6003].map(CoreIdentities::domain), [Some(0), Some(1), Some(1), None]);
        assert!(CoreIdentities::recorded(1, 0x5003) && CoreIdentities::recorded(1, 0x8003));
        assert!(!CoreIdentities::recorded(0, 0x8003) && !CoreIdentities::recorded(1, 0x4003));
    }
}
