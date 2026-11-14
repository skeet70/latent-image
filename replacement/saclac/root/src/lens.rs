#![forbid(unsafe_code)]

//! The lens children's ports: lensdc posts through the peer port, lens control's probes, the
//! replies through the shared layer's parameter blocks, the profile-record view and the
//! configuration.

use xm1_saclac_config::channel::ConfiguredChannels;
use xm1_saclac_interface::lens::{LensMode, LensPayload, PositionStep, ProfileSlotReference};
use xm1_saclac_interface::stock::{lens_dc_request, record_handle_word, LENS_PAYLOAD_BYTES};
use xm1_saclac_interface::{CompletionStatus, LensDc, ParameterValue, RecordHandle};
use xm1_saclac_lens_drive::ports::{
    CommandBlocks, Configuration, DriveFourth, DrivePolicy, DriveResults, DriveWords, Faults, LensControl, LensRequest,
    ModeSource, Probe, Probes, Records, Refused, Replies, Reply, ResourceIndex, ResourceSelection,
};
use xm1_saclac_lens_drive::stock::{drive_policy, reply_call};
use xm1_saclac_profile_records::stock::shared::{group_17, group_20, group_38};
use xm1_saclac_profile_records::{Access, MeasurementSelection, Memory, StockEntries, Tag};
use xm1_saclac_shared::commands::{ClientArguments, CommandMemory, LensDriveReply, StoredCommand};
use xm1_saclac_shared::lens::{LensControl as SharedLensControl, LensProbe, ProbeState};
use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments, Channel};
use xm1_saclac_shared::stock::{Machine, Stock};

use crate::services::{Services, NO_HANDLE};
use crate::stock;

/// The configuration byte whose bits select the drive correction (0xf0310c..0xf0313c).
const DRIVE_POLICY: AdjustmentKey = AdjustmentKey(0x88ef);

/// The tag 0x75a960(8, 1) forms for the slot reference (0xf14de4).
const SLOT_TAG: Tag = Tag::from_nibbles(8, 1);

const fn probe(state: ProbeState) -> Probe {
    match state {
        ProbeState::Overridden => Probe::Forced,
        ProbeState::Reported => Probe::Set,
        ProbeState::Clear => Probe::Clear,
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: LensDc> LensControl for Services<M, R, E, P> {
    /// A drive is not posted while 0x76d9a8 reports 1, as 0x76dec8 refuses then (0x76def4..0x76df00).
    fn send(&mut self, request: LensRequest) -> Result<(), Refused> {
        let payload = request.payload();
        if matches!(payload, LensPayload::Drive { .. }) && self.stock.probe(LensProbe::First) == ProbeState::Reported {
            return Err(Refused);
        }
        let mut bytes = [0; LENS_PAYLOAD_BYTES];
        self.peers.request(lens_dc_request(&payload, &mut bytes)).map_err(|_| Refused)
    }

    /// In 0xf14ecc's order: 0x76d9a8, 0x76da38, 0x76dba4.
    fn probes(&mut self) -> Probes {
        let bit1 = probe(self.stock.probe(LensProbe::First));
        let bit0 = probe(self.stock.probe(LensProbe::Second));
        let bit2 = probe(self.stock.probe(LensProbe::Third));
        Probes { bit0, bit1, bit2 }
    }

    fn resource_index(&mut self) -> ResourceIndex { ResourceIndex(self.machine.enter(stock::RESOURCE_INDEX, &[])) }
}

pub(crate) fn reply<M: Machine + Clone>(machine: M, reply: Reply) {
    let call = reply_call(&reply);
    let [status, first, second, third] = call.arguments;
    let status = CompletionStatus(status as i32);
    let mut commands = xm1_saclac_shared::commands::Commands::new(Stock::new(machine.clone()));
    let mut stock = Stock::new(machine.clone());
    let command = match reply {
        Reply::Drive(_) => {
            let reply = LensDriveReply { status, channel: Channel(first as i32), words: [second, third] };
            commands.reply_lens_drive(reply, &ConfiguredChannels(&stock), &mut Stock::new(machine));
            return;
        }
        Reply::Cancel(_) => StoredCommand::Cancel,
        Reply::Stop(_) => StoredCommand::Command10,
        Reply::Step { .. } => StoredCommand::Step,
        Reply::Select { .. } => StoredCommand::Command2c,
        Reply::Release(_) => StoredCommand::Command2d,
        Reply::BringUp { .. } => StoredCommand::LensBringUp,
        Reply::LensStop { .. } => StoredCommand::LensStop,
    };
    let arguments = match call.count {
        1 => ClientArguments::Status(status),
        _ => ClientArguments::Reply { status, words: [first, second, third, 0] },
    };
    commands.reply(command, arguments, &mut stock);
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Replies for Services<M, R, E, P> {
    fn reply(&mut self, reply: Reply) {
        self::reply(self.machine.clone(), reply);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Services<M, R, E, P> {
    fn block(&self, command: StoredCommand, index: usize) -> ParameterValue {
        ParameterValue(self.commands.memory().word(command, index))
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> CommandBlocks for Services<M, R, E, P> {
    fn drive(&mut self) -> DriveWords {
        let word = |index| self.block(StoredCommand::LensDrive, index);
        DriveWords { value: word(1), second: word(2), third: word(3), fourth: DriveFourth::Carried(word(4)) }
    }

    fn set_drive_third(&mut self, value: ParameterValue) { self.commands.set_word(StoredCommand::LensDrive, 3, value.0); }

    fn cancel(&mut self) -> ParameterValue { self.block(StoredCommand::Cancel, 1) }

    fn mode(&mut self, source: ModeSource) -> LensMode {
        let (command, index) = match source {
            ModeSource::Primary => (StoredCommand::Command02, 5),
            ModeSource::Secondary => (StoredCommand::Command03, 4),
            ModeSource::Autofocus => (StoredCommand::Autofocus, 38),
        };
        LensMode(self.block(command, index).0)
    }

    fn step(&mut self) -> PositionStep { PositionStep(self.block(StoredCommand::Step, 1).0 as i16) }

    fn selection(&mut self) -> ParameterValue { self.block(StoredCommand::Command2c, 1) }

    fn bring_up(&mut self) -> [ParameterValue; 5] { [1, 2, 3, 4, 5].map(|index| self.block(StoredCommand::LensBringUp, index)) }

    fn lens_stop(&mut self) -> [ParameterValue; 2] { [1, 2].map(|index| self.block(StoredCommand::LensStop, index)) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Records for Services<M, R, E, P> {
    /// 0x75a7c4(record, 63, 3, tag): all-ones for a record that is no handle or may not derive.
    fn profile_slot(&mut self, record: RecordHandle) -> ProfileSlotReference {
        let derived = xm1_saclac_profile_records::stock::handle(record)
            .and_then(|handle| handle.derive(None, Access::ReadWrite, SLOT_TAG).ok())
            .map_or(u32::MAX, |handle| record_handle_word(xm1_saclac_profile_records::stock::record_handle(handle)));
        ProfileSlotReference(derived)
    }

    fn resource_selection(&mut self, record: RecordHandle) -> ResourceSelection {
        match self.entries.measurement_selection(record) {
            MeasurementSelection::Unselected => ResourceSelection::Zero,
            MeasurementSelection::Record => ResourceSelection::One,
            MeasurementSelection::Other(_) => ResourceSelection::Other,
        }
    }

    fn shared_drive_bit(&mut self) -> bool { self.shared(NO_HANDLE, &group_20::OFFERED_MEASUREMENTS).0 & 2 != 0 }

    /// 0xf14c8c: word 8 of the lens snapshot (0x756840/0x756878), then option 0x26 (0x75a210).
    fn prepare_selection(&mut self, record: RecordHandle) {
        let mut snapshot = self.shared_bytes(record, &group_17::LENS_SNAPSHOT);
        snapshot[32..36].fill(0);
        let _refused = self.records.write_shared(record, &group_17::LENS_SNAPSHOT, snapshot);
        let _refused = self.records.write_shared(record, &group_38::AT_0894, 1);
    }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Configuration for Services<M, R, E, P> {
    fn drive_policy(&mut self) -> DrivePolicy { drive_policy(self.stock.byte(DRIVE_POLICY)) }

    fn adjustment_configured(&mut self) -> bool { self.search.configuration.controller_flag(xm1_saclac_calc::adjust::configuration::ControllerFlag::First) }

    fn select_lookup(&mut self, index: ResourceIndex) { self.machine.enter(stock::SELECT_LOOKUP, &[index.0 as usize]); }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> DriveResults for Services<M, R, E, P> {
    fn live_result(&mut self) -> ParameterValue { ParameterValue(self.machine.read_u32(stock::LIVE_DRIVE_RESULT)) }

    fn s1_report(&mut self) -> ParameterValue { ParameterValue(self.machine.read_u32(stock::S1_DRIVE_REPORT)) }
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P> Faults for Services<M, R, E, P> {
    fn fault(&mut self) { self.machine.enter(stock::FAULT, &[]); }
}
