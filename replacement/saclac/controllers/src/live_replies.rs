#![forbid(unsafe_code)]

//! The live router's answers: the profile commands' reply and the lens drive's progress.

use crate::layers::{CommandParameters, DriveProgress, Field, LensControl, LensDriveReplies, LensProbe, ProfileRecord};
use crate::live::Profile;
use crate::live_profile::word;
use crate::replies::{Replies, ReplyCommand, Values};
use xm1_saclac_interface::{CommandKind, CompletionStatus, ParameterValue, Parameters, RecordHandle};

pub trait Layers {
    type Profile: ProfileRecord;
    type Lens: LensControl;
    type Parameters: CommandParameters;
    type Replies: Replies;
    type LensDrive: LensDriveReplies;

    fn profile(&mut self) -> &mut Self::Profile;
    fn lens(&mut self) -> &mut Self::Lens;
    fn parameters(&mut self) -> &mut Self::Parameters;
    fn replies(&mut self) -> &mut Self::Replies;
    fn lens_drive(&mut self) -> &mut Self::LensDrive;
}

/// Answers the profile command with the profile's descriptor, the lens value, the lens
/// control's third notice and a bit for each overridden probe.
pub fn reply_profile(profile: Profile, record: RecordHandle, layers: &mut impl Layers) {
    let descriptor = layers.profile().read(record, Field::Descriptor);
    let lens = layers.lens();
    let probes = [(LensProbe::First, 2), (LensProbe::Second, 1), (LensProbe::Third, 4)]
        .into_iter()
        .filter(|&(probe, _)| lens.overridden(probe))
        .fold(0, |bits, (_, bit)| bits | bit);
    let value = lens.value();
    let notice = lens.third_notice();
    let command = match profile {
        Profile::Primary => ReplyCommand::Command02,
        Profile::Secondary => ReplyCommand::Command03,
    };
    layers.replies().reply(command, CompletionStatus(0), Values::Words(&[descriptor, value, notice, probes]));
}

/// Reports the drive progress a lens completion carries to the LensDrive command's client.
pub fn report_drive_progress(completion: Parameters<'_>, layers: &mut impl Layers) {
    let channel = word(completion, 1);
    let reported = ParameterValue(word(completion, 3));
    let fourth_carried = layers.parameters().word(CommandKind::LensDrive, 4) != 1;
    layers.lens_drive().report_progress(DriveProgress { channel, fourth_carried, reported });
}
