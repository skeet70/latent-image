"""One whole10CB5C8(mask701) passive Producer/paired-Image reset.

Input's preceding R/W30 remains with its existing physical owner. This joint
optional record supplies only the following74 passive accesses; earlier LUT,
geometry and enable histories remain immutable. No DMA, result or IRQ exists.
"""
from copy import deepcopy

from unicorn.arm_const import (UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R4,
                              UC_ARM_REG_R5, UC_ARM_REG_R10)
from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import processing_control_configuration as control
from . import processing_passive_configuration as passive

PROFILE = 'declared-post-prefilter-preprocessing-paired-passive-reset-unmeasured'
ATTRIBUTE = 'processing_pipeline_reset'
FIRST = (0x9332ec, 0x10ca898)
WORDS = dict(control.WORDS)
WORDS[0x160e4f4] = 0


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def producer_for_image(image):
    matches = [d for d in image.board.devices
               if type(d).__name__ == 'ProducerControl' and owner(d) is not None]
    if len(matches) > 1:
        raise UnsupportedDevice('Pipeline passive reset has multiple physical Producer owners')
    return matches[0] if matches else None


def _route(device):
    from . import producer_processing_reset, image_processing_reset
    producer_processing_reset._route(device)
    image_processing_reset._route(passive.image_for_producer(device))


def _live(device):
    read = lambda a: int.from_bytes(device.board.read_memory(a, 4, cpu_id=0), 'little')
    image = passive.image_for_producer(device)
    inputs = [d for d in device.board.devices if type(d).__name__ == 'InputProcessingControl']
    if (any(read(a) != v for a, v in WORDS.items())
            or len(inputs) != 1 or inputs[0].registers[0x30] != 0
            or device.busy or not device._idle() or image.busy or image.dma_busy
            or image.phase_busy or image.pending_data is not None):
        raise UnsupportedDevice('Pipeline passive reset changed its quiet actual mask701 owner')
    _route(device)


def _prior(device):
    previous = control.summary(device)
    if previous is None or previous['state'] != 'complete' or previous['step'] != 8:
        raise UnsupportedDevice('Pipeline passive reset precedes completed mode/enable control')
    image = passive.image_for_producer(device)
    prior = passive.owner(device)
    addresses = {b + offset for b in image.BASES
                 for _, offset in image.IDLE_RESET_SEQUENCE}
    initial = {a: prior['words'][a] for a in sorted(addresses)}
    initial[device.GLOBAL] = previous['word']
    return dict(control_sha256=digest(previous), initial=initial)


def plan(device, initial):
    image = passive.image_for_producer(device)
    addresses = {b + offset for b in image.BASES
                 for _, offset in image.IDLE_RESET_SEQUENCE} | {device.GLOBAL}
    if (type(initial) is not dict or set(initial) != addresses
            or any(type(v) is not int or not 0 <= v <= 0xffffffff for v in initial.values())):
        raise UnsupportedDevice('Pipeline passive reset changed its physical field set')
    words = dict(initial)
    events = [('read', device.GLOBAL, words[device.GLOBAL]),
              ('write', device.GLOBAL, words[device.GLOBAL] & ~3)]
    for base in image.BASES:
        for operation, offset in image.IDLE_RESET_SEQUENCE:
            address = base + offset
            if operation == 'read':
                value = words[address]
            elif offset == 0x10:
                value = 0x10001000
            elif offset == 0x1c:
                value = 0
            elif offset == 0x20:
                # Whole92AF8C extracts the retained4/6-bit phase pair;
                # whole92AE94 rewrites it while zeroing the coordinate pair.
                value = words[address] & 0x003f000f
            else:
                value = 0x10000
            events.append((operation, address, value))
            if operation == 'write':
                words[address] = value
    if len(events) != 74:
        raise UnsupportedDevice('Pipeline passive reset lost its whole74-access shape')
    return tuple(events)


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k != 'seal'})


def arm(device, profile=PROFILE):
    if profile != PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Pipeline passive reset requires explicit single-lifetime admission')
    _live(device)
    prior = _prior(device)
    record = dict(profile=PROFILE, state='resetting', step=0,
        armed_tick=device.board.virtual_ticks, closed_tick=None, mask=0x701,
        prior=prior, prior_sha256=digest(prior), plan=plan(device, prior['initial']),
        accesses=[], words=dict(prior['initial']))
    _seal(record)
    setattr(device, ATTRIBUTE, record)
    return summary(device)


def validate(device, *, full=False):
    r = owner(device)
    if r is None:
        return None
    if (type(r) is not dict or r.get('profile') != PROFILE or r.get('mask') != 0x701
            or r.get('seal') != digest({k: v for k, v in r.items() if k != 'seal'})
            or r['prior_sha256'] != digest(r['prior'])
            or r['plan'] != plan(device, r['prior']['initial'])
            or type(r['step']) is not int or not 0 <= r['step'] <= 74
            or len(r['accesses']) != r['step']
            or r['state'] != ('complete' if r['step'] == 74 else 'resetting')
            or (r['closed_tick'] is not None) != (r['step'] == 74)):
        raise UnsupportedDevice('Pipeline passive reset changed its sealed whole-driver contract')
    words = dict(r['prior']['initial'])
    previous = r['armed_tick']
    for index, (cpu, operation, address, value, tick) in enumerate(r['accesses']):
        if (cpu != 0 or (operation, address, value) != r['plan'][index]
                or type(tick) is not int or not previous <= tick <= device.board.virtual_ticks):
            raise UnsupportedDevice('Pipeline passive reset lost ordered CPU0 chronology')
        if operation == 'write':
            words[address] = value
        previous = tick
    if words != r['words'] or r['closed_tick'] is not None and r['closed_tick'] != previous:
        raise UnsupportedDevice('Pipeline passive reset changed its physical words')
    _route(device)
    if full and _prior(device) != r['prior']:
        raise UnsupportedDevice('Pipeline passive reset changed its completed control predecessor')
    return r


def access(device, operation, cpu, address, size, value=None):
    r = validate(device)
    if (r is None or r['state'] == 'complete' or type(cpu) is not int or cpu != 0
            or type(size) is not int or size != 4 or operation not in ('read', 'write')):
        raise UnsupportedDevice('Pipeline passive reset excludes additional activation/data lifetimes')
    expected = r['plan'][r['step']]
    word = expected[2] if operation == 'read' else value
    if type(word) is not int or (operation, address, word) != expected:
        raise UnsupportedDevice('Pipeline passive reset requires its next exact passive access')
    if not r['accesses']:
        c = device.board.cpus[0]
        if ((int(c.reg_read(UC_ARM_REG_PC)), int(c.reg_read(UC_ARM_REG_LR))) != FIRST
                or int(c.reg_read(UC_ARM_REG_R4)) != 0x701
                or int(c.reg_read(UC_ARM_REG_R5)) != 0 or int(c.reg_read(UC_ARM_REG_R10)) != 0):
            raise UnsupportedDevice('Pipeline passive reset changed the whole10CB5C8 caller')
        _live(device)
        validate(device, full=True)
    r['accesses'].append((cpu, operation, address, word, device.board.virtual_ticks))
    if operation == 'write':
        r['words'][address] = word
    r['step'] += 1
    if r['step'] == 74:
        r.update(state='complete', closed_tick=device.board.virtual_ticks)
    _seal(r)
    return word if operation == 'read' else None


def summary(device):
    r = validate(device, full=True)
    return deepcopy({k: v for k, v in r.items() if k != 'seal'}) if r is not None else None
