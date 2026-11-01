use super::{Case, Digest, Identity, Perturbation, Role, Sample, WatchSummary, START, END, STATE, CURSOR, OUTPUT, LOCK};
use fujimod_xm1::bridge::{read, sha256, write};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
struct CodeWord { guest: u32, physical: u32, expected: u32, tcm: [[u32; 2]; 2] }
#[derive(Deserialize)]
struct Hit { address: String, core: usize, registers: BTreeMap<String, String>, sp: String, cpsr: u32, code_word: Option<CodeWord> }
#[derive(Deserialize)]
struct Observation {
    schema: u32, role: Role, case: Case, identity: Identity, checkpoint_sha256: Digest, checkpoint_reused: bool,
    accesses_sha256: Digest,
    completed: bool, error: Option<String>, machine_failure: Option<String>, hits: Vec<Hit>, samples: Vec<Sample>,
    perturbation: Option<Perturbation>, watch: Result<WatchSummary, String>, watch_end_error: Option<String>, confinement: Result<[usize; 2], String>,
}
#[derive(Serialize)]
struct Comparison { schema: u32, accepted: bool, case: Case, retained_receipt: Digest, owned_receipt: Digest, original_source_matches: bool,
    return_value: u32, output: u32, record: [u32; 3], cursor: u32, lock: u32, surrounding_words_unchanged: bool,
    controls_are_perturbed: bool, error: Option<String> }
#[derive(Deserialize)]
struct AccessEvent { source: super::Access, physical: u64, size: u64, core: Option<usize>, pc: Option<u32>, before: Option<Vec<u8>>, value: Option<u64> }
#[derive(Deserialize)]
struct Accesses { records: Vec<AccessEvent> }
fn require(value: bool, message: &str) -> Result<(), String> { if value { Ok(()) } else { Err(message.into()) } }
fn at(hit: &Hit) -> Result<u32, String> { super::number(&hit.address) }
fn register(hit: &Hit, name: &str) -> Result<u32, String> { super::number(hit.registers.get(name).ok_or("receipt register absent")?) }
fn snapshot(observation: &Observation, address: u32) -> Result<(&Hit, &Sample), String> {
    let sample = observation.samples.iter().find(|sample| observation.hits.get(sample.hit)
        .is_some_and(|hit| hit.core == 1 && at(hit) == Ok(address))).ok_or("selected snapshot absent")?;
    Ok((&observation.hits[sample.hit], sample))
}
fn value(sample: &Sample, address: u32) -> Result<u32, String> {
    let word = sample.words.iter().find(|word| word.guest.0 == address).ok_or("sampled word absent")?;
    require(word.physical.0 == address & !0x40000000 && word.values == [word.physical_value; 2], "incoherent sampled word")?;
    Ok(word.physical_value)
}
fn concurrent_lifetime(observation: &Observation, accesses: &Accesses, address: u32) -> Result<(), String> {
    let (_, before) = snapshot(observation, 0x725e78)?;
    let (_, endpoint) = snapshot(observation, 0x69cf38)?;
    let first = before.watch_cursor.ok_or("helper return lacks access cursor")?;
    let last = endpoint.watch_cursor.ok_or("wrapper return lacks access cursor")?;
    let records = accesses.records.get(first..last).ok_or("access interval leaves sidecar")?;
    let mut current = value(before, address)?;
    for row in records {
        if !matches!(row.source, super::Access::CpuWrite | super::Access::HostWrite | super::Access::ModelBusWrite)
            || row.physical >= u64::from(address & !0x40000000) + 4
            || row.physical.checked_add(row.size).ok_or("access span overflow")? <= u64::from(address & !0x40000000) { continue; }
        require(matches!(row.source, super::Access::CpuWrite) && row.physical == u64::from(address & !0x40000000) && row.size == 4, "unattributed shared-word write")?;
        let core = row.core.filter(|core| *core == 0).ok_or("cursor writer is not the concurrent core")?;
        let pc = row.pc.filter(|pc| (START..0x725dd0).contains(pc)).ok_or("cursor writer is outside helper")?;
        let preimage = row.before.as_ref().filter(|bytes| bytes.len() == 4).ok_or("cursor write lacks preimage")?;
        let preimage = u32::from_le_bytes(preimage.as_slice().try_into().map_err(|_| "cursor preimage length")?);
        let stored = u32::try_from(row.value.ok_or("cursor write lacks value")?).map_err(|_| "cursor write value exceeds a word")?;
        require(preimage == current, "cursor write chain is discontinuous")?;
        let (index, _) = observation.hits.iter().enumerate().find(|(index, hit)| {
            *index > before.hit && *index < endpoint.hit && hit.core == core && at(hit) == Ok(pc)
                && observation.samples.iter().find(|sample| sample.hit == *index).is_some_and(|sample| value(sample, address) == Ok(preimage))
        }).ok_or("cursor write lacks matching preexecution instruction")?;
        let (entry_index, entered) = observation.hits[..index].iter().enumerate().rev()
            .find(|(_, hit)| hit.core == core && at(hit) == Ok(START)).ok_or("concurrent helper entry absent")?;
        require(register(entered, "r0")? == 103 && register(entered, "r1")? == 0x400 && register(entered, "r2")? == OUTPUT + 8,
            "concurrent helper is not task104 reservation")?;
        let (return_index, _) = observation.hits.iter().enumerate().skip(index + 1)
            .find(|(_, hit)| hit.core == core && at(hit) == Ok(0x725e78)).filter(|(index, _)| *index < endpoint.hit)
            .ok_or("concurrent helper return absent before endpoint")?;
        let invocation = &observation.hits[entry_index..=return_index];
        for address in [0x427d4, 0x427fc] {
            require(invocation.iter().filter(|hit| hit.core == core && at(hit) == Ok(address)).count() == 1, "concurrent helper synchronization differs")?;
        }
        let acquired = invocation.iter().find(|hit| hit.core == core && at(hit) == Ok(0x725cc4)).ok_or("concurrent acquisition result absent")?;
        require(register(acquired, "r0")? == 0, "concurrent helper did not acquire lock")?;
        let next = observation.samples.iter().find(|sample| sample.hit > index && observation.hits[sample.hit].core == core).ok_or("cursor post-store sample absent")?;
        require(value(next, address)? == stored, "shared-word write lacks committed sampled value")?;
        current = stored;
    }
    require(current == value(endpoint, address)?, "endpoint shared-word change is unattributed")
}
fn validate(observation: &Observation, role: Role, original: &[u8], linked: &[u8]) -> Result<(), String> {
    require(observation.schema == 1 && observation.role == role && observation.completed && observation.checkpoint_reused
        && observation.error.is_none() && observation.machine_failure.is_none() && observation.watch_end_error.is_none(), "receipt did not complete cleanly")?;
    require(observation.watch.as_ref().is_ok_and(|watch| watch.enabled && watch.violation.is_none())
        && observation.confinement.as_ref().is_ok_and(|counts| counts.iter().sum::<usize>() > 0), "receipt watch/confinement refused")?;
    require((observation.case == Case::Natural) == observation.perturbation.is_none(), "control labelling differs from perturbation")?;
    require(observation.identity.update.0 == match role {
        Role::Retained => "43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490",
        Role::Owned => "cb3834a423d288d8acc0af9d7977d919945068ffae807d1ad7a940963cde3c00",
    } && observation.identity.run.0 == "efa32ff8d2a378ba9e098ac28015faf82856765c866d5db5b97cb1c38342deef"
        && observation.identity.rom.0 == "4ecedfbfc1910bde488d36abe4380621f8845e6afcfc6cf54a49a33ec0abf9be"
        && observation.identity.original_image4.0 == sha256(original)
        && observation.identity.linked.0 == sha256(linked)
        && observation.identity.loader_a.0 == "21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882"
        && observation.identity.loader_b.0 == "8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec", "receipt input identity is not the admitted role/source")?;
    for hit in &observation.hits {
        let pc = at(hit)?;
        if (START..0x725dd0).contains(&pc) {
            let code = hit.code_word.as_ref().ok_or("helper instruction lacks sampled bytes")?;
            let bytes = if role == Role::Owned && (0x725cc4..0x725da8).contains(&pc) {
                &linked[(pc - 0x725cc4) as usize..(pc - 0x725cc4 + 4) as usize]
            } else { &original[(pc - 0x61c400) as usize..(pc - 0x61c400 + 4) as usize] };
            let expected = u32::from_le_bytes(bytes.try_into().map_err(|_| "bound word truncated")?);
            require(code.guest == expected && code.physical == expected && code.expected == expected, "code word differs from independently bound artifact")?;
            for value in code.tcm.iter().flatten() {
                let size = (value >> 2) & 31;
                if value & 1 != 0 && size != 0 {
                    require((3..=14).contains(&size), "reserved TCM size")?;
                    let low = u64::from(value & 0xfffff000);
                    require(low >= u64::from(END) || low + (512u64 << size) <= u64::from(START), "code overlaps TCM")?;
                }
            }
        }
    }
    let (entry, entry_sample) = snapshot(observation, START)?;
    let (returned, post) = snapshot(observation, 0x725e78)?;
    require(entry.core == returned.core && entry.core == 1 && entry.sp == "0x400f33a8" && returned.sp == entry.sp, "helper caller/core/frame differs")?;
    require(register(entry, "r0")? == 102 && register(entry, "r1")? == 0x400 && register(entry, "r2")? == OUTPUT, "natural helper arguments differ")?;
    let start = entry_sample.hit;
    let finish = post.hit;
    let prior = |address, before| observation.hits[..before].iter().enumerate().rev()
        .find(|(_, hit)| hit.core == entry.core && at(hit) == Ok(address)).ok_or("ordered natural caller context absent");
    let (wrapper_index, wrapper) = prior(0x725dd8, start)?;
    let (call_index, call) = prior(0x69cf34, wrapper_index)?;
    let (_, dispatcher) = prior(0x69cdf8, call_index)?;
    require(dispatcher.sp == "0x400f3410" && register(dispatcher, "r0")? == 4
        && call.sp == "0x400f33e0" && register(call, "r0")? == 0x400f33e4
        && wrapper.sp == "0x400f33e0" && register(wrapper, "r0")? == 0x400f33e4,
        "dispatcher/descriptor/wrapper ABI differs")?;
    for address in [0x427d4, 0x427fc] {
        require(observation.hits[start..=finish].iter().filter(|hit| hit.core == entry.core && at(hit) == Ok(address)).count() == 1, "retained synchronization was not one-shot")?;
    }
    for name in ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11"] { require(register(entry, name)? == register(returned, name)?, "callee-saved register differs")?; }
    require((entry.cpsr ^ returned.cpsr) & 0x0fff_ffff == 0, "helper changed non-NZCV CPSR state")?;
    let expected_status = match observation.case { Case::SizeError | Case::CapacityError => (-33i32) as u32, Case::ReservedError => (-18i32) as u32, _ => 0 };
    require(register(returned, "r0")? == expected_status, "helper returned wrong status")?;
    require(value(post, OUTPUT + 4)? == 0x400, "wrapper size publication was lost")?;
    if observation.case != Case::Natural {
        let expected = match observation.case {
            Case::Reuse => (0xa10200, [0xa10200, 0x400, 1], 0x40a10600),
            Case::SizeError => (0, [0xa10200, 0x404, 0], 0x40a10600),
            Case::ReservedError => (0, [0xa10200, 0x400, 1], 0x40a10600),
            Case::CapacityError => (0, [0, 0, 0], 0x40a5b200),
            Case::Unaligned => (0xa10200, [0xa10200, 3, 1], 0x40a10200),
            Case::Wrapping => (0xa101fc, [0xa101fc, 4, 1], 0x40a10200),
            Case::AliasCursor => (0, [0xa10600, 0x400, 1], 0xa10600),
            Case::AliasState => (0, [0xa10200, 0x400, 1], 0x40a10600),
            Case::FailedLock => (0xa10200, [0xa10200, 0x400, 1], 0x40a10600),
            Case::Natural => return Err("natural case reached perturbed expectations".into()),
        };
        require(value(post, OUTPUT)? == expected.0 && [value(post, STATE)?, value(post, STATE + 4)?, value(post, STATE + 8)?] == expected.1
            && value(post, CURSOR)? == expected.2 && value(post, LOCK)? == 0, "boundary result differs from independent fixed expectation")?;
        if observation.case == Case::FailedLock {
            let (acquire_return, _) = snapshot(observation, 0x725cc4)?;
            require(register(acquire_return, "r0")? == u32::MAX, "failed acquisition control did not fail acquisition")?;
        }
    } else {
        let (_, before) = snapshot(observation, START)?;
        require(value(before, STATE)? == 0 && value(before, STATE + 4)? == 0 && value(before, STATE + 8)? == 0, "natural record not initially empty")?;
        let cursor = value(before, CURSOR)?;
        require(value(post, OUTPUT)? == cursor & !0x40000000 && value(post, STATE)? == cursor & !0x40000000
            && value(post, STATE + 4)? == 0x400 && value(post, STATE + 8)? == 1 && value(post, CURSOR)? == cursor.wrapping_add(0x400), "natural reservation result differs")?;
        for address in [0x725e78, 0x725f6c, 0x69cf38] {
            let (_, sample) = snapshot(observation, address)?;
            let buffer = sample.buffer.as_ref().ok_or("natural pool readback absent")?;
            require(buffer.physical.0 == value(sample, OUTPUT)? && buffer.bytes.len() == 0x400
                && buffer.aliases.iter().all(|digest| digest.0 == sha256(&buffer.bytes)), "pool backing/readback differs")?;
            if address == 0x725f6c { require(buffer.bytes.iter().all(|byte| *byte == 103), "retained fill did not write task103 pattern")?; }
        }
        let (fill, _) = snapshot(observation, 0x725f68)?;
        require(register(fill, "r0")? == value(post, OUTPUT)? && register(fill, "r1")? == 103 && register(fill, "r2")? == 0x400,
            "retained fill arguments differ")?;
        let (cache, _) = snapshot(observation, 0x725f78)?;
        require(register(cache, "r0")? == value(post, OUTPUT)? && register(cache, "r1")? == 0x400, "retained cache arguments differ")?;
        let (constructor, _) = snapshot(observation, 0x725fa8)?;
        require(register(constructor, "r0")? == 0x20067 && register(constructor, "r1")? == 0x400f33b0 && register(constructor, "r3")? == 0xdd054,
            "retained task constructor binding differs")?;
        let (endpoint, wrapper_return) = snapshot(observation, 0x69cf38)?;
        require(endpoint.core == entry.core && endpoint.sp == wrapper.sp, "wrapper return core/SP differs")?;
        require(register(endpoint, "r0")? == (-25i32) as u32, "retained constructor failure did not survive wrapper return")?;
        require(!observation.hits.iter().any(|hit| hit.core == entry.core && at(hit) == Ok(0x726024)),
            "later installed service unexpectedly executed")?;
        for name in ["r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11"] {
            require(register(wrapper, name)? == register(endpoint, name)?, "wrapper callee-saved register differs")?;
        }
        require((wrapper.cpsr ^ endpoint.cpsr) & 0x0fff_ffff == 0, "wrapper changed non-NZCV CPSR state")?;
        for sampled in &post.words {
            if [CURSOR, STATE + 12, OUTPUT + 8].contains(&sampled.guest.0) { continue; }
            require(value(post, sampled.guest.0)? == value(wrapper_return, sampled.guest.0)?, "storage, neighbors or descriptor changed before wrapper return")?;
        }
    }
    let (_, before) = snapshot(observation, START)?;
    for address in [STATE - 4, STATE + 12, CURSOR - 4, CURSOR + 4, OUTPUT - 4, OUTPUT + 8, LOCK - 4, LOCK + 4, 0x400f33fc] {
        require(value(before, address)? == value(post, address)?, "surrounding storage or untouched descriptor word changed")?;
    }
    Ok(())
}
pub(super) fn compare(retained: &Path, owned: &Path, analysis: &Path, linked_path: &Path, output: &Path) -> Result<(), String> {
    let original = read(&analysis.join("lz/id4-0061c400.bin"))?;
    let linked = read(linked_path)?;
    require(sha256(&original) == "f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16"
        && linked.len() == 228 && sha256(&linked) == "3f9d7945e3adc43d3e7edbbfd3cbdbece6db66fc99cf0724e7722f36086c1768", "consumer source inputs differ")?;
    let rb = read(&retained.join("task-storage.json"))?;
    let ob = read(&owned.join("task-storage.json"))?;
    let r: Observation = serde_json::from_slice(&rb).map_err(|error| error.to_string())?;
    let o: Observation = serde_json::from_slice(&ob).map_err(|error| error.to_string())?;
    let mut comparison = Comparison { schema: 1, accepted: false, case: r.case, retained_receipt: Digest(sha256(&rb)), owned_receipt: Digest(sha256(&ob)),
        original_source_matches: false, return_value: 0, output: 0, record: [0; 3], cursor: 0, lock: 0, surrounding_words_unchanged: false,
        controls_are_perturbed: r.case != Case::Natural, error: None };
    let result = (|| -> Result<(), String> {
        validate(&r, Role::Retained, &original, &linked)?;
        validate(&o, Role::Owned, &original, &linked)?;
        for (directory, observation) in [(retained, &r), (owned, &o)] {
            let bytes = read(&directory.join("accesses.json"))?;
            require(sha256(&bytes) == observation.accesses_sha256.0
                && sha256(&read(&directory.join("reset.fujimod"))?) == observation.checkpoint_sha256.0,
                "receipt sidecar identity differs")?;
            if observation.case == Case::Natural {
                let accesses = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                for address in [CURSOR, STATE + 12, OUTPUT + 8] { concurrent_lifetime(observation, &accesses, address)?; }
            }
        }
        require(r.case == o.case && r.identity.run.0 == o.identity.run.0 && r.identity.rom.0 == o.identity.rom.0
            && r.identity.observer.0 == o.identity.observer.0 && r.identity.original_image4.0 == o.identity.original_image4.0
            && r.identity.loader_a.0 == o.identity.loader_a.0 && r.identity.loader_b.0 == o.identity.loader_b.0
            && r.identity.linked.0 == o.identity.linked.0, "comparison source identities differ")?;
        comparison.original_source_matches = true;
        let (rh, rs) = snapshot(&r, 0x725e78)?;
        let (oh, os) = snapshot(&o, 0x725e78)?;
        require(register(rh, "r0")? == register(oh, "r0")?, "original/owned statuses differ")?;
        for word in &rs.words { require(value(rs, word.guest.0)? == value(os, word.guest.0)?, "original/owned output, record, cursor, lock or surrounding memory differs")?; }
        if r.case == Case::Natural {
            let (_, re) = snapshot(&r, 0x69cf38)?;
            let (_, oe) = snapshot(&o, 0x69cf38)?;
            for word in &re.words { require(value(re, word.guest.0)? == value(oe, word.guest.0)?, "original/owned endpoint memory differs")?; }
            for address in [0x725e78, 0x725f6c, 0x69cf38] {
                let (_, rs) = snapshot(&r, address)?;
                let (_, os) = snapshot(&o, address)?;
                require(rs.buffer.as_ref().ok_or("retained buffer absent")?.bytes == os.buffer.as_ref().ok_or("owned buffer absent")?.bytes,
                    "original/owned pool bytes differ")?;
            }
        }
        comparison.return_value = register(rh, "r0")?;
        comparison.output = value(rs, OUTPUT)?;
        comparison.record = [value(rs, STATE)?, value(rs, STATE + 4)?, value(rs, STATE + 8)?];
        comparison.cursor = value(rs, CURSOR)?;
        comparison.lock = value(rs, LOCK)?;
        comparison.surrounding_words_unchanged = true;
        Ok(())
    })();
    comparison.accepted = result.is_ok();
    comparison.error = result.as_ref().err().cloned();
    write(output, &serde_json::to_vec_pretty(&comparison).map_err(|error| error.to_string())?)?;
    result
}
