#![forbid(unsafe_code)]

use crate::lanes::{Lane, Lanes, Selection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordSelection(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchModeFlags(pub u32);

/// The profile byte 0x74e5c0 returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSelectionByte(pub u8);

/// X-M1 1.50: 0xf32e88..0xf32ef8 and 0xf32f80..0xf33008.
pub const fn selection(word: RecordSelection) -> Selection {
    Selection {
        acquisition: word.0 & 1 != 0,
        search: word.0 & 2 != 0,
        auxiliary: word.0 & 4 != 0,
        image: word.0 & 8 != 0,
        source: word.0 & 32 != 0,
    }
}

/// `None` when the whole word is zero, which drains bookkeeping at 0xf3305c..0xf3306c.
pub const fn availability(word: RecordSelection) -> Option<Selection> {
    if word.0 == 0 { None } else { Some(selection(word)) }
}

/// X-M1 1.50: 0xf32f60..0xf32f7c gates search and image with property 0x3e0009.
pub const fn search_allowed(property: u32) -> bool { property != 1 }

/// X-M1 1.50: 0xf330e4..0xf33138 tests bits 1, 2 and 3 of property 0x88ef in that order.
pub const fn alternate_search_rule(flags: SearchModeFlags) -> crate::record::AlternateSearch {
    use crate::record::AlternateSearch;
    if flags.0 & 2 != 0 { AlternateSearch::WithRecordSelection }
    else if flags.0 & 4 != 0 { AlternateSearch::WithoutSelection }
    else if flags.0 & 8 != 0 { AlternateSearch::Always }
    else { AlternateSearch::Never }
}

/// X-M1 1.50: profile byte 0x30 (0x74e5c0 → 0x75b054); its readers in the round
/// compare it with 0 and 1 only.
pub const fn profile_selection(byte: ProfileSelectionByte) -> crate::profile::ProfileSelection {
    use crate::profile::ProfileSelection;
    match byte.0 { 0 => ProfileSelection::Unselected, 1 => ProfileSelection::Record, other => ProfileSelection::Other(other) }
}

/// X-M1 1.50: 0xf04840..0xf0499c and 0xf32530..0xf325cc use this lane encoding.
pub fn lane_flags(lanes: Lanes) -> u8 {
    [(Lane::Acquisition, 1), (Lane::Search, 2), (Lane::Auxiliary, 4), (Lane::Image, 8),
     (Lane::Window, 32), (Lane::Source, 64), (Lane::Bookkeeping, 16)]
        .into_iter().fold(0, |flags, (lane, bit)| if lanes.contains(lane) { flags | bit } else { flags })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulingProperty(u32);
impl SchedulingProperty { pub const fn word(self) -> u32 { self.0 } }

/// X-M1 1.50: 0xfa72d0..0xfa72e8 reads the configuration bytes keyed at 0xfab110
/// (seven per work, by mode word); mode words 2 and 6 are never stored at 0x16d877c.
pub const fn cadence_property(work: crate::planner::Work, mode: crate::planner::Mode) -> Option<SchedulingProperty> {
    use crate::planner::{Mode, Work};
    let base = match work {
        Work::PrimaryAcquisition => 0x4bc0,
        Work::Search => 0x4bc6,
        Work::Image => 0x4bcc,
        Work::Auxiliary | Work::Source => 0x4bd2,
        Work::SecondaryAcquisition | Work::Window => return None,
    };
    let offset = match mode {
        Mode::Default => 5, Mode::RecordSelected => 0,
        Mode::ProfileSelected => 2, Mode::AboveThreshold => 3, Mode::BelowThreshold => 4,
    };
    Some(SchedulingProperty(base + offset))
}

/// X-M1 1.50: 0xfa9a20 reads this configuration byte as the frame-rate threshold.
pub const FRAME_RATE_THRESHOLD: SchedulingProperty = SchedulingProperty(0x4bd8);

pub const fn cadence(word: u32) -> crate::planner::Cadence {
    crate::planner::Cadence { first: word & 1 != 0, second: word & 2 != 0, third: word & 4 != 0, fourth: word & 8 != 0 }
}

fn word(parameters: xm1_saclac_interface::Parameters<'_>, offset: usize) -> u32 {
    parameters.bytes().get(offset..offset + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// A parameter change (command 0x17).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterChange {
    pub client: crate::round::ParameterClient,
    pub requested: Selection,
    pub offered: Selection,
}

/// X-M1 1.50: 0xf3300c reads words 1 and 2 of the 0x17 parameter block (0xef9dc0
/// returns 0x169d22c); 0xefb4b8 enters word 0 with the status.
pub fn parameter_change(parameters: xm1_saclac_interface::Parameters<'_>) -> ParameterChange {
    ParameterChange {
        client: crate::round::ParameterClient(xm1_saclac_interface::stock::optional_callback(word(parameters, 0))),
        requested: selection(RecordSelection(word(parameters, 4))),
        offered: selection(RecordSelection(word(parameters, 8))),
    }
}

/// X-M1 1.50: the image row function caches the channel from word 3 of command
/// 0x14 (0xefe2fc) and word 2 of command 0x2b (0xefe3ec); the replies at
/// 0xefb328 and 0xefb438 take the callback from word 0 and the value from word 4
/// of 0x14 and word 3 of 0x2b. imgdc receives words 1 and 2 of 0x14 (0xf39fa4)
/// and the signed halfword at byte 4 of 0x2b (0xf3a02c).
pub fn image_command(kind: crate::image::ImageRequest, parameters: xm1_saclac_interface::Parameters<'_>) -> crate::image::ImageCommand {
    use crate::image::{ImageClient, ImageCommand, ImageRequest, ImageWork, ReplyValue};
    use xm1_saclac_interface::round_image::{ImageArguments, ImageChannel, SnapshotArgument};
    let (channel, value, work) = match kind {
        ImageRequest::Image => (12, 16, ImageWork::Image(ImageArguments([word(parameters, 4), word(parameters, 8)]))),
        ImageRequest::Snapshot => (8, 12, ImageWork::Snapshot(SnapshotArgument(word(parameters, 4) as i16))),
    };
    ImageCommand {
        client: ImageClient {
            callback: xm1_saclac_interface::stock::optional_callback(word(parameters, 0)),
            channel: ImageChannel(word(parameters, channel)),
            value: ReplyValue(word(parameters, value)),
        },
        work,
    }
}

/// X-M1 1.50: 0xefb3a8 and 0xefb3f0 enter word 0 of command 0x15 or 0x16.
pub fn interruption_callback(parameters: xm1_saclac_interface::Parameters<'_>) -> Option<xm1_saclac_interface::ClientCallback> {
    xm1_saclac_interface::stock::optional_callback(word(parameters, 0))
}

/// X-M1 1.50: 0xf3a084 sends word 1 of command 0x15 with imgdc 0x3f32.
pub fn cancel_argument(parameters: xm1_saclac_interface::Parameters<'_>) -> xm1_saclac_interface::round_image::CancelArgument {
    xm1_saclac_interface::round_image::CancelArgument(word(parameters, 4))
}

/// X-M1 1.50: the halfword 0x69fec8 reads at this key; 0xf39f88, 0xf3a068 and 0xf3a0a0 test
/// bit 8 before logging an image, cancel or stop request, 0xf3a010 bit 9 before a snapshot.
pub const IMAGE_LOGGING: SchedulingProperty = SchedulingProperty(232);

pub const fn image_logging(word: u16) -> crate::image::ImageLogging {
    crate::image::ImageLogging { image: word & 0x100 != 0, snapshot: word & 0x200 != 0 }
}

/// X-M1 1.50: the lane logs through 0x64744c with this word first and the entry's text second.
pub const IMAGE_LOG_SOURCE: u32 = 0x00ff_b104;

/// X-M1 1.50: the text 0xf39f48, 0xf39fd0, 0xf3a050 and 0xf3a090 log.
pub const fn image_log_text(entry: crate::image::ImageLog) -> u32 {
    use crate::image::ImageLog;
    match entry { ImageLog::Image => 0x00f3_a268, ImageLog::Snapshot => 0x00f3_a278, ImageLog::Cancel => 0x00f3_a288, ImageLog::Stop => 0x00f3_a29c }
}

/// X-M1 1.50: 0xefb37c..0xefb38c and 0xefb48c..0xefb49c replace the value of a
/// reply whose channel 0x74a214 rejects with 0x74a740(128, 0).
pub const NO_CHANNEL_VALUE: crate::image::ReplyValue = crate::image::ReplyValue(128 << 16);

/// X-M1 1.50: 0xf3a160..0xf3a1b0 and 0xf3a1b8..0xf3a208 pass the channel, the
/// value and the lens bits (2 for 0x76d9a8, 1 for 0x76da38, 4 for 0x76dba4) as
/// the callback's arguments after the status.
pub fn image_reply_values(channel: xm1_saclac_interface::round_image::ImageChannel, value: crate::image::ReplyValue, lens: crate::image::LensReport) -> [u8; 12] {
    let bits = u32::from(lens.first) * 2 + u32::from(lens.second) + u32::from(lens.third) * 4;
    let mut values = [0; 12];
    for (slot, word) in values.chunks_exact_mut(4).zip([channel.0, value.0, bits]) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    values
}

/// X-M1 1.50: the raw profile byte 0x74e5c0 returns.
pub const fn profile_selection_byte(selection: crate::profile::ProfileSelection) -> u8 {
    use crate::profile::ProfileSelection;
    match selection { ProfileSelection::Unselected => 0, ProfileSelection::Record => 1, ProfileSelection::Other(byte) => byte }
}

/// The window request as calc 0x3c0c carries it; `saclac_interface::stock::calc_payload` lays it out.
pub fn window_payload(request: crate::window::WindowRequest, callback: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::calc::CalcPayload {
    use xm1_saclac_interface::calc::{CalcPayload, WindowRecord, WindowRequest, WindowSample};
    let snapshot = request.snapshot;
    CalcPayload::Window {
        request: WindowRequest {
            selection: profile_selection_byte(request.selection),
            mode: request.mode.0,
            selected: request.selected,
            sample: WindowSample {
                mode: snapshot.timing.0,
                flags: snapshot.flags,
                exposure: snapshot.exposure[0].0,
                level: snapshot.levels[0],
                adjustment: snapshot.adjustment.0,
            },
            level: request.level,
            reference: request.reference,
            window: WindowRecord {
                first: request.record.first.0,
                second: request.record.second.0,
                status: request.record.status.0,
                halfword: request.record.halfword.0,
            },
            refresh: request.refresh,
            adjustment: [request.adjustment.controllers.0, request.adjustment.search.0],
        },
        callback,
    }
}

/// The auxiliary request as calc1 0x3d02 carries it. X-M1 1.50: 0xf37ad8..0xf37b10 store the
/// adjustment word (0xfa7928) and profile words 0x74f1ac, 0x74f9a4, 0x74fcec in that order;
/// `stock::calc1_payload` lays out the rest.
pub fn auxiliary_payload(request: crate::auxiliary::AuxiliaryRequest, callback: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::calc::Calc1Payload {
    use xm1_saclac_interface::calc::{AuxiliaryRequest, AuxiliarySample, Calc1Payload};
    let snapshot = request.snapshot;
    let profile = request.profile.profile;
    let words = |pair: [crate::pipeline::PipelineWord; 2]| pair.map(|word| word.0);
    Calc1Payload::Auxiliary {
        request: AuxiliaryRequest {
            complete: request.complete,
            pass: request.pass,
            selection: profile_selection_byte(request.selection),
            profile: [profile.adjustment, profile.words[3], profile.words[2], profile.words[0]].map(|word| word.0),
            sample: AuxiliarySample {
                mode: snapshot.timing.0,
                gain_step: snapshot.gain_step.0,
                exposure: words(snapshot.exposure),
                level: snapshot.levels[0],
                offset_level: snapshot.offset_level,
                adjustment: snapshot.adjustment.0,
                size: words(snapshot.size),
                origin: words(snapshot.origin),
                extent: words(snapshot.extent),
            },
            reference: request.reference_level,
            rate: request.rate,
            level_class: request.level_class.0,
            window_status: request.window.status.0,
            window_halfword: request.window.halfword.0,
            resource_level: request.resource_level as u16,
            word: request.profile.word.0,
            halfword: request.profile.halfword.0,
        },
        callback,
    }
}

/// The source request as calc 0x3c11 carries it. X-M1 1.50: 0xf386e4..0xf38704 store
/// 0xf4f16c's word, 0x74f124's word, its decade (0xef74e4) and 0x74f5ec's halfword.
pub fn source_payload(request: crate::source::SourceRequest, callback: xm1_saclac_interface::stock::StockCallbackWord) -> xm1_saclac_interface::calc::CalcPayload {
    use xm1_saclac_interface::calc::{CalcPayload, SourceRequest};
    let snapshot = request.snapshot;
    let profile = request.profile;
    CalcPayload::Source {
        request: SourceRequest {
            profile: profile.words.map(|word| word.0),
            decade: profile.halfwords[0].0,
            profile_halfword: profile.halfwords[1].0,
            level: request.level,
            reference: snapshot.levels[1],
            size: snapshot.size.map(|word| word.0),
            exposure: snapshot.exposure[1].0,
            mode: snapshot.timing.0,
            selection: profile_selection_byte(request.selection),
            extension: request.extension.0,
        },
        callback,
    }
}

/// X-M1 1.50: 0xf38668 posts completion 0x9a with the six bytes calc returns; 0xf38e20..0xf38e2c
/// keeps the first as the strength.
pub fn source_strength(parameters: xm1_saclac_interface::Parameters<'_>) -> crate::source::SourceStrength {
    crate::source::SourceStrength(parameters.bytes().first().copied().unwrap_or(0))
}

/// X-M1 1.50: 0xff6b4c copies 0x5c4 bytes of pipeline state to 0x16f496c + slot·0x5c4,
/// which 0xfea7ac returns. Offsets from the window (0xf3a5a4..0xf3a614), history
/// (0xf32680..0xf32874), auxiliary request (0xf37b34..0xf37bf8) and acquisition request
/// (0xf35888..0xf35938) builders.
pub const SNAPSHOT_BYTES: usize = 0x5c4;

pub fn pipeline_snapshot(bytes: &[u8; SNAPSHOT_BYTES]) -> crate::pipeline::PipelineSnapshot {
    use crate::pipeline::{GainStep, PipelineSnapshot, PipelineWord, SnapshotStatus, TimingWord};
    let word = |offset: usize| u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
    let half = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    PipelineSnapshot {
        status: SnapshotStatus(word(0)),
        timing: TimingWord(word(0xec)),
        flags: bytes[0xf2],
        code: half(0xf0),
        reference: half(0x108),
        gain_step: GainStep(word(0xf4)),
        exposure: [PipelineWord(word(0xf8)), PipelineWord(word(0x110))],
        levels: [half(0xfc), half(0xfe)],
        offset_level: half(0x100),
        adjustment: PipelineWord(word(0x104)),
        size: [PipelineWord(word(0xc)), PipelineWord(word(0x10))],
        origin: [PipelineWord(word(0x14)), PipelineWord(word(0x18))],
        extent: [PipelineWord(word(0xe4)), PipelineWord(word(0xe8))],
        retained: [4, 8, 0xdc, 0xe0, 0x34, 0x38].map(|offset| PipelineWord(word(offset))),
    }
}

/// X-M1 1.50: 0xfeaadc copies the capture's S1 sample, 0x6c bytes (0xf2431c, 0xf24840, 0xf24dd0).
pub const S1_SAMPLE_BYTES: usize = 0x6c;

/// X-M1 1.50: offsets from the S1 request builders 0xf243cc..0xf244ac, 0xf24854..0xf24874 and
/// 0xf24e1c..0xf24e3c. The sample carries no gain step or adjustment.
pub fn s1_sample(bytes: &[u8; S1_SAMPLE_BYTES]) -> crate::pipeline::PipelineSnapshot {
    use crate::pipeline::{PipelineSnapshot, PipelineWord, SnapshotStatus, TimingWord};
    let word = |offset: usize| u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
    let half = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    PipelineSnapshot {
        status: SnapshotStatus(word(0x58)),
        timing: TimingWord(word(0x18)),
        flags: bytes[0x5c],
        code: half(0x4e),
        reference: half(0x26),
        exposure: [PipelineWord(word(0x1c)), PipelineWord(word(0x28))],
        levels: [half(0x20), half(0x22)],
        offset_level: half(0x24),
        size: [PipelineWord(word(0)), PipelineWord(word(4))],
        extent: [PipelineWord(word(8)), PipelineWord(word(0xc))],
        origin: [PipelineWord(word(0x10)), PipelineWord(word(0x14))],
        ..PipelineSnapshot::EMPTY
    }
}

use crate::acquisition::ports::StatisticsBuffer;

/// X-M1 1.50: the buffer a started acquisition names for all four statistics until calc1
/// returns its own (0xf33878..0xf3388c).
pub const UNMEASURED_STATISTICS: StatisticsBuffer = StatisticsBuffer(0x5ffc_0340);
/// X-M1 1.50: the buffers the release 0xf34c98 fills and names in the acquisition block: the
/// classes (0xf34ef4), the first of the four offset buffers (0xf35084), the extra statistics
/// moved from 0x5ffc6340 (0xf350e8) and the copy, named on every release (0xf3510c..0xf35114)
/// and filled under configuration 0x4a07 with 0x1150 bytes from 0x5ffec1b4, the buffers a
/// 0x3d00 request names at +0x5c and +0x60 (0xf350f4..0xf35108).
pub const RELEASE_CLASSES: StatisticsBuffer = StatisticsBuffer(0x5ffb_f930);
pub const RELEASE_OFFSETS: StatisticsBuffer = StatisticsBuffer(0x5ffb_ece0);
pub const RELEASE_EXTRA: StatisticsBuffer = StatisticsBuffer(0x5ffb_f4e0);
pub const RELEASE_COPY: StatisticsBuffer = StatisticsBuffer(0x5ffb_6120);

/// X-M1 1.50: the measurement history ring 0xf32654 writes. The shared record
/// keeps its position at +0xa4 (0x7567ec/0x756814, grant 16): a started word, the
/// newest slot byte at +4 and the ring address at +8.
///
/// Retained readers, by module: FocusSearch machine 34 at 0xf26688, 0xf2706c and
/// 0xf2821c; FocusSearch machine 23 at 0xf3db20, 0xf43d24 and 0xf47a1c;
/// ShotSequence machines 5 and 25 at 0xf16b08. Also 0xf3dbcc checks started and
/// walks the five newest entries; 0xf47d48 reads the newest cell levels.
/// The five-slot walks also occur at 0xf2821c and 0xf16b08.
pub const HISTORY_RING: u32 = 0x5ffe_e718;
pub const HISTORY_ENTRY_BYTES: usize = 552;
/// 0xf326d4..0xf326e4 clears the started word of six entries; the ring uses five.
pub const HISTORY_CLEARED_ENTRIES: usize = 6;

/// X-M1 1.50: 0xf32738..0xf32878. Banks come from 0x5ffee414 then 0x5ffee314,
/// the levels from the half-word banks at 0x5ffee518, the last word from 0x5ffee514.
pub fn history_entry_bytes(entry: &crate::history::HistoryEntry) -> [u8; HISTORY_ENTRY_BYTES] {
    let mut bytes = [0; HISTORY_ENTRY_BYTES];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    let statistics = entry.statistics();
    put(0, &u32::from(entry.written()).to_le_bytes());
    for (base, bank) in [(4, &statistics.banks[1]), (0x104, &statistics.banks[0])] {
        for (cell, value) in bank.iter().enumerate() { put(base + cell * 4, &value.to_le_bytes()); }
    }
    put(0x204, &entry.cell_levels()[0].to_le_bytes());
    put(0x206, &entry.cell_levels()[1].to_le_bytes());
    put(0x208, &entry.exposure().to_le_bytes());
    put(0x20a, &entry.reference_level().to_le_bytes());
    for (index, word) in entry.retained().iter().enumerate() { put(0x20c + index * 4, &word.0.to_le_bytes()); }
    put(0x224, &statistics.word.0.to_le_bytes());
    bytes
}

/// X-M1 1.50 source publication at 0xf38cb8..0xf38d38 and 0xf396f0..0xf39718.
/// The averaged path never writes the flag; owned always publishes flag 1.
pub fn source_publication(publication: crate::source::SourcePublication<'_>) -> [u8; 16] {
    use crate::source::SourcePublication;
    let (flag, summary, means, strength): (u32, u32, u32, u8) = match publication {
        SourcePublication::Withdrawn => (0, 0, 0, 0),
        SourcePublication::Fresh { strength, .. } => (1, 0x16c05a4, 0x16c05a4, strength.0),
        SourcePublication::Averaged { strength, .. } => (1, 0x16c05a4, 0x16c0622, strength.0),
    };
    let mut bytes = [0; 16];
    bytes[..4].copy_from_slice(&flag.to_le_bytes());
    bytes[4..8].copy_from_slice(&summary.to_le_bytes());
    bytes[8..12].copy_from_slice(&means.to_le_bytes());
    bytes[12] = strength;
    bytes
}
