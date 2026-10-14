"""One post-PFP 10CBB44(mask180C) Image geometry transaction; no data.

The completed paired quiet reset and earlier AF/still DMA remain sealed.
Both banks retain new desired words in a separate optional physical owner.
Stock Input geometry setters remain the responsibility of the Input model.
"""
from copy import deepcopy
import hashlib
import struct

from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R4, UC_ARM_REG_R5

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import image_processing_reset as reset

PROFILE = 'declared-post-prefilter-paired-image-desired-geometry-unmeasured'
ATTRIBUTE = 'processing_configuration'
WORDS = {0x15e32d0: 4, 0x15d502c: 0, 0x15f63a4: 3, 0x15f63a8: 0,
    0x15f64a4: 0, 0x15f68a4: 0, 0x15f68a8: 0, 0x15f6bf4: 0,
    0x15f6e50: 1, 0x173d308: 1, 0x17d54dc: 1, 0x17d54e0: 1,
    0x17d54ec: 0, 0x152b724: 6, 0x160e4f8: 0x701, 0x160e4e4: 0x180c}
FIRST = (0x92aa90, 0x10cbbc8)
CALLER_BYTES = 0x198


def owner(device): return getattr(device, ATTRIBUTE, None)


def _producer_anchor(device):
    matches = [d for d in device.board.devices if type(d).__name__ == 'ProducerControl']
    if len(matches) != 1:
        raise UnsupportedDevice('Processing Image geometry requires one completed Producer triplet')
    from . import producer_processing_configuration
    producer = producer_processing_configuration.summary(matches[0])
    if producer is None or producer['state'] != 'complete':
        raise UnsupportedDevice('Processing Image geometry precedes the actual Producer triplet')
    return digest(producer)


def _prior(device):
    marker = reset.summary(device)
    if marker is None or marker['state'] != 'complete' or marker['channel'] != 2:
        raise UnsupportedDevice('Processing Image geometry requires its completed paired quiet reset')
    return dict(reset_sha256=digest(marker), producer_sha256=_producer_anchor(device),
                registers_sha256=digest(device.registers))


def plan(device, block):
    if type(block) is not bytes or len(block) != CALLER_BYTES:
        raise UnsupportedDevice('Processing Image geometry requires the whole408-byte caller block')
    result = []
    for channel, start in enumerate((8, 0x40)):
        u16 = lambda offset: struct.unpack_from('<H', block, start + offset)[0]
        u32 = lambda offset: struct.unpack_from('<I', block, start + offset)[0]
        if (not all(1 <= u16(o) <= 0x1000 for o in (0, 2))
                or u32(4) not in (0, 1) or max(u32(12), u32(16)) > 0x1fff
                or not all(8 <= u16(o) <= 0x800 and u16(o) % 8 == 0 for o in (20, 26))
                or not 1 <= u16(22) <= 0x800 or u16(24) > 0x7f8 or u16(24) % 8
                or u16(32) > 0xff or u32(36) > 3 or u32(40) > 4
                or u32(44) > 0x3ffffff0 or u32(44) % 16
                or u16(48) > 0x1ffc or u16(48) % 8
                or max(block[start + 52:start + 54]) > 15):
            raise UnsupportedDevice('Processing Image geometry leaves the stock setter domains')
        values = (u16(0)-1 | (u16(2)-1)<<16,
            (u32(4)==1)<<31 | u16(8), u16(10), u32(12) | u32(16)<<16,
            u16(20)-1 | (u16(22)-1)<<16, u16(24)<<16 | u16(26)-1,
            (u32(28)==1)<<24 | u16(32)<<16 | u32(36)<<4 | u32(40),
            u32(44), u16(48), block[start + 52])
        # The direction flag at +28 is the stock explicit Boolean domain.
        if u32(28) not in (0, 1) or any(not device._config_valid(offset, value)
                for offset, value in zip(device.CONFIG_ORDER, values)):
            raise UnsupportedDevice('Processing Image geometry produced an invalid desired word')
        result.extend((device.BASES[channel] + offset, value)
                      for offset, value in zip(device.CONFIG_ORDER, values))
    # Mask180C also executes921B14 unchanged after the twenty Image stores.
    left, width, top, height = struct.unpack_from('<4H', block, 0x190)
    if left > 0x3fff or top > 0x3fff or not 1 <= width <= 0x1000 or not 1 <= height <= 0x4000:
        raise UnsupportedDevice('Processing Image geometry leaves the accompanying Input domain')
    return tuple(result)


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k not in ('seal', 'caller_block')})


def arm(device, profile=PROFILE, *, caller_address=None):
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or device.busy or device.dma_busy or device.pending_data is not None):
        raise UnsupportedDevice('Processing Image geometry requires its explicit fresh passive profile')
    prior = _prior(device)
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    if any(read(address) != value for address, value in WORDS.items()):
        raise UnsupportedDevice('Processing Image geometry requires the actual stopped processing owner')
    if caller_address is None:
        core = device.board.cpus[0]
        if (int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != FIRST or int(core.reg_read(UC_ARM_REG_R4)) != 0x180c:
            raise UnsupportedDevice('Processing Image geometry admission requires its first stock store')
        caller_address = int(core.reg_read(UC_ARM_REG_R5))
    if (type(caller_address) is not int or caller_address & 3
            or not 0 <= caller_address < 0x20000000 - CALLER_BYTES):
        raise UnsupportedDevice('Processing Image geometry caller block is outside aligned RAM')
    block = bytes(device.board.read_memory(caller_address, CALLER_BYTES, cpu_id=0))
    record = dict(profile=PROFILE, state='configuring', step=0, armed_tick=device.board.virtual_ticks,
        closed_tick=None, caller_address=caller_address, caller_block=block, caller_sha256=hashlib.sha256(block).hexdigest(),
        plan=plan(device, block), prior=prior, prior_sha256=digest(prior), words={}, accesses=[], seal=None)
    _seal(record); setattr(device, ATTRIBUTE, record)
    return summary(device)


def validate(device, *, full=False):
    record = owner(device)
    if record is None: return None
    if (type(record) is not dict or record.get('profile') != PROFILE
            or record.get('seal') != digest({k: v for k, v in record.items() if k not in ('seal', 'caller_block')})
            or record['caller_sha256'] != hashlib.sha256(record['caller_block']).hexdigest()
            or record['plan'] != plan(device, record['caller_block'])
            or record['prior_sha256'] != digest(record['prior'])
            or type(record['step']) is not int or not 0 <= record['step'] <= 20
            or record['state'] != ('complete' if record['step'] == 20 else 'configuring')
            or len(record['accesses']) != record['step']
            or (record['closed_tick'] is not None) != (record['step'] == 20)):
        raise UnsupportedDevice('Processing Image geometry changed its sealed caller/whole-driver contract')
    previous = record['armed_tick']
    words = {}
    for index, event in enumerate(record['accesses']):
        if (type(event) is not tuple or len(event) != 4 or event[0] != 0
                or event[1:3] != record['plan'][index]
                or type(event[3]) is not int or not previous <= event[3] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Processing Image geometry lost its ordered CPU0 stores')
        previous = event[3]; words[event[1]] = event[2]
    if words != record['words'] or record['closed_tick'] is not None and record['closed_tick'] != previous:
        raise UnsupportedDevice('Processing Image geometry physical words changed')
    reset._route(device)
    if full and record['prior'] != _prior(device):
        raise UnsupportedDevice('Processing Image geometry altered its completed reset predecessors')
    return record


def access(device, operation, cpu, address, size, value=None):
    record = validate(device)
    if (record is None or record['state'] == 'complete' or type(cpu) is not int or cpu != 0
            or type(size) is not int or size != 4 or operation != 'write'
            or type(value) is not int or (address, value) != record['plan'][record['step']]):
        raise UnsupportedDevice('Processing Image geometry excludes reordered writes, reads and fresh activation')
    core = device.board.cpus[0]
    if (int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != FIRST and not record['accesses']:
        raise UnsupportedDevice('Processing Image geometry requires the unchanged whole10CBB44 driver')
    if bytes(device.board.read_memory(record['caller_address'], CALLER_BYTES, cpu_id=0)) != record['caller_block']:
        raise UnsupportedDevice('Processing Image geometry caller changed during its transaction')
    if not record['accesses']:
        if int(core.reg_read(UC_ARM_REG_R4)) != 0x180c:
            raise UnsupportedDevice('Processing Image geometry changed the full mask180C caller')
        validate(device, full=True)
    record['words'][address] = value
    record['accesses'].append((cpu, address, value, device.board.virtual_ticks))
    record['step'] += 1
    if record['step'] == 20: record.update(state='complete', closed_tick=device.board.virtual_ticks)
    _seal(record)


def summary(device):
    record = validate(device, full=True)
    return deepcopy({k: v for k, v in record.items() if k not in ('seal', 'caller_block')}) if record is not None else None
