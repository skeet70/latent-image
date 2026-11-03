use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Value { Incoming(u8), Stack(i32), Return, CallReturn(Guest), Result(Hook), Number(u32), Unknown }
fn add(value: Value, amount: i32) -> Result<Value, Failure> {
    match value {
        Value::Stack(offset) => offset.checked_add(amount).map(Value::Stack).ok_or_else(|| refusal(Refusal::StackContext, "symbolic stack arithmetic overflows")),
        Value::Number(number) => Ok(Value::Number(number.wrapping_add(amount as u32))),
        _ => Err(refusal(Refusal::CodeIdentity, "unproved arithmetic operand")),
    }
}
fn store(registers: &mut [Value; 16], register: usize, value: Value) -> Result<(), Failure> {
    require(register != 15, Refusal::CodeIdentity, "ordinary instruction writes PC outside a modeled transfer")?;
    if register == 13 { require(matches!(value, Value::Stack(offset) if (-320..=0).contains(&offset) && offset % 4 == 0), Refusal::StackContext, "stack pointer lost admitted provenance")?; }
    registers[register] = value;
    Ok(())
}
fn returned(registers: &[Value; 16], calls: usize, target: Value) -> Result<(), Failure> {
    require(calls == 3, Refusal::HookSequence, "program returns before all retained calls")?;
    require(target == Value::Return && registers[13] == Value::Stack(0), Refusal::Handback, "program does not restore caller stack/return target")?;
    require(registers[0] == Value::Result(Hook::Normalizer), Refusal::NormalizerResult, "program does not return arbitrary symbolic normalizer result")?;
    require((4..12).all(|index| registers[index] == Value::Incoming(index as u8)), Refusal::Handback, "program changes an incoming callee-saved register")
}
pub(super) fn derive(catalog: &Catalog) -> Result<Profile, Failure> {
    let mut registers: [Value; 16] = std::array::from_fn(|index| Value::Incoming(index as u8));
    registers[13] = Value::Stack(0);
    registers[14] = Value::Return;
    let mut stack = BTreeMap::new();
    let mut calls = Vec::new();
    let mut low = 0i32;
    let mut return_pc = None;
    for offset in (0..32).step_by(4) {
        let pc = Guest(CARRIER.0 + offset);
        let instruction = catalog.word(pc).map_err(|error| refusal(Refusal::CodeIdentity, error))?.1;
        if return_pc.is_some() { require(instruction == 0xe320f000, Refusal::CodeIdentity, "non-NOP bytes follow the modeled return")?; continue; }
        require(instruction >> 28 == 14, Refusal::CodeIdentity, "conditional/unmodeled instruction in straight-line carrier")?;
        if let Some(target) = branch(pc, instruction) {
            let phase = *[Hook::Prehook, Hook::Normalizer, Hook::Posthook].get(calls.len()).ok_or_else(|| refusal(Refusal::HookSequence, "program has an extra external call"))?;
            require(target == [Guest(0xbfb1b8), Guest(0xbfb20c), Guest(0xbfb1e0)][calls.len()], Refusal::HookSequence, "external call escapes ordered retained service set")?;
            require(matches!(registers[13], Value::Stack(offset) if offset % 8 == 0), Refusal::StackContext, "external call has an unaligned symbolic stack")?;
            for index in [0, 1, 2, 3, 12] { registers[index] = Value::Unknown; }
            registers[0] = Value::Result(phase);
            registers[14] = Value::CallReturn(Guest(pc.0 + 4));
            calls.push((pc, target));
        } else if instruction & 0xffff0000 == 0xe92d0000 {
            let mask = instruction & 0xffff;
            require(mask != 0 && mask & ((1 << 13) | (1 << 15)) == 0, Refusal::CodeIdentity, "push writes back PC/SP or stores PC")?;
            let Value::Stack(sp) = registers[13] else { return Err(refusal(Refusal::StackContext, "push lost stack provenance")); };
            let next = sp.checked_sub(mask.count_ones() as i32 * 4).ok_or_else(|| refusal(Refusal::StackContext, "push overflows"))?;
            require(next >= -320, Refusal::StackContext, "push exceeds frame bound")?;
            let mut address = next;
            for (index, value) in registers.iter().enumerate() {
                if mask & (1 << index) != 0 { require(stack.insert(address, *value).is_none(), Refusal::StackContext, "push overwrites a live saved word")?; address += 4; }
            }
            registers[13] = Value::Stack(next);
            low = low.min(next);
        } else if instruction & 0xffff0000 == 0xe8bd0000 {
            let mask = instruction & 0xffff;
            require(mask != 0 && mask & (1 << 13) == 0 && mask & ((1 << 14) | (1 << 15)) != ((1 << 14) | (1 << 15)), Refusal::CodeIdentity, "pop has an unmodeled PC/SP writeback")?;
            let Value::Stack(sp) = registers[13] else { return Err(refusal(Refusal::StackContext, "pop lost stack provenance")); };
            let mut address = sp;
            let mut target = None;
            for (index, register) in registers.iter_mut().enumerate() {
                if mask & (1 << index) != 0 {
                    let value = stack.remove(&address).ok_or_else(|| refusal(Refusal::StackContext, "pop reads uninitialized/unallocated caller memory"))?;
                    if index == 15 { target = Some(value); } else { *register = value; }
                    address += 4;
                }
            }
            require(address <= 0, Refusal::StackContext, "pop crosses caller stack boundary")?;
            registers[13] = Value::Stack(address);
            if let Some(target) = target { returned(&registers, calls.len(), target)?; return_pc = Some(pc); }
        } else if instruction == 0xe12fff1e {
            returned(&registers, calls.len(), registers[14])?;
            return_pc = Some(pc);
        } else if instruction == 0xe320f000 {
        } else if instruction & 0x0fe00ff0 == 0x01a00000 {
            let destination = ((instruction >> 12) & 15) as usize;
            let source = (instruction & 15) as usize;
            require(source != 15 && instruction & (1 << 20) == 0, Refusal::CodeIdentity, "MOV reads PC or changes status flags")?;
            let value = registers[source];
            store(&mut registers, destination, value)?;
        } else if instruction & 0x0e000000 == 0x02000000 {
            let operation = (instruction >> 21) & 15;
            let destination = ((instruction >> 12) & 15) as usize;
            let source = ((instruction >> 16) & 15) as usize;
            require(destination != 15 && source != 15 && instruction & (1 << 20) == 0, Refusal::CodeIdentity, "data operation writes/reads PC or flags outside model")?;
            let immediate = (instruction & 255).rotate_right(((instruction >> 8) & 15) * 2);
            let value = match operation {
                13 => Value::Number(immediate),
                4 | 2 => {
                    require(immediate <= 320, Refusal::StackContext, "frame immediate exceeds bound")?;
                    add(registers[source], if operation == 4 { immediate as i32 } else { -(immediate as i32) })?
                }
                _ => return Err(refusal(Refusal::CodeIdentity, "data operation is outside straight-line model")),
            };
            store(&mut registers, destination, value)?;
            if let Value::Stack(sp) = registers[13] { low = low.min(sp); }
        } else {
            return Err(refusal(Refusal::CodeIdentity, "instruction or control transfer is outside the straight-line model"));
        }
    }
    let returned = return_pc.ok_or_else(|| refusal(Refusal::Handback, "program has no modeled return"))?;
    require(stack.is_empty(), Refusal::StackContext, "program leaves live saved words")?;
    Ok(Profile { calls, returned, scratch_low: low })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(offset: u32, target: u32) -> u32 {
        0xeb000000 | (((target as i64 - i64::from(CARRIER.0 + offset) - 8) / 4) as u32 & 0xffffff)
    }
    fn body() -> [u32; 8] {
        [0xe92d4010, call(4, 0xbfb1b8), call(8, 0xbfb20c), 0xe1a04000,
            call(16, 0xbfb1e0), 0xe1a00004, 0xe8bd8010, 0xe320f000]
    }
    fn admitted(words: &[u32; 8]) -> Result<Profile, Failure> {
        let bytes = words.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>();
        let binding = SourceBinding { kind: SourceKind::Image5, start: CARRIER, bytes: ByteCount(32), sha256: Digest::of(&bytes) };
        derive(&Catalog(vec![SourceImage { binding, bytes }]))
    }
    fn rejects(words: [u32; 8], reason: Refusal) {
        assert!(matches!(admitted(&words), Err(failure) if failure.reason == reason));
    }
    #[test]
    fn accepts_distinct_register_and_frame_implementations_of_the_same_contract() {
        let original = body();
        assert!(matches!(admitted(&original), Ok(proof) if proof.scratch_low == -8 && proof.calls.len() == 3));
        let mut alternate = original;
        alternate[0] = 0xe92d4020;
        alternate[3] = 0xe1a05000;
        alternate[5] = 0xe1a00005;
        alternate[6] = 0xe8bd8020;
        assert!(admitted(&alternate).is_ok());
        let framed = [0xe92d4c10, 0xe28db008, call(8, 0xbfb1b8), call(12, 0xbfb20c),
            0xe1a04000, call(20, 0xbfb1e0), 0xe1a00004, 0xe8bd8c10];
        assert!(matches!(admitted(&framed), Ok(proof) if proof.scratch_low == -16));
    }
    #[test]
    fn refuses_every_unmodeled_pc_write_and_backward_transfer() {
        for instruction in [0xe1a0f00e, 0xe59df000, 0xe8b48000, 0xe284f004, 0x03a0f000, 0xeafffffe] {
            let mut words = body();
            words[5] = instruction;
            rejects(words, Refusal::CodeIdentity);
        }
    }
    #[test]
    fn refuses_reordered_hooks_and_substituted_normalizer_results() {
        let mut reordered = body();
        reordered[1] = call(4, 0xbfb1e0);
        rejects(reordered, Refusal::HookSequence);
        let mut constant = body();
        constant[5] = 0xe3a00000;
        rejects(constant, Refusal::NormalizerResult);
    }
    #[test]
    fn refuses_unrestored_callee_saves_stack_or_return_provenance() {
        let mut registers = body();
        registers[6] = 0xe8bd8020;
        rejects(registers, Refusal::Handback);
        let mut target = body();
        target[0] = 0xe92d0030;
        rejects(target, Refusal::Handback);
        let mut sp = body();
        sp[5] = 0xe3a0d001;
        rejects(sp, Refusal::StackContext);
        let mut alignment = body();
        alignment[0] = 0xe92d4030;
        rejects(alignment, Refusal::StackContext);
    }
    #[test]
    fn refuses_unmodeled_memory_effects_and_missing_return() {
        let mut write = body();
        write[5] = 0xe5800000;
        rejects(write, Refusal::CodeIdentity);
        let mut missing = body();
        missing[6] = 0xe320f000;
        rejects(missing, Refusal::Handback);
    }
}
