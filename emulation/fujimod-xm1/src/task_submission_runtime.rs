use super::{Address, Boundary, Code, Context, Core, Digest, Kernel, Object, TaskId, View, Word, pointer, require, view, word};
use super::task_submission_control::{ProbeCase, ProbeStop};
use super::task_submission_program::{Entry, Program, Target};
use fujimod_emulator::board::factory::MemoryMap;
use fujimod_xm1::{bridge::read, machine::Machine};
use std::path::PathBuf;

pub(super) struct Arguments {
    pub(super) run: PathBuf, pub(super) update: PathBuf, pub(super) rom: PathBuf,
    pub(super) analysis: PathBuf, pub(super) output: PathBuf, pub(super) contract: Option<PathBuf>,
    pub(super) native: Option<PathBuf>, pub(super) case: ProbeCase,
}
impl Arguments {
    pub(super) fn parse() -> Result<Self, String> {
        let mut run = None; let mut update = None; let mut rom = None; let mut analysis = None;
        let mut output = None; let mut contract = None; let mut native = None; let mut case = None;
        let mut arguments = std::env::args_os().skip(1);
        while let Some(flag) = arguments.next() {
            let flag = flag.into_string().map_err(|_| super::USAGE)?;
            let value = arguments.next().ok_or(super::USAGE)?;
            match flag.as_str() {
                "--run" => put(&mut run, PathBuf::from(value))?, "--update" => put(&mut update, PathBuf::from(value))?,
                "--boot-rom" => put(&mut rom, PathBuf::from(value))?, "--analysis" => put(&mut analysis, PathBuf::from(value))?,
                "--output" => put(&mut output, PathBuf::from(value))?, "--contract" => put(&mut contract, PathBuf::from(value))?,
                "--native" => put(&mut native, PathBuf::from(value))?,
                "--case" => put(&mut case, value.into_string().map_err(|_| super::USAGE)?.parse::<ProbeCase>()?)?,
                _ => return Err(super::USAGE.into()),
            }
        }
        Ok(Self { run: run.ok_or(super::USAGE)?, update: update.ok_or(super::USAGE)?, rom: rom.ok_or(super::USAGE)?,
            analysis: analysis.ok_or(super::USAGE)?, output: output.ok_or(super::USAGE)?, contract, native, case: case.unwrap_or(ProbeCase::Natural) })
    }
}
fn put<T>(slot: &mut Option<T>, value: T) -> Result<(), String> { require(slot.is_none(), "duplicate observer argument")?; *slot = Some(value); Ok(()) }
#[derive(Clone, Copy)]
pub(super) struct Binding { pub(super) id: TaskId, pub(super) core: Core, pub(super) descriptor: Address, pub(super) caller_sp: Address }
#[derive(Clone, Copy)]
pub(super) struct Invocation { pub(super) entry: Entry, pub(super) sp: Address }

pub(super) fn local(machine: &Machine, maps: &[MemoryMap], state: &Context, binding: Option<Binding>, invocation: Option<Invocation>, kernels: &[Kernel], program: &Program, boundary: Boundary, taken: Option<bool>) -> Result<Vec<View>, String> {
    let mut views = vec![view(machine, maps, state, Object::SharedDomains, 0x400c0800, 256)?];
    let roots = view(machine, maps, state, Object::InterfaceRoots, 0x96bd20, 8)?;
    for (offset, expected) in [(0, 0x4004dae0), (4, 0x400ddaa0)] {
        let root = word(&roots.bytes, offset)?; require(root == expected, "installed interface root differs")?;
        views.push(view(machine, maps, state, Object::InterfaceTable, root, 0x162c)?);
    }
    views.push(roots);
    for (object, address, bytes) in [(Object::ErrorState, 0x40a0f82c, 4), (Object::Callback, 0x4089ea8c, 4),
        (Object::Cursor, 0x4089ea98, 4), (Object::Lock, 0x409c280c, 4), (Object::ClassTable, 0x7cad9c, 420),
        (Object::LogCallback, 0x4004f15c, 4), (Object::LogMode, 0x4004f25c, 4), (Object::LogEnable, 0x4004f5e4, 38)] {
        views.push(view(machine, maps, state, object, address, bytes)?);
    }
    if let Some(binding) = binding {
        require(binding.core == state.core && binding.id.0 != 0 && binding.id.0 <= 104, "active binding differs from stopped core")?;
        views.push(view(machine, maps, state, Object::Descriptor, binding.descriptor.0, 28)?);
        let i = u32::from(binding.id.0) - 1;
        let wrapper = view(machine, maps, state, Object::Wrapper, 0x40a5b204 + 8 * i, 8)?;
        let storage = view(machine, maps, state, Object::Storage, 0x40a0fc44 + 12 * i, 12)?;
        let pool = word(&storage.bytes, 0)?; let bytes = word(&storage.bytes, 4)?;
        if pool != 0 {
            let physical = pool & !0x40000000;
            require(bytes == 1024 && physical >= 0xa10200 && physical.checked_add(bytes).is_some_and(|end| end <= 0xa5b200), "reserved backing leaves task pool")?;
            views.push(view(machine, maps, state, Object::Stack, pool, bytes as usize)?);
        }
        views.push(wrapper); views.push(storage);
        let (low, high) = backing(binding, kernels)?;
        if let Some(invocation) = invocation {
            let native_low = invocation.sp.0.checked_add_signed(program.frame_low(invocation.entry)?).ok_or("native frame wraps")? & !0x40000000;
            require(native_low >= low && (invocation.sp.0 & !0x40000000) <= high, "native frame leaves disjoint caller backing")?;
        }
        views.push(view(machine, maps, state, Object::Stack, low, usize::try_from(high-low).map_err(|_| "caller backing size exceeds host")?)?);
        if taken != Some(false) && matches!(boundary, Boundary::AuxiliaryCall | Boundary::CreationCall) {
            let invocation = invocation.ok_or("parameter call has no invocation")?;
            let bytes = if boundary == Boundary::AuxiliaryCall { 8 } else { 24 };
            let start = state.registers[1].0;
            let native_low = invocation.sp.0.checked_add_signed(program.frame_low(invocation.entry)?).ok_or("parameter frame wraps")?;
            require(start & 3 == 0 && start >= native_low && start.checked_add(bytes).is_some_and(|end| end <= invocation.sp.0), "parameters leave native private frame")?;
            views.push(view(machine, maps, state, Object::Parameters, start, bytes as usize)?);
        }
    }
    Ok(views)
}
fn backing(binding: Binding, kernels: &[Kernel]) -> Result<(u32,u32), String> {
    let boot_sp = match binding.core { Core::Primary => 0x40063420, Core::Secondary => 0x400f33e0 };
    if binding.caller_sp.0 == boot_sp {
        let low = match binding.core { Core::Primary => 0x62c54, Core::Secondary => 0xf2c14 };
        let high = boot_sp.checked_add(32).ok_or("boot caller frame wraps")? & !0x40000000;
        require(low < (binding.caller_sp.0 & !0x40000000), "boot caller backing is empty")?;
        return Ok((low,high));
    }
    let kernel = kernels.get(binding.core.index()).ok_or("caller kernel is absent")?;
    let current = kernel.tasks.iter().find(|task| task.node == kernel.current).ok_or("current caller task is absent")?;
    require(current.id == TaskId(104) && current.stack_bytes == Word(1024), "controlled caller is not root104")?;
    let low = current.stack_base.0 & !0x40000000;
    let high = low.checked_add(current.stack_bytes.0).ok_or("task caller frame wraps")?;
    let sp = binding.caller_sp.0 & !0x40000000;
    require(low <= sp && sp < high, "controlled caller SP leaves its allocated stack")?;
    Ok((low,high))
}
pub(super) fn condition(instruction: Word, cpsr: Word) -> Result<bool, String> {
    let n = cpsr.0 & 0x80000000 != 0; let z = cpsr.0 & 0x40000000 != 0;
    let c = cpsr.0 & 0x20000000 != 0; let v = cpsr.0 & 0x10000000 != 0;
    Ok(match instruction.0 >> 28 { 0 => z, 1 => !z, 2 => c, 3 => !c, 4 => n, 5 => !n, 6 => v, 7 => !v,
        8 => c && !z, 9 => !c || z, 10 => n == v, 11 => n != v, 12 => !z && n == v, 13 => z || n != v,
        14 => true, _ => return Err("semantic call is not conditional A32".into()) })
}
pub(super) fn call_target(boundary: Boundary) -> Option<Target> {
    Some(match boundary {
        Boundary::CriticalEnterCall => Target::CriticalEnter, Boundary::CriticalExitCall => Target::CriticalExit,
        Boundary::ReservationCall => Target::Reservation, Boundary::FillCall => Target::Fill, Boundary::CacheCall => Target::Cache,
        Boundary::LogCall => Target::Log, Boundary::ErrorCallbackCall => Target::ErrorCallback, Boundary::TryLockCall => Target::TryLock,
        Boundary::UnlockCall => Target::Unlock, Boundary::AuxiliaryCall => Target::AuxiliaryService, Boundary::CreationCall => Target::CreationService,
        Boundary::ActivationCall => Target::ActivationService, Boundary::IdentityCall => Target::CurrentCoreService,
        Boundary::SaveMaskCall => Target::SaveMaskService, Boundary::RestoreMaskCall => Target::RestoreMaskService,
        _ => return None,
    })
}
pub(super) fn is_return(boundary: Boundary) -> bool {
    matches!(boundary, Boundary::CriticalEnterReturn | Boundary::CriticalExitReturn | Boundary::ReservationReturn
        | Boundary::FillReturn | Boundary::CacheReturn | Boundary::LogReturn | Boundary::ErrorCallbackReturn
        | Boundary::TryLockReturn | Boundary::UnlockReturn | Boundary::AuxiliaryReturn | Boundary::CreationReturn
        | Boundary::ActivationReturn | Boundary::IdentityReturn | Boundary::SaveMaskReturn | Boundary::RestoreMaskReturn)
}
fn register(state: &Context, index: u32) -> Result<u32, String> {
    match index { 0..=12 => Ok(state.registers[index as usize].0), 13 => Ok(state.sp.0), 14 => Ok(state.lr.0), 15 => Ok(state.pc.0), _ => Err("call register exceeds A32".into()) }
}
pub(super) fn validate_call(machine: &Machine, state: &Context, binding: Binding, invocation: Invocation, program: &Program, instruction: Word, target: Target) -> Result<(), String> {
    let actual = if instruction.0 & 0x0ffffff0 == 0x012fff30 { register(state, instruction.0 & 15)? }
        else if instruction.0 & 0x0f000000 == 0x0b000000 { state.pc.0.wrapping_add(8).wrapping_add((((instruction.0 & 0xffffff) << 8) as i32 >> 6) as u32) }
        else { return Err("semantic site is not an A32 call".into()); };
    if let Some(call) = program.call(state.pc.0, invocation.entry, invocation.sp, state.sp)? {
        require(call.target == target, "native call role differs")?;
        let encoded_register = if instruction.0 & 0x0ffffff0 == 0x012fff30 { Some((instruction.0 & 15) as u8) } else { None };
        require(call.target_register == encoded_register, "native call target register differs")?;
    }
    let fixed = match target {
        Target::CriticalEnter => Some(0x7235d8), Target::CriticalExit => Some(0x7236b0), Target::Reservation => Some(0x725c6c),
        Target::Fill => Some(0x7221c8), Target::Cache => Some(0x42634), Target::Log => Some(0x41a34),
        Target::ErrorCallback => Some(0x723398), Target::TryLock => Some(0x427d4), Target::Unlock => Some(0x427fc), _ => None,
    };
    let expected = if let Some(fixed) = fixed { fixed } else {
        let (root_index, slot) = match target {
            Target::AuxiliaryService | Target::CreationService => {
                let class = pointer(machine, state.core, 0x7cad9c + 4 * (u32::from(binding.id.0) - 1))?;
                require(class == 1 || class == 2, "admitted descriptor class is outside startup roots")?;
                (class-1, if target == Target::AuxiliaryService { 0x80 } else { 0 })
            }
            Target::ActivationService => (state.core.index() as u32, 0xc), Target::CurrentCoreService => (0, 0x1618),
            Target::SaveMaskService => (state.core.index() as u32, 0x164), Target::RestoreMaskService => (state.core.index() as u32, 0x168),
            _ => return Err("unknown dynamic service".into()),
        };
        let root = pointer(machine, state.core, 0x96bd20 + 4 * root_index)?;
        pointer(machine, state.core, root.checked_add(slot).ok_or("service slot wraps")?)?
    };
    require(actual == expected, "actual service call target differs from published dependency")?;
    if matches!(target, Target::AuxiliaryService | Target::CreationService | Target::ActivationService) {
        let class = pointer(machine, state.core, 0x7cad9c + 4 * (u32::from(binding.id.0) - 1))?;
        require(state.registers[0].0 == (class << 16 | u32::from(binding.id.0)), "encoded service id differs")?;
    }
    Ok(())
}
pub(super) fn probe_stop(boundary: Boundary, state: &Context) -> ProbeStop {
    match boundary {
        Boundary::Submission => ProbeStop::SubmissionEntry, Boundary::DeletionEntry => ProbeStop::DeletionEntry,
        Boundary::ReleaseEntry => ProbeStop::ReleaseEntry, Boundary::ReleaseReturn => ProbeStop::ReleaseReturn,
        Boundary::UnlockCall => ProbeStop::UnlockCall,
        Boundary::TaskEntry if state.pc == Address(0x69cbf0) => ProbeStop::CallbackEntry,
        Boundary::EnteredInstruction if state.pc == Address(0x69cbf4) => ProbeStop::CallbackEntered,
        Boundary::EnteredInstruction if state.pc == Address(0x69d164) => ProbeStop::OuterReturn,
        _ => ProbeStop::Other,
    }
}
pub(super) struct RawInputs {
    pub(super) run: Vec<u8>, pub(super) update: Vec<u8>, pub(super) rom: Vec<u8>,
    pub(super) run_digest: Digest, pub(super) update_digest: Digest, pub(super) rom_digest: Digest,
}
pub(super) fn inputs(arguments: &Arguments) -> Result<RawInputs,String> {
    use fujimod_xm1::bridge::sha256;
    let run = read(&arguments.run)?; let update = read(&arguments.update)?; let rom = read(&arguments.rom)?;
    let run_digest = Digest(sha256(&run)); let update_digest = Digest(sha256(&update)); let rom_digest = Digest(sha256(&rom));
    Ok(RawInputs { run, update, rom, run_digest, update_digest, rom_digest })
}
pub(super) fn identity<'a>(run: Digest, update: Digest, rom: Digest, code: &'a [Code]) -> Result<super::Identity<'a>,String> {
    use fujimod_xm1::bridge::sha256;
    require(run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef" && rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be", "observer run/ROM identity differs")?;
    Ok(super::Identity { run, update, boot_rom: rom, observer: Digest(sha256(&read(&std::env::current_exe().map_err(|error| error.to_string())?)?)), sources: code.iter().map(|code| super::SourceIdentity { name: code.name, base: Address(code.base), bytes: code.bytes.len(), sha256: &code.sha256 }).collect() })
}
