use super::continuation_view::{ByteCount, EventId, FileBinding, RangeSnapshot, Words};
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Progress {
    pub event: EventId,
    pub cumulative: ByteCount,
    pub fresh: Option<RangeSnapshot>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum SampleKind { Initial, ProducerReturn, ExtentHook, ExtentReady, ExtentPublished, TaskEntry, Image5Entry }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Sample { pub event: EventId, pub kind: SampleKind, pub view: RangeSnapshot }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Frame { pub event: EventId, pub words: Words }
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct StackLease {
    pub event: EventId,
    pub current: Words,
    pub task: Words,
    pub creation: Words,
    pub view: RangeSnapshot,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(super) enum Limit {
    PausedMappingSamplesNotTransientRemapExclusion,
    FrameSamplesNotContinuousStackConfinement,
    FiniteDestinationHaloNotWholeMachineWriteConfinement,
    RetainedServiceAndCallerLifetimeOnly,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Evidence {
    pub binding: Binding,
    pub payload: FileBinding,
    pub code: Option<RangeSnapshot>,
    pub caller_before: Option<EventId>,
    pub entry: Option<EventId>,
    pub caller_return: Option<EventId>,
    pub raw_calls: Vec<EventId>,
    pub progress: Vec<Progress>,
    pub samples: Vec<Sample>,
    pub frames: Vec<Frame>,
    pub stack: Option<StackLease>,
    pub limits: Vec<Limit>,
}
