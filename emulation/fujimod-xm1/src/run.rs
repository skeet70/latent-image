//! The X-M1 run document and the strict FujiMod session it opens.

use crate::guest::FirmwareFacts;
use crate::provenance::{Guest, Provenance, RamFact, plan_windows};
use fujimod_catalog::configuration::{
    GeneratedSceneAsset, ResolvedConfiguration, RunEngine, RunExecution, RunExecutionMode,
};
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

pub(crate) fn graph(document: &RunDocument) -> Result<(ResolvedConfiguration, factory::Graph), String> {
    let resolved = ResolvedConfiguration::with_overlays(&document.catalog_configuration.0, &[])?;
    let graph = fujimod_emulator::boot::graph::assemble_with_starts(
        &resolved.reference,
        document.execution.instructions_per_tick,
        None,
        &[],
    )?;
    graph.validate()?;
    Ok((resolved, graph))
}

/// The processors' memory maps of the session `open` builds.
pub fn memory_maps(document: &RunDocument) -> Result<Vec<factory::MemoryMap>, String> {
    Ok(graph(document)?.1.mappings)
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
    let flash = update
        .0
        .get(document.update_flash_offset..)
        .filter(|rest| !rest.is_empty())
        .ok_or("update flash offset exceeds input")?;
    let mut assets = BTreeMap::from([("nand_base".to_owned(), flash.iter().map(|b| !b).collect::<Vec<u8>>())]);
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
    board.write_physical(document.boot_rom.address.0, boot_rom)?;
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
