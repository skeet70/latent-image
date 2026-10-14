"""Explicit sparse monochrome PFP experiment through the actual ARM driver.

Hardware source/configuration, IRQ49, status/ACK and teardown are qualified.
Pixel coordinates, first-entry coefficients and four-neighbor geometry are
declared simulation choices, not recovered Fuji filtering behavior.
"""
from copy import deepcopy
import hashlib
import struct

from . import afs_still_readout as raw, prefilter_initialization as idle, prefilter_scene as codec
from .hardware import UnsupportedDevice
from .histogram_protocol import digest

PROFILE = 'declared-still-sparse-monochrome-prefilter-irq49-unmeasured'
ATTRIBUTE = 'prefilter_job'
BASE, IRQ, ISR, CALLBACK = 0xfff1a000, 49, 0x10d437c, 0x11806a8
MAX_POLLS = 4096


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _seal(job):
    job['seal'] = digest({k: v for k, v in job.items() if k not in ('seal', 'contract', 'patches')})


def _contract_hash(contract):
    return digest({k: v for k, v in contract.items() if k not in ('auxiliary', 'qtable')})


def _words(device, address, count):
    return struct.unpack('<' + str(count) + 'I', device.board.read_memory(address, count * 4, cpu_id=0))


def _raw(device):
    inputs = next((d for d in device.board.devices if type(d).__name__ == 'InputProcessingControl'), None)
    if inputs is None:
        raise UnsupportedDevice('Prefilter requires the owned completed finite RAW transfer')
    record = raw.validate(inputs, pixels=True)
    if record is None or record['state'] != 'complete' or record['geometry']['packed']:
        raise UnsupportedDevice('Prefilter only covers completed declared U16 RAW rows')
    return record


def _route(device, *, armed):
    banks = device.board.interrupts.banks
    bank = banks[0]
    if bool(bank.enabled & (1 << IRQ)) != armed:
        raise UnsupportedDevice('Prefilter IRQ49 enable does not match its actual driver phase')
    if armed and (bank.config.get(0x400 + 4 * IRQ) != ISR
            or bank.config.get(0x800 + 4 * IRQ) != 13 or bank.source_config & (1 << IRQ)
            or (bank.config.get(0x180 + IRQ // 16 * 4, 0) >> (IRQ % 16 * 2)) & 3 != 1):
        raise UnsupportedDevice('Prefilter requires real logical67 IRQ49/vector/priority/type registration')
    if any(b.pending & (1 << IRQ) or b.active == IRQ or b.injected == IRQ
           or any(e.get('line') == IRQ for e in b.active_stack) for b in banks):
        raise UnsupportedDevice('Prefilter admission/completion requires drained IRQ49')


def _patch_hash(patches):
    h = hashlib.sha256()
    for offset, data in patches:
        h.update(offset.to_bytes(4, 'little')); h.update(data)
    return h.hexdigest()


def arm(device, profile=PROFILE, *, completion_ticks=1000, idle_ticks=16):
    return _arm(device, profile, completion_ticks=completion_ticks, idle_ticks=idle_ticks)


def _arm(device, profile, *, completion_ticks, idle_ticks, source_offset=8, column_shift=0, start_mode=1):
    # Separate successor owners supply independently guarded offsets/modes.
    # The ordinary public arm remains source+8/mode1.
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or device.state != 'idle' or device.request is not None
            or any(type(v) is not int or not 1 <= v <= 400000 for v in (completion_ticks, idle_ticks))
            or type(start_mode) is not int or start_mode not in (1, 3)):
        raise UnsupportedDevice('Prefilter scene requires its explicit bounded one-shot idle owner')
    previous = idle.summary(device)
    if previous is None or previous['state'] != 'complete' or _words(device, 0x152b724, 1) != (1,):
        raise UnsupportedDevice('Prefilter scene precedes whole idle initializer/CPU flag1')
    _route(device, armed=False)
    request = _words(device, 0x1552c20, 8)
    descriptor = _words(device, 0x1564fe4, 7)
    expected = (request[1], request[2], request[3], request[4], request[5], request[7], request[6])
    record = _raw(device)
    geometry = deepcopy(record['geometry'])
    if (descriptor != expected or request[0] != start_mode or request[1] != 1 or request[6] != 0
            or (request[2] & 0x3fffffff) != geometry['destination'] + source_offset
            or request[3] * 2 != geometry['pitch'] or not 0 < request[5] < 0x40000
            or request[4] & 7 or not request[7]):
        raise UnsupportedDevice('Prefilter owned U16/mode/calibration descriptor changed')
    auxiliary = bytes(device.board.read_memory(request[4], (request[5] * 34 + 7) // 8, cpu_id=0))
    qtable = bytes(device.board.read_memory(request[7], 512, cpu_id=0))
    source = bytes(device.board.read_memory(geometry['destination'], geometry['span_bytes'], cpu_id=0))
    try:
        rendered = codec.render(source, width=geometry['width'], height=geometry['height'],
            pitch=geometry['pitch'], origin=tuple(geometry['origin']), source_offset=source_offset,
            auxiliary=auxiliary, count=request[5], qtable=qtable, profile=codec.PROFILE, column_shift=column_shift)
    except ValueError as error:
        raise UnsupportedDevice(str(error)) from error
    now = device.board.virtual_ticks
    if not 0 <= now < (1 << 64) - max(completion_ticks, idle_ticks):
        raise UnsupportedDevice('Prefilter completion escapes unsigned virtual time')
    contract = dict(request=request, descriptor=descriptor, geometry=geometry,
        raw_anchor=dict(profile=record['profile'], codec=record['codec'], rows_sha256=record['rows_sha256'],
                        acknowledged_tick=record['acknowledged_tick']),
        initializer_sha256=digest(previous), auxiliary=auxiliary, qtable=qtable,
        auxiliary_sha256=hashlib.sha256(auxiliary).hexdigest(), qtable_sha256=hashlib.sha256(qtable).hexdigest(),
        source_sha256=rendered['source_sha256'], output_sha256=rendered['output_sha256'],
        weights=rendered['weights'], eligible_records=rendered['eligible_records'], changed_pixels=rendered['changed_pixels'])
    if source_offset != 8 or column_shift:
        contract.update(source_offset=source_offset, column_shift=column_shift)
    job = dict(profile=PROFILE, codec=codec.PROFILE, state='configuring', contract=contract,
        contract_sha256=_contract_hash(contract), patches=rendered['patches'], patches_sha256=_patch_hash(rendered['patches']),
        armed_tick=now, completion_ticks=completion_ticks, idle_ticks=idle_ticks, step=0,
        started_tick=None, due_tick=None, produced_tick=None, acknowledged_tick=None,
        idle_requested_tick=None, idle_due_tick=None, idle_ready_tick=None, idle_poll_boundary=None,
        polls=0, busy_polls=0, ready_polls=0, registers={BASE + o: 0 for o in (0, 4, 8, 0xc, 0x10, 0x14, 0x18, 0x1c, 0x20)}, seal=None)
    job['registers'][BASE] = 0x10000
    _seal(job); setattr(device, ATTRIBUTE, job)
    return summary(device)


def _setup(contract):
    request = contract['request']
    _, uploads = codec.coefficients(contract['qtable'])
    return (('write', BASE, 0x10100), ('write', BASE + 4, request[2] & 0x3fffffff),
            ('write', BASE + 8, request[3]),
            *(('write', BASE + 0x30 + 4 * i, word) for i, word in enumerate(uploads)),
            ('write', BASE + 0xc, request[4] & 0x3fffffff), ('write', BASE + 0x10, request[5]),
            ('write', BASE + 0x14, 0), ('write', BASE + 0x20, 0x1f),
            ('read', BASE, 0x10100), ('write', BASE, 0x10100 | request[0]))


def validate(device, *, full=False):
    job = owner(device)
    if job is None: return None
    if (type(job) is not dict or job.get('profile') != PROFILE or job.get('codec') != codec.PROFILE
            or job.get('state') not in ('configuring', 'running', 'irq-pending', 'acknowledged',
                'awaiting-idle', 'idle-requested', 'idle-ready', 'complete')
            or job.get('seal') != digest({k: v for k, v in job.items() if k not in ('seal', 'contract', 'patches')})
            or type(job.get('step')) is not int or not 0 <= job['step'] <= 137
            or any(type(job.get(k)) is not int or not 0 <= job[k] <= MAX_POLLS for k in ('polls', 'busy_polls', 'ready_polls'))
            or job['polls'] != job['busy_polls'] + job['ready_polls']):
        raise UnsupportedDevice('Prefilter scene changed its sealed bounded physical lifecycle')
    contract = job['contract']
    if (job['contract_sha256'] != _contract_hash(contract)
            or hashlib.sha256(contract['auxiliary']).hexdigest() != contract['auxiliary_sha256']
            or hashlib.sha256(contract['qtable']).hexdigest() != contract['qtable_sha256']):
        raise UnsupportedDevice('Prefilter scene changed its retained calibration/RAW contract')
    if full:
        if job['patches_sha256'] != _patch_hash(job['patches']):
            raise UnsupportedDevice('Prefilter sparse produced pixels changed')
        geometry = job['contract']['geometry']
        if any(type(offset) is not int or type(data) is not bytes or len(data) != 2
               or offset < 8 or offset + 2 > geometry['span_bytes']
               or offset % geometry['pitch'] >= geometry['width'] * 2 for offset, data in job['patches']):
            raise UnsupportedDevice('Prefilter sparse pixels leave the owned active RAW spans')
    if job['state'] == 'configuring' and job['step'] >= 137 or job['state'] != 'configuring' and job['step'] != 137:
        raise UnsupportedDevice('Prefilter starts before its full137-access setup')
    if job['started_tick'] is not None and job['due_tick'] != job['started_tick'] + job['completion_ticks']:
        raise UnsupportedDevice('Prefilter output deadline changed')
    if job['idle_ready_tick'] is not None and job['idle_poll_boundary'] != job['busy_polls']:
        raise UnsupportedDevice('Prefilter idle polling response boundary changed')
    return job


def contains(device, address, size):
    return owner(device) is not None and BASE <= address < BASE + 0x230


def access(device, operation, core, address, size, value=None):
    job = validate(device)
    if (job is None or operation not in ('read', 'write') or type(core) is not int or core != 0
            or type(size) is not int or size != 4 or type(address) is not int or address & 3
            or operation == 'write' and (type(value) is not int or not 0 <= value < 1 << 32)):
        raise UnsupportedDevice('Prefilter admits only the selected CPU0 WORD transaction')
    contract = job['contract']; now = device.board.virtual_ticks
    if job['state'] == 'configuring':
        expected_operation, expected_address, expected_value = _setup(contract)[job['step']]
        if (operation != expected_operation or address != expected_address
                or operation == 'write' and value != expected_value):
            raise UnsupportedDevice('Prefilter requires the next exact source/calibration/start access')
        if _words(device, 0x1564fe4, 7) != contract['descriptor']:
            raise UnsupportedDevice('Prefilter caller descriptor changed during setup')
        if job['step'] == 136:
            _route(device, armed=True)
            if _words(device, 0x152b748, 1) != (CALLBACK,) or _words(device, 0x152b750, 1) != (0,):
                raise UnsupportedDevice('Prefilter requires the actual single-chunk stock callback')
            job.update(state='running', started_tick=now, due_tick=now + job['completion_ticks'])
        if operation == 'write': job['registers'][address] = value
        job['step'] += 1; _seal(job)
        return expected_value if operation == 'read' else None
    if operation == 'read' and address == BASE + 0x18 and job['state'] == 'irq-pending':
        return 1
    if operation == 'write' and address == BASE + 0x1c and value == 0x1f and job['state'] == 'irq-pending':
        job['registers'][BASE + 0x18] = 0
        job.update(state='acknowledged', acknowledged_tick=now)
    elif operation == 'write' and address == BASE + 0x20 and value == 0 and job['state'] == 'acknowledged':
        if _words(device, 0xbe3920, 1) != (0,): raise UnsupportedDevice('Prefilter stop precedes stock software-busy clear')
        job['registers'][address] = 0; job['state'] = 'awaiting-idle'
    elif operation == 'write' and address == BASE and value == 0 and job['state'] == 'awaiting-idle':
        _route(device, armed=False)
        job['registers'][address] = 0
        job.update(state='idle-requested', idle_requested_tick=now, idle_due_tick=now + job['idle_ticks'])
    elif operation == 'read' and address == BASE and job['state'] in ('idle-requested', 'idle-ready'):
        if job['polls'] >= MAX_POLLS: raise UnsupportedDevice('Prefilter teardown polling exceeds its physical bound')
        job['polls'] += 1
        job['ready_polls' if job['state'] == 'idle-ready' else 'busy_polls'] += 1
        _seal(job); return job['registers'][address]
    elif operation == 'write' and address == BASE + 0x1c and value == 0x1f and job['state'] == 'idle-ready' and job['ready_polls']:
        job['state'] = 'complete'
    else:
        raise UnsupportedDevice('Prefilter job/status/teardown access is unqualified or reordered')
    _seal(job)


def tick(device):
    job = owner(device)
    if job is None or job['state'] == 'complete': return False
    now = device.board.virtual_ticks
    if job['state'] == 'running' and now >= job['due_tick']:
        validate(device, full=True); _route(device, armed=True)
        contract = job['contract']; geometry = contract['geometry']; record = _raw(device)
        anchor = dict(profile=record['profile'], codec=record['codec'], rows_sha256=record['rows_sha256'], acknowledged_tick=record['acknowledged_tick'])
        if (record['geometry'] != geometry or anchor != contract['raw_anchor']
                or _words(device, 0x152b72c, 7) != contract['descriptor']
                or _words(device, 0x152b748, 1) != (CALLBACK,) or _words(device, 0xbe3920, 1) != (1,)):
            raise UnsupportedDevice('Prefilter owned RAW or live stock request changed before pixels')
        request = contract['request']
        if (hashlib.sha256(device.board.read_memory(geometry['destination'], geometry['span_bytes'])).hexdigest() != contract['source_sha256']
                or bytes(device.board.read_memory(request[4], len(contract['auxiliary']))) != contract['auxiliary']
                or bytes(device.board.read_memory(request[7], 512)) != contract['qtable']):
            raise UnsupportedDevice('Prefilter source/calibration changed before physical completion')
        # No full-raster copy is retained. All sparse targets were preflighted.
        for offset, data in job['patches']:
            device.board.write_memory(geometry['destination'] + offset, data, cpu_id=0)
        job['registers'][BASE + 0x18] = 1
        job.update(state='irq-pending', produced_tick=now); _seal(job)
        device.board.queue_irq(0, IRQ); return True
    if job['state'] == 'idle-requested' and now >= job['idle_due_tick']:
        validate(device)
        job['registers'][BASE] = 0x10000
        job.update(state='idle-ready', idle_ready_tick=now, idle_poll_boundary=job['polls']); _seal(job)
        return True
    return False


def next_event_tick(device, now):
    job = owner(device)
    if job is None: return None
    return job['due_tick'] if job['state'] == 'running' else job['idle_due_tick'] if job['state'] == 'idle-requested' else None


def summary(device):
    job = validate(device, full=True)
    return deepcopy({key: value for key, value in job.items() if key not in ('patches', 'seal')}) if job is not None else None
