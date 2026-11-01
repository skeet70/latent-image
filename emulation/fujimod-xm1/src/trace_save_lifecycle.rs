use super::*;

const ROOT: Guest = Guest(0x0157dd3c);
const ACTIVE_SLOT: Guest = Guest(0x015ad53c);
const CATALOGUE: Guest = Guest(0x00e8a338);
const FRAME_STRIDE: u32 = 0x326c;

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CaptureScope {
    #[default]
    Capture,
    SaveLifecycle { program: LifecycleProgram },
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LifecycleProgram {
    Retained,
    Owned {
        image: [Guest; 2],
        save_entry: Guest,
        save_completion: Guest,
        release_completion: Guest,
        permit_entry: Guest,
    },
}

impl CaptureScope {
    pub(super) fn lifecycle(self) -> bool {
        matches!(self, Self::SaveLifecycle { .. })
    }

    pub(super) fn validate(self, sites: &BTreeMap<u32, String>) -> Result<(), String> {
        if let Self::SaveLifecycle { program: LifecycleProgram::Owned {
            image: [start, end], save_entry, save_completion, release_completion, permit_entry,
        } } = self {
            if start.0 % 4 != 0 || end.0 % 4 != 0 || start.0 >= end.0
                || end.0 - start.0 > 0x40000 || end.0 > 0x20000000 {
                return Err("lifecycle program image is not a bounded aligned RAM image".into());
            }
            let entries = [save_entry, save_completion, release_completion, permit_entry];
            if entries.iter().any(|entry| entry.0 % 4 != 0
                || !(start.0..end.0).contains(&entry.0) || !sites.contains_key(&entry.0))
                || entries.iter().enumerate().any(|(index, entry)| entries[..index].contains(entry)) {
                return Err("lifecycle program entries are absent, overlapping or outside the image".into());
            }
        }
        Ok(())
    }

    fn save_entry(self, address: Guest) -> bool {
        address.0 == 0x00e99e8c || address.0 == 0x00e518c0
            || matches!(self, Self::SaveLifecycle { program: LifecycleProgram::Owned { save_entry, .. } }
                if address == save_entry)
    }

    fn observes(self, address: Guest) -> bool {
        matches!(address.0,
            0x00e88d88 | 0x00e88e18 | 0x00e80e10 | 0x00e80e3c | 0x00e80e44 |
            0x00e80e78 | 0x00e80e7c | 0x00e80e84 | 0x00e80ea0 | 0x00e81a24 |
            0x00e4e830 | 0x00e4eca4 | 0x00e87d80 | 0x00e87dc0 | 0x00e87dc4 |
            0x00e87dd0 | 0x00e99e8c | 0x00e99e90 | 0x00e518c0 | 0x00e4e5dc |
            0x00e4e5a8 | 0x00e4e618 | 0x00e4e63c | 0x00e4e5d4 | 0x00e6a754 |
            0x00e6bb90 | 0x00e7e95c | 0x00e77168 | 0x011d5600 | 0x011d5688 | 0x011d5714)
            || matches!(self, Self::SaveLifecycle { program: LifecycleProgram::Owned {
                save_entry, save_completion, release_completion, permit_entry, ..
            } } if [save_entry, save_completion, release_completion, permit_entry].contains(&address))
    }
}

#[derive(Clone, Copy, Serialize)]
pub struct LifecycleContext {
    r4: u32,
    r5: u32,
    r11: u32,
    tcm: [u32; 2],
}

impl LifecycleContext {
    fn read(session: &Session, core: MemoryCore, tcm: &TcmViews) -> Result<Self, String> {
        Ok(Self {
            r4: register(session, core.0, arm::Register::R(4))?,
            r5: register(session, core.0, arm::Register::R(5))?,
            r11: register(session, core.0, arm::Register::R(11))?,
            tcm: tcm.registers,
        })
    }
}

#[derive(Clone, Copy)]
struct FrameSlot(u32);

impl FrameSlot {
    fn parse(word: u32) -> Option<Self> {
        (word < 201).then_some(Self(word))
    }
}

#[derive(Clone, Copy)]
struct FrameCount(u32);

#[derive(Clone, Copy)]
struct FrameRegion {
    base: Guest,
    count: FrameCount,
}

impl FrameRegion {
    fn from_root(root: Guest) -> Option<Self> {
        let count = match root.0 {
            0x1f9dbb60 => 87,
            0x5f9fb7e0 => 96,
            0x04988600 | 0x49c4f480 | 0x44988600 => 1,
            _ => return None,
        };
        Some(Self { base: root, count: FrameCount(count) })
    }

    fn address(self, slot: FrameSlot) -> Option<Guest> {
        if slot.0 >= self.count.0 { return None; }
        self.base.0.checked_add(slot.0.checked_mul(FRAME_STRIDE)?).map(Guest)
    }

    fn slot(self, address: Guest) -> Option<FrameSlot> {
        let offset = address.0.checked_sub(self.base.0)?;
        if offset % FRAME_STRIDE != 0 { return None; }
        let slot = FrameSlot::parse(offset / FRAME_STRIDE)?;
        self.address(slot).filter(|expected| *expected == address).map(|_| slot)
    }

    fn descriptor(self, address: Guest) -> Option<StorageFrame> {
        let offset = address.0.checked_sub(self.base.0)?.checked_sub(0x27d0)?;
        let bank = match offset % FRAME_STRIDE {
            0 => DescriptorBank::Zero,
            0x150 => DescriptorBank::One,
            0x2a0 => DescriptorBank::Two,
            _ => return None,
        };
        let frame = self.address(FrameSlot::parse(offset / FRAME_STRIDE)?)?;
        Some(StorageFrame { frame, bank })
    }
}

#[derive(Clone, Copy)]
enum FrameSource {
    Slot(Option<FrameSlot>),
    Pointer(Guest),
    Paired { slot: FrameSlot, address: Guest },
    Unavailable,
}

impl FrameSource {
    fn from_slot_word(word: u32) -> Self {
        FrameSlot::parse(word).map(|slot| Self::Slot(Some(slot))).unwrap_or(Self::Unavailable)
    }

    fn from_pair_words(slot: u32, address: u32) -> Self {
        FrameSlot::parse(slot).map(|slot| Self::Paired { slot, address: Guest(address) }).unwrap_or(Self::Unavailable)
    }
}

#[derive(Clone, Copy)]
enum DescriptorBank { Zero, One, Two }

impl DescriptorBank {
    fn from_kind(kind: u32) -> Self {
        match kind { 1 => Self::One, 2..=4 => Self::Two, _ => Self::Zero }
    }

    fn offset(self) -> u32 {
        match self { Self::Zero => 0, Self::One => 0x150, Self::Two => 0x2a0 }
    }
}

struct StorageFrame { frame: Guest, bank: DescriptorBank }

#[derive(Clone, Copy)]
enum FrameProfile {
    Population,
    Save { bank: DescriptorBank, status: Option<Guest> },
}

fn globals(ram: &mut CaptureRam, session: &Session, views: &DiagnosticViews<'_>) {
    for (address, length) in [
        (ROOT, 4), (ACTIVE_SLOT, 4), (Guest(0x0154d970), 4),
        (Guest(0x0155ccb8), 8), (Guest(0x014ddc1c), 12),
        (Guest(0x014ddbd8), 4), (Guest(0x01581f50), 28),
    ] {
        ram.view(session, views, address, length);
    }
}

fn frame_fields(ram: &mut CaptureRam, session: &Session, views: &DiagnosticViews<'_>, frame: Guest, profile: FrameProfile) {
    let mut read = |offset: u32, length: usize| {
        if let Some(address) = frame.0.checked_add(offset).map(Guest) {
            ram.view(session, views, address, length);
        } else {
            ram.incomplete = true;
        }
    };
    read(0x578, 4);
    read(0x5f0, 24);
    read(0x275c, 4);
    read(0x3260, 12);
    match profile {
        FrameProfile::Population => {
            read(0x27c8, 8);
            read(0x2f64, 0x50);
            read(0x78, 4);
            read(0x4a8, 4);
            for bank in [DescriptorBank::Zero, DescriptorBank::One, DescriptorBank::Two] {
                read(0x27d4 + bank.offset(), 4);
                read(0x2918 + bank.offset(), 8);
            }
        }
        FrameProfile::Save { bank, status } => {
            read(0x2f64, 0x30);
            read(0x27d0 + bank.offset(), 0x150);
            if let Some(status) = status { ram.view(session, views, status, 4); }
        }
    }
}

fn active_source(ram: &CaptureRam) -> FrameSource {
    match ram.word(ACTIVE_SLOT) {
        Some(u32::MAX) => FrameSource::Slot(None),
        Some(slot) => FrameSource::from_slot_word(slot),
        None => FrameSource::Unavailable,
    }
}

fn paired_source(ram: &CaptureRam, address: Guest) -> FrameSource {
    match (ram.word(address), address.0.checked_add(4).and_then(|next| ram.word(Guest(next)))) {
        (Some(frame), Some(slot)) => FrameSource::from_pair_words(slot, frame),
        _ => FrameSource::Unavailable,
    }
}

fn sample_frame(ram: &mut CaptureRam, session: &Session, views: &DiagnosticViews<'_>, source: FrameSource, profile: FrameProfile) {
    if matches!(source, FrameSource::Slot(None)) { return; }
    let Some(region) = ram.word(ROOT).map(Guest).and_then(FrameRegion::from_root) else {
        ram.incomplete = true;
        return;
    };
    let frame = match source {
        FrameSource::Slot(Some(slot)) => region.address(slot),
        FrameSource::Pointer(address) => region.slot(address).and_then(|slot| region.address(slot)),
        FrameSource::Paired { slot, address } => region.address(slot).filter(|expected| *expected == address),
        FrameSource::Slot(None) | FrameSource::Unavailable => None,
    };
    if let Some(frame) = frame {
        frame_fields(ram, session, views, frame, profile);
    } else {
        ram.incomplete = true;
    }
}

pub(super) fn event(
    scope: CaptureScope, session: &Session, memory: Option<&CaptureMemory>, core: MemoryCore,
    address: Guest, registers: &[u32; 4],
) -> Result<(CaptureRam, LifecycleContext), String> {
    let tcm = TcmViews::read(session, core)?;
    let context = LifecycleContext::read(session, core, &tcm)?;
    let views = DiagnosticViews { memory, core, tcm: Some(tcm) };
    let mut ram = CaptureRam::default();
    globals(&mut ram, session, &views);
    if !scope.observes(address) {
        return Ok((ram, context));
    }
    ram.view(session, &views, address, 4);
    if matches!(address.0, 0x011d5600 | 0x011d5688) {
        let storage = ram.word(ROOT).map(Guest).and_then(FrameRegion::from_root)
            .and_then(|region| region.descriptor(Guest(registers[1])));
        if let Some(storage) = storage {
            sample_frame(&mut ram, session, &views, FrameSource::Pointer(storage.frame),
                FrameProfile::Save { bank: storage.bank, status: None });
        } else {
            ram.incomplete = true;
        }
        return Ok((ram, context));
    }
    if address.0 == 0x00e99e90 {
        if let Some(status) = context.r11.checked_sub(0x20).map(Guest) {
            ram.view(session, &views, status, 4);
        } else {
            ram.incomplete = true;
        }
    }
    let profile = if scope.save_entry(address) {
        let bank = DescriptorBank::from_kind(registers[1]);
        FrameProfile::Save { bank, status: (registers[2] != 0).then_some(Guest(registers[2])) }
    } else {
        FrameProfile::Population
    };
    let source = match address.0 {
        0x00e88e18 if registers[0] == u32::MAX => FrameSource::Slot(None),
        0x00e87d80 if registers[0] >= 201 => FrameSource::Slot(None),
        0x00e88e18 | 0x00e80e3c | 0x00e87d80 => FrameSource::from_slot_word(registers[0]),
        0x00e80e44 | 0x00e80e78 | 0x00e4e830 | 0x00e99e8c | 0x00e518c0 => FrameSource::Pointer(Guest(registers[0])),
        0x00e4eca4 => FrameSource::Pointer(Guest(context.r5)),
        0x00e87dc0 | 0x00e87dc4 => FrameSource::Pointer(Guest(context.r4)),
        0x00e80e7c => {
            ram.view(session, &views, Guest(context.r4), 4);
            ram.view(session, &views, Guest(context.r5), 4);
            match (ram.word(Guest(context.r4)), ram.word(Guest(context.r5))) {
                (Some(slot), Some(frame)) => FrameSource::from_pair_words(slot, frame),
                _ => FrameSource::Unavailable,
            }
        }
        0x00e80ea0 | 0x00e81a24 => {
            if let Some(locals) = context.r11.checked_sub(0x24).map(Guest) {
                ram.view(session, &views, locals, 8);
                paired_source(&ram, locals)
            } else {
                FrameSource::Unavailable
            }
        }
        _ if scope.save_entry(address) => FrameSource::Pointer(Guest(registers[0])),
        _ => active_source(&ram),
    };
    sample_frame(&mut ram, session, &views, source, profile);
    Ok((ram, context))
}

pub(super) fn snapshot(
    session: &Session, memory: Option<&CaptureMemory>,
) -> Result<(CaptureRam, LifecycleContext), String> {
    let core = MemoryCore(0);
    let tcm = TcmViews::read(session, core)?;
    let context = LifecycleContext::read(session, core, &tcm)?;
    let views = DiagnosticViews { memory, core, tcm: Some(tcm) };
    let mut ram = CaptureRam::default();
    globals(&mut ram, session, &views);
    ram.view(session, &views, CATALOGUE, 0xd0);
    let source = active_source(&ram);
    sample_frame(&mut ram, session, &views, source, FrameProfile::Population);
    Ok((ram, context))
}
