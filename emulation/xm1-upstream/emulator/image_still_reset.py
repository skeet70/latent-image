"""One explicitly armed quiet image-bank reset after the stock still setup.

Stock92A8D0 clears/polls bit0 on both banks and writes D4=33FF1. Only
the existing declared idle-stop delay is supplied; no pixels or IRQs.
"""
from copy import copy, deepcopy

from .hardware import UnsupportedDevice

PROFILE = 'declared-still-image-banks-quiet-reset-after-af-unmeasured'


def owner(device):
    return getattr(device, 'still_reset', None)


def historical(device):
    record = owner(device)
    if record is None:
        return device
    proxy = copy(device)
    del proxy.still_reset
    proxy.registers = dict(record['prior_words'])
    return proxy


def _prior_hash(device):
    return device._afs_digest(dict(banks=device.banks, data_history=device.data_history))


def arm(device, profile):
    if profile != PROFILE or owner(device) is not None or device.board.failure is not None:
        raise UnsupportedDevice('Still image reset requires explicit one-shot admission')
    device._validate_closed_reset()
    device._idle_quiescent()
    read = lambda address, n=4: int.from_bytes(device.board.read_memory(address, n, cpu_id=0), 'little')
    sources = [d for d in device.board.devices if type(d).__name__ == 'AfsFrameSource']
    producers = [d for d in device.board.devices if type(d).__name__ == 'ProducerControl']
    if len(sources) != 1 or len(producers) != 1:
        raise UnsupportedDevice('Still image reset requires unique completed sensor/numeric owners')
    clock = sources[0].still_qualifying_clock_summary()
    anchor = producers[0].still_capture_source_anchor()
    if (device._afs_stop_marker().get('state') != 'completed'
            or clock is None or clock['state'] != 'acknowledged' or clock['eoi_tick'] is None
            or device.stop_profile != 'stock-idle-image-banks-bit0-stop-ack-unmeasured'
            or any(read(a, n) != v for a, n, v in ((0x15d502c,4,6),(0x15d5088,4,2),
                (0x15d5090,1,1),(0x15d5091,1,0),(0x15d50b4,4,0),
                (0x15f63a4,4,3),(0x15f63a8,4,1),(0x15f64a4,4,0),(0x15f68a4,4,1)))
            or any(device.registers[base] != 1 for base in device.BASES)
            or any(b.enabled & ((1 << 44) | (1 << 45)) or b.pending & ((1 << 44) | (1 << 45))
                   or b.active in (44,45) or b.injected in (44,45) for b in device.board.interrupts.banks)):
        raise UnsupportedDevice('Still image reset differs from the actual private6 quiet caller')
    device.still_reset = dict(profile=PROFILE, prior_words=dict(device.registers), prior_hash=_prior_hash(device),
        anchor=anchor, clock_sha256=device._afs_digest(clock), armed_tick=device.board.virtual_ticks,
        accesses=[], channel=0, phase='read', due_tick=None, acknowledgments=[])
    return summary(device)


def validate(device, *, history=True):
    record = owner(device)
    if record is None:
        return None
    if 'configuration' in record:
        from . import image_still_configuration as configuration
        configuration.validate(device,history=history)
        return validate(configuration.historical(device),history=history)
    if record['profile'] != PROFILE or record['prior_hash'] != _prior_hash(device):
        raise UnsupportedDevice('Still image reset changed its retained AF image history')
    # The full predecessor was validated at admission and is revalidated at
    # snapshot/summary boundaries. Ordinary polls retain its complete hash
    # and replay only this bounded new transaction.
    if history:
        historical(device)._validate_closed_reset()
    words = dict(record['prior_words'])
    channel, phase, previous = 0, 'read', record['armed_tick']
    acknowledgments = iter(record['acknowledgments'])
    ack = next(acknowledgments, None)
    for index, event in enumerate(record['accesses']):
        op, cpu, address, value, tick = event
        if type(tick) is not int or not previous <= tick <= device.board.virtual_ticks or cpu != 0 or channel > 1:
            raise UnsupportedDevice('Still image reset lost its typed transaction chronology')
        base = device.BASES[channel]
        if phase == 'read':
            expected = ('read', base, 1); phase = 'zero'
        elif phase == 'zero':
            expected = ('write', base, 0); words[base] = 0; phase = 'poll'
        elif phase == 'poll':
            ready = ack is not None and ack[0] == channel and index >= ack[2]
            if ready and tick < ack[1]:
                raise UnsupportedDevice('Still image reset poll precedes its acknowledgment')
            expected = ('read', base, int(ready))
            if ready:
                words[base] = 1; phase = 'ack'
                ack = next(acknowledgments, None)
        else:
            expected = ('write', base + 0xd4, device.STOP_ACK)
            words[base + 0xd4] = device.STOP_ACK
            channel += 1; phase = 'read' if channel < 2 else 'completed'
        if (op, address, value) != expected:
            raise UnsupportedDevice('Still image reset differs from the whole92A8D0 transaction')
        previous = tick
    if record['phase'] == 'poll' and ack is not None:
        if ack[0] != channel:
            raise UnsupportedDevice('Still image reset ACK belongs to another bank')
        words[device.BASES[channel]] = 1
    if (record['channel'], record['phase']) != (channel, phase) or device.registers != words:
        raise UnsupportedDevice('Still image reset current register/phase projection differs')
    return record


def access(device, operation, core, channel, offset, value=None):
    record = validate(device,history=False)
    if record is None:
        return False
    if record['phase'] == 'completed':
        raise UnsupportedDevice('Still image reset completed; next image lifetime needs separate admission')
    word = device.registers[device.BASES[channel] + offset] if operation == 'read' else value
    phase = record['phase']
    expected = ('read', record['channel'], 0, 1) if phase == 'read' else (
        ('write', record['channel'], 0, 0) if phase == 'zero' else (
        ('read', record['channel'], 0, int(record['due_tick'] is None)) if phase == 'poll'
        else ('write', record['channel'], 0xd4, device.STOP_ACK)))
    if type(core) is not int or core != 0 or (operation, channel, offset, word) != expected:
        raise UnsupportedDevice('Still image reset requires its exact next quiet bank access')
    now = device.board.virtual_ticks
    if len(record['accesses']) >= 2 * (4 + device.CLOSED_RESET_POLL_LIMIT):
        raise UnsupportedDevice('Still image reset exhausted bounded idle polling')
    record['accesses'].append((operation,core,device.BASES[channel]+offset,word,now))
    if phase == 'read':
        record['phase'] = 'zero'
    elif phase == 'zero':
        record.update(phase='poll',due_tick=now+device.stop_ticks)
    elif phase == 'poll' and word == 1:
        record['phase'] = 'ack'
    elif phase == 'ack':
        record['channel'] += 1
        record['phase'] = 'read' if record['channel'] < 2 else 'completed'
    if operation == 'write':
        address = device.BASES[channel] + offset
        old = device.registers[address];device.registers[address] = word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old),tick=now))
    return True


def tick(device, board):
    record = owner(device)
    if record is None or record['due_tick'] is None or board.virtual_ticks < record['due_tick']:
        return
    validate(device,history=False)
    device.registers[device.BASES[record['channel']]] = 1
    record['acknowledgments'].append((record['channel'],board.virtual_ticks,len(record['accesses'])))
    record['due_tick'] = None


def summary(device):
    record = validate(device)
    return deepcopy(record) if record is not None else None


def due(device):
    record = owner(device)
    return record['due_tick'] if record is not None else None
