use super::shared::{Address32, ObjectGroup, ObjectKind, OperationCode, ServiceCompletion, StageContext, StageResult};
use crate::capture::StepPlan;
use crate::develop::framework;
use core::ptr::read_volatile;

impl StageContext {
    pub(super) unsafe fn object(self, group: ObjectGroup, kind: ObjectKind) -> Address32 {
        Address32(unsafe { framework::xm1_develop_lookup_object(self.0, group.0, kind.0) })
    }
}

#[derive(Clone, Copy)]
#[repr(transparent)]
pub(super) struct StagePlan(*const StepPlan);

impl StagePlan {
    pub(super) unsafe fn develop(operation: OperationCode) -> Self {
        Self(unsafe { framework::xm1_develop_plan_lookup(operation.0, 0x0d) })
    }

    pub(super) fn address(self) -> Address32 {
        Address32(self.0 as u32)
    }

    pub(super) unsafe fn begin(self, context: StageContext) -> StageResult {
        StageResult(unsafe { framework::xm1_develop_begin(context.0, self.0) } as u32)
    }

    pub(super) unsafe fn complete(self, context: StageContext, completion: ServiceCompletion) -> StageResult {
        StageResult(unsafe { framework::xm1_develop_complete(context.0, self.0, completion.0 as i32) } as u32)
    }

    pub(super) unsafe fn skip(self, context: StageContext) -> StageResult {
        let next = unsafe { read_volatile((self.0 as *const u32).add(4)) };
        unsafe { framework::xm1_develop_store(context.0, next) };
        StageResult::SUCCESS
    }
}
