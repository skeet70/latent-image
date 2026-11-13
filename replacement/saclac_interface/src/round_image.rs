#![forbid(unsafe_code)]

//! What the measurement round's image lane asks of imgdc (0x3f30..0x3f33) for its clients' image,
//! snapshot, cancel and stop commands.

/// The channel a client's image or snapshot command names; imgdc receives it with the request and
/// the client's reply carries it back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageChannel(pub u32);

/// The two words a client's image command (0x14) carries for imgdc, which alone gives them meaning.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageArguments(pub [u32; 2]);

/// The halfword a client's snapshot command (0x2b) carries for imgdc.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotArgument(pub i16);

/// The word a client's cancel command (0x15) carries for imgdc.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CancelArgument(pub u32);

/// The two halfwords the area configuration leaves for the lane's channel.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ImageArea(pub [i16; 2]);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageLaneRequest {
    /// imgdc 0x3f30; finishes with completion 0x47.
    Image { arguments: ImageArguments, area: ImageArea, channel: ImageChannel },
    /// imgdc 0x3f33; finishes with completion 0x66.
    Snapshot { argument: SnapshotArgument, area: ImageArea, channel: ImageChannel },
    /// imgdc 0x3f32.
    Cancel(CancelArgument),
    /// imgdc 0x3f31.
    Stop,
}
