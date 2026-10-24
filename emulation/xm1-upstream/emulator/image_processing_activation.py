"""Explicit paired postprocessing Image activation, without image data.

Whole10CAEA0(mask400) clears D8, registers physical44 through the stock
kernel, and enables channel0; mask800 then activates channel1/physical45.
Each fresh route is independently checked;
retired predecessors are validated through a read-only historical route view.
No output, status, interrupt assertion or callback is supplied here.
"""
from copy import copy, deepcopy
import hashlib

from unicorn.arm_const import (UC_ARM_REG_PC, UC_ARM_REG_LR, UC_ARM_REG_R4,
                              UC_ARM_REG_R5, UC_ARM_REG_R6, UC_ARM_REG_R11)
from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import processing_pipeline_reset as reset
from . import processing_passive_configuration as passive

PROFILE = 'declared-postprocessing-image-channel0-activation-unmeasured'
ATTRIBUTE = 'processing_activation'
FIRST = (0x92ba00, 0x92b9b4)
CALLER = 0x15f66c4
CALLER_BYTES = 0x28
CALLBACK = 0x11f5c40
PLAN = (('write', 0xfff120d8, 1), ('read', 0xfff12000, 1),
        ('write', 0xfff12000, 0x10001))
WORDS = dict(reset.WORDS)
WORDS[0x160e4f8] = 0  # Stock1203560 clears the completed passive-reset mask.


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def image_for_producer(producer):
    image = passive.image_for_producer(producer)
    return image if owner(image) is not None else None


def _producer(image):
    producer = reset.producer_for_image(image)
    if producer is None:
        raise UnsupportedDevice('Processing Image activation lacks its paired reset owner')
    return producer


def _route_words(board, channel=0):
    line = 44 + channel
    return [dict(vector=b.config.get(0x400 + line*4),
                 priority=b.config.get(0x800 + line*4),
                 source_type=b.source_config >> line & 1)
            for b in board.interrupts.banks]


def _route(image, record, channel=0):
    banks = image.board.interrupts.banks
    line = 44 + channel
    sibling = 45 - channel
    top = owner(image)
    sibling_selected = (channel == 1 or top is not None and 'channel1' in top)
    for cpu, bank in enumerate(banks):
        if (bank.pending & ((1 << 44) | (1 << 45)) or bank.active in (44, 45)
                or bank.injected in (44, 45)
                or not sibling_selected and bank.enabled & (1 << sibling)
                or any(e.get('line') in (44, 45) for e in bank.active_stack)
                or cpu == 1 and bank.enabled & ((1 << 44) | (1 << 45))):
            raise UnsupportedDevice('Processing Image activation excludes IRQ assertion or another route')
    current = _route_words(image.board, channel)
    if current[1] != record['route'][1]:
        raise UnsupportedDevice('Processing Image activation changed its CPU1 route')
    enabled = bool(banks[0].enabled & (1 << line))
    lease = 0xccc1f0 + channel*0x10
    vector = 0x92a71c if channel == 0 else 0x92a74c
    read = lambda a: int.from_bytes(image.board.read_memory(a, 4, cpu_id=0), 'little')
    if enabled:
        if (record['step'] < 1 or current[0] != dict(vector=vector, priority=10, source_type=1)
                or (read(lease), read(lease+4)) != (CALLBACK, 1)):
            raise UnsupportedDevice('Processing Image activation changed its stock IRQ registration')
    elif record['step'] >= 2:
        raise UnsupportedDevice('Processing Image activation lost its live registered IRQ')
    elif record['step'] == 0:
        if current != record['route'] or read(lease) != 0 or read(lease+4) not in (0, 1):
            raise UnsupportedDevice('Processing Image activation changed its retired pre-registration route')
    elif (current[0]['vector'] not in (record['route'][0]['vector'], vector)
            or current[0]['priority'] not in (record['route'][0]['priority'], 10)
            or current[0]['source_type'] not in (record['route'][0]['source_type'], 1)
            or read(lease) not in (0, CALLBACK) or read(lease+4) != 1):
        raise UnsupportedDevice('Processing Image activation changed partial stock registration')


def historical(device):
    """Read-only predecessor view, excluding only validated fresh IRQ44/45."""
    image = device if type(device).__name__ == 'ImageWorkControl' else image_for_producer(device)
    if image is None or owner(image) is None:
        return device
    validate(image)
    board = copy(device.board)
    views = [copy(d) for d in board.devices]
    board.devices = views
    for view in views:
        view.board = board
        if type(view).__name__ == 'ImageWorkControl' and hasattr(view, ATTRIBUTE):
            delattr(view, ATTRIBUTE)
    controller = next(v for v in views if type(v).__name__ == 'KernelInterrupts')
    controller.banks = [copy(b) for b in device.board.interrupts.banks]
    # Do not change vectors, priorities or any other source. The prior closed
    # owner sees its retired line while the new owner checks the live route.
    retired = (1 << 44) | ((1 << 45) if 'channel1' in owner(image) else 0)
    controller.banks[0].enabled &= ~retired
    controller.banks[0].masked &= ~retired
    board.interrupts = controller
    return views[device.board.devices.index(device)]


def _prior(image):
    producer = _producer(image)
    previous = reset.summary(producer)
    if previous is None or previous['state'] != 'complete' or previous['step'] != 74:
        raise UnsupportedDevice('Processing Image activation precedes the whole paired passive reset')
    return digest(previous)


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items()
                            if k not in ('seal', 'caller_block', 'channel1')})


def arm(image, profile=PROFILE, *, channel=0, caller_address=None):
    if (profile != PROFILE or type(channel) is not int or channel not in (0, 1)
            or channel == 0 and owner(image) is not None
            or channel == 1 and (owner(image) is None or 'channel1' in owner(image))
            or image.board.failure is not None
            or image.busy or image.dma_busy or image.phase_busy or image.pending_data is not None):
        raise UnsupportedDevice('Processing Image activation requires its explicit quiet single owner')
    producer = _producer(image)
    read = lambda a: int.from_bytes(image.board.read_memory(a, 4, cpu_id=0), 'little')
    if (any(read(a) != v for a, v in WORDS.items()) or producer.busy or not producer._idle()
            or read(0x15f66a4) != 1 or read(0x15f66a8) != 0x131db24):
        raise UnsupportedDevice('Processing Image activation changed its cleared-mask caller owner')
    if channel == 0:
        reset._route(producer)
        prior = _prior(image)
    else:
        first = validate(image, full=True)
        if first['step'] != 3:
            raise UnsupportedDevice('Processing Image channel1 precedes the complete actual channel0')
        prior = _first_anchor(first)
    if caller_address is None:
        cpu = image.board.cpus[0]
        if (int(cpu.reg_read(UC_ARM_REG_PC)), int(cpu.reg_read(UC_ARM_REG_LR))) != FIRST:
            raise UnsupportedDevice('Processing Image activation requires first stock92B978 D8 access')
        caller_address = CALLER
    if (type(caller_address) is not int or caller_address & 3
            or not 0 < caller_address <= 0x20000000 - CALLER_BYTES):
        raise UnsupportedDevice('Processing Image activation caller leaves aligned RAM')
    block = bytes(image.board.read_memory(caller_address, CALLER_BYTES, cpu_id=0))
    if block != bytes(0x24) + CALLBACK.to_bytes(4, 'little'):
        raise UnsupportedDevice('Processing Image activation changed the observed shared request')
    record = dict(profile=PROFILE, state='activating', step=0,
        armed_tick=image.board.virtual_ticks, closed_tick=None,
        prior_sha256=prior, route=_route_words(image.board, channel),
        caller_address=caller_address, caller_block=block,
        caller_sha256=hashlib.sha256(block).hexdigest(), accesses=[],
        words={image.BASES[channel]: 1,
               image.BASES[channel]+0xd8: image.registers[image.BASES[channel]+0xd8]})
    _seal(record)
    _route(image, record, channel)
    if channel == 0:
        setattr(image, ATTRIBUTE, record)
    else:
        owner(image)['channel1'] = record
    return summary(image)


def _first_anchor(record):
    return digest({k: v for k, v in record.items() if k not in ('caller_block', 'channel1')})


def _plan(channel):
    return tuple((op, address+channel*0x1000, value) for op, address, value in PLAN)


def _validate_one(image, r, channel):
    if (type(r) is not dict or r.get('profile') != PROFILE
            or r.get('seal') != digest({k: v for k, v in r.items()
                                       if k not in ('seal', 'caller_block', 'channel1')})
            or r['caller_block'] != bytes(0x24) + CALLBACK.to_bytes(4, 'little')
            or r['caller_sha256'] != hashlib.sha256(r['caller_block']).hexdigest()
            or type(r['step']) is not int or not 0 <= r['step'] <= 3
            or len(r['accesses']) != r['step']
            or r['state'] != ('enabled-awaiting-input' if r['step'] == 3 else 'activating')
            or (r['closed_tick'] is not None) != (r['step'] == 3)):
        raise UnsupportedDevice('Processing Image activation changed its sealed three-access contract')
    base = image.BASES[channel]
    words = {base: 1, base+0xd8: image.registers[base+0xd8]}
    previous = r['armed_tick']
    for index, (cpu, op, address, value, tick) in enumerate(r['accesses']):
        if cpu != 0 or (op, address, value) != _plan(channel)[index] or not previous <= tick <= image.board.virtual_ticks:
            raise UnsupportedDevice('Processing Image activation lost ordered CPU0 chronology')
        if op == 'write':
            words[address] = value
        previous = tick
    if r['words'] != words or r['closed_tick'] is not None and r['closed_tick'] != previous:
        raise UnsupportedDevice('Processing Image activation changed retained physical words')
    _route(image, r, channel)


def validate(image, *, full=False):
    from . import processing_image_delivery
    if processing_image_delivery.owner(image) is not None:
        processing_image_delivery.validate(image,history=full)
        return validate(processing_image_delivery.historical(image),full=full)
    r = owner(image)
    if r is None:
        return None
    _validate_one(image, r, 0)
    if 'channel1' in r:
        child = r['channel1']
        _validate_one(image, child, 1)
        if (r['step'] != 3 or child['prior_sha256'] != _first_anchor(r)
                or child['armed_tick'] < r['closed_tick'] or 'channel1' in child):
            raise UnsupportedDevice('Processing Image channel1 changed its closed channel0 predecessor')
    if full and _prior(historical(image)) != r['prior_sha256']:
        raise UnsupportedDevice('Processing Image activation changed the closed reset predecessor')
    return r


def access(image, operation, cpu, address, size, value=None):
    top = validate(image)
    channel = 1 if 0xfff13000 <= address < 0xfff14000 else 0
    r = top.get('channel1') if top is not None and channel else top
    if (r is None or r['step'] == 3 or type(cpu) is not int or cpu != 0
            or type(size) is not int or size != 4):
        raise UnsupportedDevice('Processing Image activation excludes output or another active lifetime')
    expected = _plan(channel)[r['step']]
    word = expected[2] if operation == 'read' else value
    if type(word) is not int or (operation, address, word) != expected:
        raise UnsupportedDevice('Processing Image activation requires its next exact D8/control access')
    if r['step'] == 0:
        core = image.board.cpus[0]
        read = lambda a: int.from_bytes(image.board.read_memory(a, 4), 'little')
        frame = read(int(core.reg_read(UC_ARM_REG_R11)) - 4)
        if ((int(core.reg_read(UC_ARM_REG_PC)), int(core.reg_read(UC_ARM_REG_LR))) != FIRST
                or tuple(int(core.reg_read(reg)) for reg in
                         (UC_ARM_REG_R4, UC_ARM_REG_R5, UC_ARM_REG_R6)) != (channel, CALLBACK, channel)
                or not 0 < frame < 0x20000000 or frame & 3
                or read(frame - 16) != (0x400 << channel) or read(frame - 12) != r['caller_address']
                or read(0xccc1f4 + channel*0x10) != 1
                or bytes(image.board.read_memory(r['caller_address'], CALLER_BYTES)) != r['caller_block']):
            raise UnsupportedDevice('Processing Image activation changed whole10CAEA0 caller ABI')
        validate(image, full=True)
    r['accesses'].append((cpu, operation, address, word, image.board.virtual_ticks))
    if operation == 'write':
        r['words'][address] = word
    r['step'] += 1
    if r['step'] == 3:
        r.update(state='enabled-awaiting-input', closed_tick=image.board.virtual_ticks)
    _seal(r)
    return word if operation == 'read' else None


def summary(image):
    r = validate(image, full=True)
    return deepcopy({k: v for k, v in r.items() if k != 'seal'}) if r is not None else None
