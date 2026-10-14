"""One stock 14601C0 size-change lifetime after a canceled Face request.

The explicit profile retains mode-zero stores and reinitializes already idle
DMA channels. It supplies no face bytes, detections, callback or interrupt.
"""
from copy import copy, deepcopy

from .hardware import UnsupportedDevice

PROFILE = 'declared-canceled-face-still-size-reconfiguration-unmeasured'
KEY = 'reconfiguration'
CONFIG_ORDER = (0x10, 0x40, 0x44, 0x4c, 0x50, 0x54,
                0x60, 0x68, 0x70, 0x78, 0x64, 0x6c, 0x74, 0x7c,
                *(0x80 + 12*i + j for i in range(16) for j in (0, 4, 8)),
                *(0x88 + 12*i for i in range(16)))
MODE_ZERO = (0x04000444, 0x01e00280, 0x280, 0x4275c000, 0x42765800,
             0x00800100, 0x003800bc, 0x00620004, 0x00010000, 0,
             0x01930191, 0x01fc01fc, 0x0282027f, 0)


def owner(device):
    return device.stop.get(KEY) if isinstance(device.stop, dict) else None


def historical(device):
    job = owner(device)
    view = copy(device)
    view.stop = {k: v for k, v in device.stop.items() if k != KEY}
    view.registers = job['initial_registers'].copy()
    view.dma_states = list(job['initial_dma_states'])
    return view


def _phase(step):
    return ('configuration' if step < 78 else 'release' if step < 86 else
            'initialize' if step < 126 else 'complete')


def _slot(device, step):
    if step < 78:
        return 'write', CONFIG_ORDER[step], None
    if step < 86:
        channel = device.ARM_ORDER[(step - 78)//2]
        return ('read' if step % 2 == 0 else 'write'), 0x804 + 0x80*channel, channel
    channel = device.ARM_ORDER[(step - 86)//10]
    op, local = (('read', 4), ('write', 4), ('read', 0x10), ('read', 4),
                 ('write', 0), ('read', 0x20), ('write', 0x20), ('write', 4),
                 ('write', 0x28), ('write', 0x44))[(step - 86) % 10]
    return op, 0x800 + 0x80*channel + local, channel


def _accept(device, step, words, operation, offset, value):
    op, wanted_offset, channel = _slot(device, step)
    if operation != op or offset != wanted_offset or not device._word(value):
        return False
    if step < 78:
        return (value == MODE_ZERO[step] if step < 14 else
                device._config_valid(offset, value))
    if operation == 'read':
        return value == words[offset] and (offset % 0x80 != 0x10 or value == 0)
    if step < 86:
        return value == words[offset] & ~1
    local = (step - 86) % 10
    if local == 6:
        # 14647FC keeps the selected mode-zero channel selector (four).
        return value == ((words[offset] | 0x3000) & 0x8f8fffff | 0x00400000)
    if local == 9:
        # 145D2D4 preserves each canceled job's fourth software mode word.
        return value == 2
    return value == (0x100 if local == 8 else 0)


def _seal(device, job):
    return device._digest({k: v for k, v in job.items()
                           if k not in ('state_sha256', 'request', 'cancellation')})


def validate(device):
    job = owner(device)
    required = {'profile', 'cpu', 'step', 'phase', 'initial_registers',
                'initial_dma_states', 'predecessor_sha256', 'accesses',
                'state_sha256', 'request', 'requested_tick', 'completed_tick'}
    if (not isinstance(job, dict) or set(job) not in (required, required | {'cancellation'}) or job['profile'] != PROFILE or
            type(job['cpu']) is not int or job['cpu'] != 0 or
            type(job['step']) is not int or not 0 <= job['step'] <= 126 or
            job['phase'] != _phase(job['step']) or
            not isinstance(job['initial_registers'], dict) or
            job['initial_dma_states'] != ['stopped']*4 or
            job['state_sha256'] != _seal(device, job) or
            job['predecessor_sha256'] != device._digest({
                'request': device.request,
                'stop': {k: v for k, v in device.stop.items() if k != KEY}}) or
            not historical(device)._stop_valid()):
        raise UnsupportedDevice('Face still reconfiguration predecessor/state was changed')
    if (not isinstance(job['accesses'], list) or len(job['accesses']) != job['step'] or
            type(job['requested_tick']) is not int or
            not 0 <= job['requested_tick'] <= device.board.virtual_ticks or
            ((job['step'] == 126) != (type(job['completed_tick']) is int)) or
            job['completed_tick'] is not None and not
            job['requested_tick'] <= job['completed_tick'] <= device.board.virtual_ticks):
        raise UnsupportedDevice('Face still reconfiguration timing/access prefix was changed')
    words = job['initial_registers'].copy()
    states = list(job['initial_dma_states'])
    for step, event in enumerate(job['accesses']):
        if (not isinstance(event, dict) or set(event) != {'operation', 'offset', 'value', 'cpu'} or
                event['cpu'] != job['cpu'] or
                not _accept(device, step, words, event['operation'], event['offset'], event['value'])):
            raise UnsupportedDevice('Face still reconfiguration ordered access ledger was changed')
        if event['operation'] == 'write':
            words[event['offset']] = event['value']
            channel = _slot(device, step)[2]
            if 78 <= step < 86:
                states[channel] = 'closed'
            if step >= 86 and (step - 86) % 10 == 9:
                states[channel] = 'configured'
    if job['request'] is None:
        if words != device.registers or states != device.dma_states:
            raise UnsupportedDevice('Face still configuration words changed outside their ledger')
    elif 'cancellation' in job:
        from . import face_still_cancellation
        face_still_cancellation.validate(device)
    else:
        view = _request_view(device)
        expected_states = (['armed-awaiting-result']*4 if
                           view.request.get('phase') == 'armed-awaiting-result' else ['configured']*4)
        if (job['step'] != 126 or not view._request_valid() or
                view.request['initial_registers'] != words or device.dma_states != expected_states):
            raise UnsupportedDevice('Face successor desired request/configuration was changed')
        if any(event.get('cpu') != job['cpu'] for event in view.request['accesses']):
            raise UnsupportedDevice('Face successor request changed its CPU owner')
    return job


def arm(device, profile):
    if profile != PROFILE or owner(device) is not None:
        raise UnsupportedDevice('Unknown or already selected Face still reconfiguration')
    b = device.board
    u = lambda a: int.from_bytes(b.read_memory(a, 4), 'little')
    if (not device._stop_valid() or device.stop['step'] != 26 or
            device.stop['cpu'] != 0 or u(0x41986db0) != 4 or
            u(0x4197f02c) != 2 or u(0x147ca90) != 0x01e00280 or u(0x18d6afc) != 0 or
            any(u(a) != 2 for a in (0x18d1c1c, 0x18d1c34, 0x18d1c4c, 0x18d1c64))):
        raise UnsupportedDevice('Face still reconfiguration needs the actual canceled mode-zero caller')
    predecessor = device._digest({'request': device.request, 'stop': device.stop})
    job = dict(profile=profile, cpu=0, step=0, phase=_phase(0),
               initial_registers=deepcopy(device.registers),
               initial_dma_states=list(device.dma_states), predecessor_sha256=predecessor,
               accesses=[], request=None, requested_tick=b.virtual_ticks, completed_tick=None)
    job['state_sha256'] = _seal(device, job)
    device.stop[KEY] = job
    return deepcopy(validate(device))


def _request_view(device):
    view = copy(device)
    view.stop = None
    view.request = owner(device)['request']
    return view


def access(device, operation, core, address, size, value=None):
    job = validate(device)
    if size != 4 or type(core) is not int or core != job['cpu']:
        raise UnsupportedDevice('Face still reconfiguration requires its CPU0 word owner')
    offset = address - device.BASE
    if 'cancellation' in job:
        from . import face_still_cancellation
        return face_still_cancellation.access(device, operation, core, address, size, value)
    if job['step'] == 126:
        view = _request_view(device)
        if view.request is None:
            if operation != 'write' or offset != 0x14:
                raise UnsupportedDevice('Face reconfiguration awaits a complete new desired request')
            view._begin_request(core, address, size, value, inactive_mode=2)
            result = None
        else:
            result = view._request_access(operation, core, address, size, value)
        job['request'] = view.request
        device.dma_states = view.dma_states
        return result
    if operation == 'read':
        value = device.registers.get(offset)
    if not _accept(device, job['step'], device.registers, operation, offset, value):
        raise UnsupportedDevice('Face access is outside the recovered still-size reconfiguration')
    step = job['step']
    event = dict(operation=operation, offset=offset, value=value, cpu=core)
    job['accesses'].append(event)
    if operation == 'write':
        old = device.registers[offset]
        device.registers[offset] = value
        device.writes.append(dict(cpu=core, address=hex(address), old=hex(old), value=hex(value)))
        channel = _slot(device, step)[2]
        if 78 <= step < 86:
            device.dma_states[channel] = 'closed'
        if step >= 86 and (step - 86) % 10 == 9:
            device.dma_states[channel] = 'configured'
    job['step'] += 1
    job['phase'] = _phase(job['step'])
    if job['step'] == 126:
        job['completed_tick'] = device.board.virtual_ticks
    job['state_sha256'] = _seal(device, job)
    return value


def summary(device):
    return deepcopy(validate(device)) if owner(device) is not None else None
