use super::*;

struct Call<'a> { before: &'a Sample, target: Guest, continuation: Guest, entered: bool }
struct Receive<'a> {
    entry: &'a Sample, destination: Guest, call: Option<Call<'a>>, status: Option<u32>,
    packet: Option<Guest>, payload: Vec<u8>, copied: bool, release: bool, released: bool,
}
fn bytes(views: &[Words], address: u32, length: usize) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::with_capacity(length);
    for offset in 0..length {
        let address = address.checked_add(offset as u32).ok_or_else(|| refusal(Refusal::QueueProvenance, "packet extent overflows"))?;
        let word = projected(views, address & !3)?;
        result.push((word >> (8 * (address & 3))) as u8);
    }
    Ok(result)
}
fn scalar(views: &[Words], address: u32) -> Result<u32, Failure> {
    projected(views, address).map_err(|failure| refusal(Refusal::QueueProvenance, failure.detail))
}
pub(super) enum ServiceSlot { Receive, Release }
pub(super) fn service(sample: &Sample, slot: ServiceSlot) -> Result<Guest, Failure> {
    let domain = KernelDomain::from_selector(sample.registers.r[6])?;
    let executing = KernelDomain::from_identity(sample.identities[usize::from(sample.core)])?;
    require(domain.index() == executing.index(), Refusal::QueueProvenance, "queue kernel selector differs from executing domain")?;
    let root = scalar(&sample.state, 0x96bd20 + 4 * domain.index() as u32)?;
    require(root == [0x4004dae0, 0x400ddaa0][domain.index()], Refusal::QueueProvenance, "queue interface root differs")?;
    let offset = match slot { ServiceSlot::Receive => 0xc8, ServiceSlot::Release => 0xf8 };
    let target = Guest(scalar(&sample.state, root + offset)?);
    require(target.0 & 3 == 0 && target.0 == sample.registers.r[3], Refusal::QueueProvenance, "queue indirect call differs from committed retained slot")?;
    let encoded = 1 | scalar(&sample.state, 0x7cad9c)?.checked_shl(16).ok_or_else(|| refusal(Refusal::QueueProvenance, "queue domain shift overflows"))?;
    require(sample.registers.r[0] == encoded, Refusal::QueueProvenance, "routed queue1 identity differs")?;
    Ok(target)
}
struct Adapter<'a> {
    before: &'a Sample,
    entry: Option<&'a Sample>,
    receive_call: Option<&'a Sample>,
    inner: Option<&'a Sample>,
    exit: Option<&'a Sample>,
    status: Option<u32>,
    copied: bool,
    returned: bool,
}
struct AdapterReceive { entry: SampleId, exit: SampleId }
fn adapters(report: &PumpReport) -> Result<Vec<AdapterReceive>, Failure> {
    let mut active: [Option<Adapter<'_>>; 2] = [None, None];
    let mut linked = Vec::new();
    for sample in &report.samples {
        let pc = physical(sample.pc);
        let core = usize::from(sample.core);
        if matches!(pc, 0xc22fd4 | 0xc2314c) {
            require(active[core].is_none(), Refusal::QueueProvenance, "UI receive reentered before adapter return")?;
            active[core] = Some(Adapter { before: sample, entry: None, receive_call: None, inner: None, exit: None, status: None, copied: false, returned: false });
            continue;
        }
        let Some(adapter) = &mut active[core] else {
            require(!matches!(pc, 0xdb5124 | 0xdb515c | 0xdb5170 | 0xdb5174 | 0xdb5178 | 0xdb51d0 | 0xc22fd8 | 0xc23150), Refusal::QueueProvenance, "UI adapter boundary has no actual caller")?;
            continue;
        };
        if pc == 0xdb5124 {
            require(adapter.entry.is_none() && sample.registers.sp == adapter.before.registers.sp && sample.registers.r == adapter.before.registers.r && physical(sample.registers.lr) == physical(adapter.before.pc) + 4, Refusal::QueueProvenance, "UI adapter entry differs from actual call")?;
            require(scalar(&sample.packet, sample.registers.r[0])? == 0x107, Refusal::QueueProvenance, "actual UI caller did not supply its source-bound blocking selector")?;
            adapter.entry = Some(sample);
        }
        let Some(entry) = adapter.entry else { continue; };
        let frame = entry.registers.sp.0.checked_sub(32).ok_or_else(|| refusal(Refusal::StackContext, "UI adapter frame underflows"))?;
        if pc == 0xdb5170 {
            require(adapter.receive_call.is_none() && adapter.inner.is_none() && sample.registers.sp.0 == frame && sample.registers.r[0] == 1 && sample.registers.r[1] == entry.registers.sp.0.checked_sub(28).ok_or_else(|| refusal(Refusal::StackContext, "UI receive envelope underflows"))? && sample.registers.r[2] == u32::MAX, Refusal::QueueProvenance, "adapter blocking queue1 call changed")?;
            adapter.receive_call = Some(sample);
        }
        if pc == 0x725318 {
            require(adapter.inner.is_none() && sample.registers.sp.0 == frame && physical(sample.registers.lr) == 0xdb5174, Refusal::QueueProvenance, "adapter inner queue1 call is absent or changed")?;
            let call = adapter.receive_call.ok_or_else(|| refusal(Refusal::QueueProvenance, "queue1 entry lacks actual adapter receive call"))?;
            require(sample.registers.r == call.registers.r && sample.registers.sp == call.registers.sp, Refusal::QueueProvenance, "adapter-to-queue1 register transfer differs")?;
            adapter.inner = Some(sample);
        }
        if pc == 0x72541c {
            let inner = adapter.inner.ok_or_else(|| refusal(Refusal::QueueProvenance, "adapter queue1 exit has no entry"))?;
            require(adapter.exit.is_none() && sample.registers.r[11].checked_add(4) == Some(inner.registers.sp.0), Refusal::QueueProvenance, "adapter queue1 exit has another frame")?;
            adapter.exit = Some(sample);
            linked.push(AdapterReceive { entry: inner.id, exit: sample.id });
        }
        if matches!(pc, 0xdb515c | 0xdb5174) {
            let inner = adapter.inner.ok_or_else(|| refusal(Refusal::QueueProvenance, "adapter receive return hides omitted queue1 entry"))?;
            require(adapter.status.is_none() && pc == 0xdb5174 && sample.registers.sp == inner.registers.sp && sample.registers.r[4..12] == inner.registers.r[4..12], Refusal::QueueProvenance, "adapter receive return differs from inner call")?;
            let exit = adapter.exit.ok_or_else(|| refusal(Refusal::QueueProvenance, "adapter return hides omitted queue1 lifecycle"))?;
            require(sample.registers.r[0] == exit.registers.r[0], Refusal::QueueProvenance, "adapter receive return changed inner status")?;
            adapter.status = Some(sample.registers.r[0]);
        }
        if pc == 0xdb5178 {
            require(adapter.status == Some(sample.registers.r[5]) && !adapter.copied, Refusal::QueueProvenance, "adapter copy lacks exactly one inner receive return")?;
            adapter.copied = true;
        }
        if pc == 0xdb51d0 {
            require(adapter.copied && !adapter.returned && adapter.status == Some(sample.registers.r[5]), Refusal::QueueProvenance, "adapter return precedes receive/copy")?;
            adapter.returned = true;
        }
        if matches!(pc, 0xc22fd8 | 0xc23150) {
            require(adapter.returned && pc == physical(adapter.before.pc) + 4 && sample.registers.sp == adapter.before.registers.sp && sample.registers.r[4..12] == adapter.before.registers.r[4..12] && adapter.status == Some(sample.registers.r[0]), Refusal::QueueProvenance, "UI receive handback differs from its adapter lifecycle")?;
            active[core] = None;
        }
    }
    Ok(linked)
}
pub(super) fn check(report: &PumpReport) -> Result<(), Failure> {
    let mut active: Vec<Receive<'_>> = Vec::new();
    let mut successful = 0usize;
    let linked = adapters(report)?;
    let mut completed = BTreeSet::new();
    for sample in &report.samples {
        let pc = physical(sample.pc);
        if pc == 0x725318 {
            require(sample.registers.r[0] == 1 && sample.registers.r[2] == u32::MAX && !active.iter().any(|receive| receive.entry.core == sample.core && receive.entry.registers.sp == sample.registers.sp), Refusal::QueueProvenance, "blocking queue1 entry/context differs")?;
            task_context(sample)?;
            let destination = Guest(sample.registers.r[1]);
            require(sample.packet.iter().any(|view| view.mapping.guest == destination && view.values.len() == 3), Refusal::QueueProvenance, "destination envelope entry absent")?;
            active.push(Receive { entry: sample, destination, call: None, status: None, packet: None, payload: Vec::new(), copied: false, release: false, released: false });
        }
        let Some(index) = active.iter().position(|receive| receive.entry.core == sample.core && (sample.registers.r[11].checked_add(4) == Some(receive.entry.registers.sp.0) || receive.entry.id == sample.id)) else { continue; };
        let receive = &mut active[index];
        if let Some(call) = &mut receive.call {
            if physical(call.target) == pc {
                require(!call.entered && sample.registers.sp == call.before.registers.sp && sample.registers.r == call.before.registers.r && physical(sample.registers.lr) == call.continuation.0, Refusal::QueueProvenance, "queue service entry differs from call boundary")?;
                call.entered = true;
            } else if pc == call.continuation.0 {
                require(call.entered && sample.registers.sp == call.before.registers.sp && sample.registers.r[4..12] == call.before.registers.r[4..12], Refusal::QueueProvenance, "queue service failed to restore caller context")?;
                receive.call = None;
            }
        }
        match pc {
            0x725378 => {
                let target = service(sample, ServiceSlot::Receive)?;
                require(receive.call.is_none() && sample.registers.r[1] == sample.registers.r[11].checked_sub(32).ok_or_else(|| refusal(Refusal::QueueProvenance, "receive slot underflows"))? && sample.registers.r[2] == u32::MAX, Refusal::QueueProvenance, "kernel receive arguments differ")?;
                receive.call = Some(Call { before: sample, target, continuation: Guest(0x72537c), entered: false });
            }
            0x72537c => {
                require(receive.call.is_none(), Refusal::QueueProvenance, "receive continuation lacks actual kernel return")?;
                receive.status = Some(sample.registers.r[0]);
            }
            0x7253c0 => {
                require(receive.status == Some(0), Refusal::QueueProvenance, "success copy follows a nonzero receive")?;
                let slot = sample.registers.r[11].checked_sub(32).ok_or_else(|| refusal(Refusal::QueueProvenance, "packet slot underflows"))?;
                let packet = Guest(scalar(&sample.packet, slot)?);
                let length = scalar(&sample.packet, packet.0 + 8)?;
                require(length <= 76 && sample.registers.r[5] == receive.destination.0, Refusal::QueueProvenance, "received envelope or payload capacity differs")?;
                receive.packet = Some(packet);
                receive.payload = bytes(&sample.packet, packet.0 + 12, length as usize)?;
                if length == 0 { receive.copied = true; }
            }
            0x7253f8 => {
                let packet = receive.packet.ok_or_else(|| refusal(Refusal::QueueProvenance, "copy lacks source packet"))?;
                require(receive.call.is_none() && !receive.copied && sample.registers.r[0] == scalar(&sample.packet, receive.destination.0 + 8)? && sample.registers.r[1] == packet.0 + 12 && sample.registers.r[2] as usize == receive.payload.len(), Refusal::QueueProvenance, "packet copy arguments differ")?;
                receive.call = Some(Call { before: sample, target: Guest(0x722054), continuation: Guest(0x7253fc), entered: false });
            }
            0x7253fc => {
                require(receive.call.is_none() && receive.status == Some(0), Refusal::QueueProvenance, "packet copy return is absent")?;
                receive.copied = true;
            }
            0x725414 => {
                let target = service(sample, ServiceSlot::Release)?;
                let packet = receive.packet.ok_or_else(|| refusal(Refusal::QueueProvenance, "release lacks received packet"))?;
                require(receive.call.is_none() && receive.copied && !receive.release && sample.registers.r[1] == packet.0, Refusal::QueueProvenance, "packet released before copy, twice or under another identity")?;
                let command = scalar(&sample.packet, packet.0 + 4)?;
                require(scalar(&sample.packet, receive.destination.0)? == command && scalar(&sample.packet, receive.destination.0 + 4)? as usize == receive.payload.len(), Refusal::QueueProvenance, "copied envelope header differs from original packet")?;
                let destination = scalar(&sample.packet, receive.destination.0 + 8)?;
                require(bytes(&sample.packet, destination, receive.payload.len())? == receive.payload, Refusal::QueueProvenance, "copied UI payload differs before packet release")?;
                receive.release = true;
                receive.call = Some(Call { before: sample, target, continuation: Guest(0x725418), entered: false });
            }
            0x725418 => { require(receive.release && receive.call.is_none(), Refusal::QueueProvenance, "packet release lacks its actual return")?; receive.released = true; }
            0x72541c => {
                require(receive.call.is_none() && receive.status.is_some(), Refusal::QueueProvenance, "receive exits with an unfinished service")?;
                let adapter = linked.iter().find(|adapter| adapter.entry == receive.entry.id).ok_or_else(|| refusal(Refusal::QueueProvenance, "queue1 success is unrelated to an actual UI receive"))?;
                require(adapter.exit == sample.id && completed.insert(receive.entry.id.0), Refusal::QueueProvenance, "UI adapter names another queue1 exit")?;
                if receive.status == Some(0) { require(receive.copied && receive.released && sample.registers.r[0] == 0, Refusal::QueueProvenance, "successful receive lacks copy/release/zero return")?; successful += 1; }
                else { require(!receive.release && sample.registers.r[0] == receive.status.unwrap_or(0), Refusal::QueueProvenance, "error receive released an unowned packet or changed status")?; }
                active.remove(index);
            }
            _ => {}
        }
    }
    require(linked.len() == completed.len(), Refusal::QueueProvenance, "actual UI receive omitted its inner queue1 lifecycle")?;
    require(successful >= 2, Refusal::QueueProvenance, "repeated successful queue1 receive/copy/release not witnessed")
}
