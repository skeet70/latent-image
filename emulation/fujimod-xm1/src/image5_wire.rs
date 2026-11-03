use super::continuation_view::{ByteCount, EventId, FileBinding, RangeSnapshot};
pub(super) use super::plain_wire::{Frame, Limit, StackLease};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Role { Retained, Owned }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Artifacts {
    pub manifest: FileBinding,
    pub linked: FileBinding,
    pub symbols: FileBinding,
    pub image4: FileBinding,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Binding { pub role: Role, pub artifacts: Option<Artifacts> }

/// One feed-callback entry and, for a storage-read progress callback, the staging block it completed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Progress {
    pub event: EventId,
    pub cumulative: ByteCount,
    pub fresh: Option<RangeSnapshot>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SampleKind { DecoderFinish, ProducerReturn, Image5Entry }
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Region { Destination, Bss }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Sample { pub event: EventId, pub kind: SampleKind, pub region: Region, pub view: RangeSnapshot }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Evidence {
    pub binding: Binding,
    pub payload: FileBinding,
    pub code: Option<RangeSnapshot>,
    pub caller_before: Option<EventId>,
    pub entry: Option<EventId>,
    pub caller_return: Option<EventId>,
    pub progress: Vec<Progress>,
    pub samples: Vec<Sample>,
    pub frames: Vec<Frame>,
    pub stack: Option<StackLease>,
    pub limits: Vec<Limit>,
}
