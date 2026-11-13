use crate::ports::{
    CommandBlocks, Configuration, DriveFourth, DrivePolicy, DriveResults, DriveWords, Faults, LensControl, LensRequest,
    ModeSource, Probe, Probes, Records, Refused, Replies, Reply, ResourceIndex, ResourceSelection,
};
use std::vec::Vec;
use xm1_saclac_interface::lens::{LensMode, LensPayload, PositionStep, ProfileSlotReference};
use xm1_saclac_interface::{stock, Completed, Completion, ParameterValue, RecordHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Seen {
    Send(LensPayload),
    Reply(Reply),
    SetDriveThird(ParameterValue),
    PrepareSelection(RecordHandle),
    SelectLookup(ResourceIndex),
    Fault,
}

pub struct Fake {
    pub seen: Vec<Seen>,
    pub effects_require_contract: bool,
    pub refuse: bool,
    pub probes: Probes,
    pub drive: DriveWords,
    pub cancel: ParameterValue,
    pub step: PositionStep,
    pub selection: ParameterValue,
    pub bring_up: [ParameterValue; 5],
    pub lens_stop: [ParameterValue; 2],
    pub slot: ProfileSlotReference,
    pub resource: ResourceSelection,
    pub shared_bit: bool,
    pub policy: DrivePolicy,
    pub configured: bool,
    pub index: ResourceIndex,
    pub live_result: ParameterValue,
    pub s1_report: ParameterValue,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            seen: Vec::new(),
            effects_require_contract: true,
            refuse: false,
            probes: Probes { bit0: Probe::Clear, bit1: Probe::Clear, bit2: Probe::Clear },
            drive: DriveWords {
                value: ParameterValue(11),
                second: ParameterValue(12),
                third: ParameterValue(13),
                fourth: DriveFourth::Carried(ParameterValue(14)),
            },
            cancel: ParameterValue(0x77),
            step: PositionStep(-3),
            selection: ParameterValue(0x2c2c),
            bring_up: [1, 2, 3, 4, 5].map(ParameterValue),
            lens_stop: [6, 7].map(ParameterValue),
            slot: ProfileSlotReference(0x8100_0042),
            resource: ResourceSelection::Other,
            shared_bit: false,
            policy: DrivePolicy { always: false, resource_one: false, resource_zero: false },
            configured: false,
            index: ResourceIndex(2),
            live_result: ParameterValue(0x954),
            s1_report: ParameterValue(0x750c),
        }
    }
}

impl Fake {
    pub fn take(&mut self) -> Vec<Seen> { core::mem::take(&mut self.seen) }
}

pub const PRIMARY_MODE: LensMode = LensMode(0x20);
pub const SECONDARY_MODE: LensMode = LensMode(0x30);
pub const AUTOFOCUS_MODE: LensMode = LensMode(0xd0);

pub fn record() -> RecordHandle { stock::record_handle_from_a2(0x0169_e580) }

pub fn completed(completion: Completion, bytes: &[u8]) -> Completed<'_> {
    Completed { completion, parameters: stock::parameters(bytes) }
}

impl LensControl for Fake {
    fn send(&mut self, request: LensRequest) -> Result<(), Refused> {
        self.effects_require_contract &= request.effect().requires_lens_safety_contract();
        self.seen.push(Seen::Send(request.payload()));
        if self.refuse { Err(Refused) } else { Ok(()) }
    }
    fn probes(&mut self) -> Probes { self.probes }
    fn resource_index(&mut self) -> ResourceIndex { self.index }
}

impl Replies for Fake {
    fn reply(&mut self, reply: Reply) { self.seen.push(Seen::Reply(reply)); }
}

impl CommandBlocks for Fake {
    fn drive(&mut self) -> DriveWords { self.drive }
    fn set_drive_third(&mut self, value: ParameterValue) {
        self.drive.third = value;
        self.seen.push(Seen::SetDriveThird(value));
    }
    fn cancel(&mut self) -> ParameterValue { self.cancel }
    fn mode(&mut self, source: ModeSource) -> LensMode {
        match source {
            ModeSource::Primary => PRIMARY_MODE,
            ModeSource::Secondary => SECONDARY_MODE,
            ModeSource::Autofocus => AUTOFOCUS_MODE,
        }
    }
    fn step(&mut self) -> PositionStep { self.step }
    fn selection(&mut self) -> ParameterValue { self.selection }
    fn bring_up(&mut self) -> [ParameterValue; 5] { self.bring_up }
    fn lens_stop(&mut self) -> [ParameterValue; 2] { self.lens_stop }
}

impl Records for Fake {
    fn profile_slot(&mut self, _: RecordHandle) -> ProfileSlotReference { self.slot }
    fn resource_selection(&mut self, _: RecordHandle) -> ResourceSelection { self.resource }
    fn shared_drive_bit(&mut self) -> bool { self.shared_bit }
    fn prepare_selection(&mut self, record: RecordHandle) { self.seen.push(Seen::PrepareSelection(record)); }
}

impl Configuration for Fake {
    fn drive_policy(&mut self) -> DrivePolicy { self.policy }
    fn adjustment_configured(&mut self) -> bool { self.configured }
    fn select_lookup(&mut self, index: ResourceIndex) { self.seen.push(Seen::SelectLookup(index)); }
}

impl DriveResults for Fake {
    fn live_result(&mut self) -> ParameterValue { self.live_result }
    fn s1_report(&mut self) -> ParameterValue { self.s1_report }
}

impl Faults for Fake {
    fn fault(&mut self) { self.seen.push(Seen::Fault); }
}
