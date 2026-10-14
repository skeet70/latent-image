"""One post-reset 10CC06C(mask4) desired triplet; no activation or data.

The completed GLOBAL/Image resets and older numeric histories remain sealed.
New desired words live in this separate optional physical owner. The stock
driver retains its own software leases and updates Input bit4 afterward.
"""
from copy import deepcopy
import hashlib
import struct

from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R1

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import producer_processing_reset as reset

PROFILE = 'declared-post-prefilter-producer-desired-triplet-unmeasured'
ATTRIBUTE = 'processing_configuration'
WORDS = dict(reset.WORDS)
WORDS[0x17d54e0] = 1
PCS = ((0x9338cc, 0x10cc0b0), (0x9338d8, 0x10cc0b0), (0x933a00, 0x10cc0bc))


def owner(device): return getattr(device, ATTRIBUTE, None)


def _image_anchor(device):
    matches = [d for d in device.board.devices if type(d).__name__ == 'ImageWorkControl']
    if len(matches) != 1:
        raise UnsupportedDevice('Processing desired triplet requires one completed paired Image reset')
    from . import image_processing_reset
    image = image_processing_reset.summary(matches[0])
    if image is None or image['state'] != 'complete' or image['channel'] != 2:
        raise UnsupportedDevice('Processing desired triplet precedes actual paired Image reset')
    return digest(image)


def _prior(device):
    marker = reset.summary(device)
    if marker is None or marker['state'] != 'complete' or marker['word'] != 0x10000000:
        raise UnsupportedDevice('Processing desired triplet requires its completed GLOBAL reset')
    return dict(reset_sha256=digest(marker), image_sha256=_image_anchor(device),
                registers_sha256=digest(device.registers))


def plan(device, block):
    if type(block) is not bytes or len(block) != 0x34:
        raise UnsupportedDevice('Processing desired triplet requires the whole52-byte caller block')
    a, b, c = struct.unpack_from('<3H', block, 0x24)
    vertical, horizontal = struct.unpack_from('<2H', block, 0x2a)
    kind, phase = block[0x2e:0x30]
    input_kind = struct.unpack_from('<I', block, 0x30)[0]
    if (max(a, b, c) >= 0x2000 or max(vertical, horizontal) > 15
            or kind > 5 or phase > 11 or input_kind not in (0, 1)):
        raise UnsupportedDevice('Processing desired triplet leaves the stock13/4/3-bit parameter domains')
    return ((device.GLOBAL + 4, a << 16 | b), (device.GLOBAL + 8, c << 16),
            (device.GLOBAL + 0xc, vertical << 16 | horizontal << 20 | kind | phase << 3))


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k not in ('seal', 'caller_block')})


def arm(device, profile=PROFILE, *, caller_address=None):
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or not device._idle() or device.busy):
        raise UnsupportedDevice('Processing desired triplet requires its explicit fresh passive profile')
    prior = _prior(device)
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    if any(read(address) != value for address, value in WORDS.items()):
        raise UnsupportedDevice('Processing desired triplet requires the actual cached mode1 owner')
    if caller_address is None:
        core = device.board.cpus[0]
        if (int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != PCS[0]:
            raise UnsupportedDevice('Processing desired triplet admission requires its first stock store')
        caller_address = int(core.reg_read(UC_ARM_REG_R1)) - 0x24
    if (type(caller_address) is not int or caller_address & 3
            or not 0 <= caller_address < 0x20000000 - 0x34):
        raise UnsupportedDevice('Processing desired triplet caller block is outside aligned RAM')
    block = bytes(device.board.read_memory(caller_address, 0x34, cpu_id=0))
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
            or type(record['step']) is not int or not 0 <= record['step'] <= 3
            or record['state'] != ('complete' if record['step'] == 3 else 'configuring')
            or len(record['accesses']) != record['step']
            or (record['closed_tick'] is not None) != (record['step'] == 3)):
        raise UnsupportedDevice('Processing desired triplet changed its sealed caller/whole-driver contract')
    previous = record['armed_tick']
    words = {}
    for index, event in enumerate(record['accesses']):
        if (type(event) is not tuple or len(event) != 4 or event[0] != 0
                or event[1:3] != record['plan'][index]
                or type(event[3]) is not int or not previous <= event[3] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Processing desired triplet lost its ordered CPU0 stores')
        previous = event[3]; words[event[1]] = event[2]
    if words != record['words'] or record['closed_tick'] is not None and record['closed_tick'] != previous:
        raise UnsupportedDevice('Processing desired triplet physical words changed')
    reset._route(device)
    if full and record['prior'] != _prior(device):
        raise UnsupportedDevice('Processing desired triplet altered its completed reset predecessors')
    return record


def access(device, operation, cpu, address, size, value=None):
    record = validate(device)
    if (record is None or record['state'] == 'complete' or type(cpu) is not int or cpu != 0
            or type(size) is not int or size != 4 or operation != 'write'
            or type(value) is not int or (address, value) != record['plan'][record['step']]):
        raise UnsupportedDevice('Processing desired triplet excludes reordered writes, reads and fresh activation')
    core = device.board.cpus[0]
    if (int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != PCS[record['step']]:
        raise UnsupportedDevice('Processing desired triplet requires the unchanged whole10CC06C driver')
    if bytes(device.board.read_memory(record['caller_address'], 0x34, cpu_id=0)) != record['caller_block']:
        raise UnsupportedDevice('Processing desired triplet caller changed during its transaction')
    if not record['accesses']: validate(device, full=True)
    record['words'][address] = value
    record['accesses'].append((cpu, address, value, device.board.virtual_ticks))
    record['step'] += 1
    if record['step'] == 3: record.update(state='complete', closed_tick=device.board.virtual_ticks)
    _seal(record)


def summary(device):
    record = validate(device, full=True)
    return deepcopy({k: v for k, v in record.items() if k not in ('seal', 'caller_block')}) if record is not None else None
