"""One post-still paired quiet Image reset, without a new image lifetime.

The optional owner retains physical control/D4 words separately so the
completed still DMA and AF histories remain immutable. Only the ordinary
92A8D0 read/clear/poll/D4 sequence and declared bit0 delay are supplied.
"""
from copy import deepcopy

from unicorn.arm_const import UC_ARM_REG_LR, UC_ARM_REG_PC

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import afs_still_image_delivery as delivery

PROFILE = 'declared-post-still-paired-image-quiet-reset-unmeasured'
ATTRIBUTE = 'processing_reset'
MAX_POLLS = 4096


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _prior(device):
    data = delivery.summary(device)
    if (data is None or data['state'] != 'acknowledged'
            or data['retirement']['phase'] != 'completed'):
        raise UnsupportedDevice('Processing Image reset requires the closed finite still output')
    return digest(dict(data=data, registers=device.registers, banks=device.banks,
                       history=device.data_history, writes=device.writes))


def _route(device):
    if any(b.enabled & ((1 << 44) | (1 << 45))
           or b.pending & ((1 << 44) | (1 << 45))
           or b.active in (44, 45) or b.injected in (44, 45)
           or any(e.get('line') in (44, 45) for e in b.active_stack)
           for b in device.board.interrupts.banks):
        raise UnsupportedDevice('Processing Image reset requires retired drained IRQ44/45')


def _seal(record):
    record['seal'] = digest({k: v for k, v in record.items() if k != 'seal'})


def arm(device, profile=PROFILE):
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or device.stop_profile != 'stock-idle-image-banks-bit0-stop-ack-unmeasured'
            or device.busy or device.dma_busy or device.pending_data is not None
            or any(device.registers[b] != 1 for b in device.BASES)):
        raise UnsupportedDevice('Processing Image reset requires its explicit closed quiet owner')
    _route(device)
    prior = _prior(device)
    words = {a: device.registers[a] for b in device.BASES for a in (b, b + 0xd4)}
    record = dict(profile=PROFILE, state='awaiting-read', channel=0,
        armed_tick=device.board.virtual_ticks, prior_sha256=prior,
        initial_words=dict(words), words=words, accesses=[], acknowledgments=[],
        requested_tick=None, due_tick=None, closed_tick=None)
    _seal(record)
    setattr(device, ATTRIBUTE, record)
    device.phase_busy = True
    return summary(device)


def validate(device, *, full=False):
    r = owner(device)
    if r is None:
        return None
    if (type(r) is not dict or r.get('profile') != PROFILE
            or r.get('seal') != digest({k: v for k, v in r.items() if k != 'seal'})
            or len(r['accesses']) > 2 * (4 + MAX_POLLS)
            or len(r['acknowledgments']) > 2):
        raise UnsupportedDevice('Processing Image reset changed its sealed bounded history')
    phase, channel = 'awaiting-read', 0
    words = dict(r['initial_words'])
    previous = r['armed_tick']
    acknowledgments = iter(r['acknowledgments'])
    ack = next(acknowledgments, None)
    polls = 0
    for index, (op, address, value, tick) in enumerate(r['accesses']):
        if channel > 1 or type(tick) is not int or not previous <= tick <= device.board.virtual_ticks:
            raise UnsupportedDevice('Processing Image reset lost its transaction chronology')
        base = device.BASES[channel]
        if phase == 'awaiting-read':
            expected = ('read', base, 1)
            phase = 'awaiting-clear'
        elif phase == 'awaiting-clear':
            expected = ('write', base, 0)
            words[base] = 0
            phase = 'polling'
        elif phase == 'polling':
            ready = ack is not None and ack[0] == channel and index >= ack[2]
            if ready and tick < ack[1] or polls >= MAX_POLLS:
                raise UnsupportedDevice('Processing Image reset polling response/bound changed')
            expected = ('read', base, int(ready))
            polls += 1
            if ready:
                words[base] = 1
                phase = 'awaiting-ack'
                ack = next(acknowledgments, None)
        else:
            expected = ('write', base + 0xd4, device.STOP_ACK)
            words[base + 0xd4] = device.STOP_ACK
            channel += 1
            phase = 'complete' if channel == 2 else 'awaiting-read'
            polls = 0
        if (op, address, value) != expected:
            raise UnsupportedDevice('Processing Image reset differs from whole92A8D0')
        previous = tick
    if phase == 'polling' and ack is not None and ack[0] == channel:
        words[device.BASES[channel]] = 1
    if ((r['state'], r['channel']) != (phase, channel) or r['words'] != words
            or (phase == 'complete') != (r['closed_tick'] is not None)
            or r['closed_tick'] is not None and r['closed_tick'] != previous
            or getattr(device, 'phase_busy', None) is not (phase != 'complete')
            or r['due_tick'] is not None and r['due_tick'] != r['requested_tick'] + device.stop_ticks):
        raise UnsupportedDevice('Processing Image reset physical words or phase changed')
    _route(device)
    if full and _prior(device) != r['prior_sha256']:
        raise UnsupportedDevice('Processing Image reset changed its completed DMA predecessor')
    return r


def access(device, operation, cpu, address, size, value=None):
    r = validate(device)
    if (r is None or type(cpu) is not int or cpu != 0 or type(size) is not int or size != 4
            or address not in r['words'] or r['state'] == 'complete'):
        raise UnsupportedDevice('Processing Image reset excludes a new active lifetime or unknown access')
    base = device.BASES[r['channel']]
    word = r['words'][address] if operation == 'read' else value
    phase = r['state']
    expected = (('read', base, 1) if phase == 'awaiting-read' else
                ('write', base, 0) if phase == 'awaiting-clear' else
                ('read', base, int(r['due_tick'] is None)) if phase == 'polling' else
                ('write', base + 0xd4, device.STOP_ACK))
    if (operation, address, word) != expected:
        raise UnsupportedDevice('Processing Image reset requires its next paired quiet transaction')
    if not r['accesses']:
        c = device.board.cpus[0]
        if (int(c.reg_read(UC_ARM_REG_PC)), int(c.reg_read(UC_ARM_REG_LR))) != (0x92a8ec, 0x10cad08):
            raise UnsupportedDevice('Processing Image reset requires its whole stock10CAB9C caller')
    if len(r['accesses']) >= 2 * (4 + MAX_POLLS):
        raise UnsupportedDevice('Processing Image reset exhausted its bounded polling')
    now = device.board.virtual_ticks
    r['accesses'].append((operation, address, word, now))
    if phase == 'awaiting-read':
        r['state'] = 'awaiting-clear'
    elif phase == 'awaiting-clear':
        r.update(state='polling', requested_tick=now, due_tick=now + device.stop_ticks)
        r['words'][base] = 0
    elif phase == 'polling' and word == 1:
        r['state'] = 'awaiting-ack'
    elif phase == 'awaiting-ack':
        r['words'][base + 0xd4] = word
        r['channel'] += 1
        r['state'] = 'complete' if r['channel'] == 2 else 'awaiting-read'
        if r['state'] == 'complete':
            r['closed_tick'] = now
            device.phase_busy = False
    _seal(r)
    return word if operation == 'read' else None


def tick(device, board):
    r = owner(device)
    if r is None or r['due_tick'] is None or board.virtual_ticks < r['due_tick']:
        return
    validate(device, full=True)
    r['words'][device.BASES[r['channel']]] = 1
    r['acknowledgments'].append((r['channel'], board.virtual_ticks, len(r['accesses'])))
    r['due_tick'] = None
    _seal(r)


def due(device):
    r = owner(device)
    return r['due_tick'] if r is not None else None


def summary(device):
    return deepcopy(validate(device, full=True))
