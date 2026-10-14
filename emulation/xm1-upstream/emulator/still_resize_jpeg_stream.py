"""One explicit class1 Resize stream coupled to the thumbnail JPEG engine.

Virtual pixels stay inside the peripheral input facade. No guest RAM output
or IRQ74 completion is produced; JPEG EOF retires the stream and stock code
performs the quiet Resize poll, unregister and clock close.
"""
from copy import deepcopy
import hashlib
import json
import struct

from .hardware import UnsupportedDevice


PROFILE = 'declared-still-class1-mode1-jpeg-thumbnail-stream-unmeasured'
KEYS = frozenset(('profile', 'phase', 'armed_tick', 'history', 'history_sha256',
    'entry_uploads', 'entry_words', 'input', 'start_step', 'started_tick', 'finished_tick',
    'stop_tick', 'quiet_tick'))


def _reject(reason):
    raise UnsupportedDevice('Still Resize JPEG stream: '+reason)


def _hash(data):
    return hashlib.sha256(data).hexdigest()


def _seal(value):
    return _hash(json.dumps(value, sort_keys=True, separators=(',', ':')).encode())


def _marker(device):
    if not isinstance(device.request, dict):
        return None
    return device.request.get('jpeg_main_stream', device.request.get('jpeg_stream'))


def _closed(device):
    return (int.from_bytes(device.board.read_memory(0xc9ba3c, 4, cpu_id=0), 'little') == 0
        and bytes(device.board.read_memory(0xc9ba8c, 4, cpu_id=0)) == bytes(4)
        and all(not (bank.enabled | bank.pending) & (1 << 74)
            and bank.active != 74 and bank.injected != 74
            and not any(item.get('line') == 74 for item in getattr(bank, 'active_stack', []))
            for bank in device.board.interrupts.banks))


def _jpeg_acknowledged(device, marker):
    """Pure selected-job projection; no JPEG/Resize public-validator cycle."""
    jobs = [item for item in device.board.devices if getattr(item, 'BASE', None) == 0xfff62000]
    if len(jobs) != 1:
        return False
    record = getattr(jobs[0], 'declared', None)
    if (not isinstance(record, dict) or record.get('state') not in ('acknowledged', 'closing', 'closed')
            or record.get('pending_action') is not None
            or record.get('latch', {}).get('resize_stream', {}).get('anchor_sha256')
                != marker['input']['anchor_sha256']):
        return False
    times = tuple(record.get(key) for key in ('delivered_tick', 'acknowledged_tick', 'count_read_tick'))
    return (type(marker['finished_tick']) is int and all(type(tick) is int for tick in times)
        and marker['finished_tick'] <= times[0] <= times[1] <= times[2] <= device.board.virtual_ticks
        and all(not bank.pending & (1 << 77) and bank.active != 77 and bank.injected != 77
            and not any(entry.get('line') == 77 for entry in getattr(bank, 'active_stack', []))
            for bank in device.board.interrupts.banks))


def _plan(descriptor, profile=PROFILE):
    from . import still_resize_jpeg_main_stream as main
    if profile == main.PROFILE:
        return main.plan(descriptor)
    if profile != PROFILE:
        _reject('unknown stream geometry profile')
    from .still_resize_devices import _physical
    d = struct.unpack('<20I', descriptor)
    if (d[:4] != (1, 1, 4096, 4096) or d[5:10] != (320, 320, 240, 318, 238)
            or d[10:19] != (0, 160, 0, 0, 160, 120, 159, 119, 0)
            or d[19] != 0x1cd52b8 or d[4] & 15):
        _reject('requires the actual class1/mode1/160x120 streaming descriptor')
    source = _physical(d[4], 640*240)
    expected = {4: 0x40, 0x10: source, 0x14: 320, 0x18: 320 | 240 << 16,
        0x1c: 0, 0x20: 160, 0x24: 0, 0x28: 160 | 120 << 16,
        0x2c: 0, 0x30: 0, 0x34: 1, 0x38: 4095, 0x3c: 4095, 0x40: 4096,
        0x44: 4096, 0x48: 0, 0x4c: 0x10001, 0x50: 318 | 238 << 16,
        0x54: 159 | 119 << 16, 0x58: 318 | 238 << 16,
        **dict.fromkeys(range(0x60, 0xdc, 4), 0x10000)}
    return dict(source=d[4], source_bytes=640*240, source_width=320, source_height=240,
        source_stride_bytes=640, width=160, height=120, stride_bytes=320, expected=expected)


def _pixels(source, profile=PROFILE):
    from . import still_resize_jpeg_main_stream as main
    if profile == main.PROFILE:
        return main.pixels(source)
    if profile != PROFILE:
        _reject('unknown virtual sample profile')
    # Source includes the original allocation's retained letterbox rows.
    # This explicitly declared monochrome law samples even luma coordinates
    # and supplies neutral chroma rather than inventing silicon coefficients.
    return b''.join(b''.join(bytes((source[y*2*640+x*4], source[y*2*640+(x+1)*4], 128, 128))
        for x in range(0, 160, 2)) for y in range(120))


def _input_seal(record):
    return _seal({k: v for k, v in record.items() if k not in ('descriptor', 'packed_bytes', 'anchor_sha256')})


def validate(device, *, source=False, historical=False):
    marker = _marker(device)
    if marker is None:
        return False
    from . import still_resize_successor as previous
    from . import still_resize_jpeg_main_stream as main
    is_main = isinstance(marker, dict) and marker.get('profile') == main.PROFILE
    if (not isinstance(marker, dict) or set(marker) != KEYS | ({'previous'} if is_main else set())
            or marker['profile'] not in (PROFILE, main.PROFILE)
            or marker['phase'] not in ('awaiting-open', 'opening', 'upload', 'ready', 'streaming', 'finished', 'closing', 'idle-acknowledged')
            or type(marker['armed_tick']) is not int or not 0 <= marker['armed_tick'] <= device.board.virtual_ticks
            or type(marker['history']) is not list or len(marker['history']) != 2):
        _reject('retained owner schema or phase changed')
    if historical and (is_main or source or marker['phase'] != 'idle-acknowledged'):
        _reject('only the closed thumbnail capsule permits a historical projection')
    if is_main:
        main.validate_previous(device, marker)
    for item in marker['history']:
        previous._closed(item)
    history_ids = [item['sha256'] for item in marker['history']]
    if is_main:
        history_ids.append(marker['previous']['sha256'])
    if (marker['history_sha256'] != _seal(history_ids)
            or device.request.get('history') != marker['history'][:1]
            or {k: v for k, v in device.request.items() if k not in ('history', 'jpeg_stream', 'jpeg_main_stream')}
                != marker['history'][1]['request']
            or tuple(device.uploads[:len(marker['entry_uploads'])]) != marker['entry_uploads']
            or not is_main and marker['entry_words'] != marker['history'][1]['registers']):
        _reject('the two closed predecessors changed')
    phase, inputs = marker['phase'], marker['input']
    if phase == 'awaiting-open' and device.registers != marker['entry_words']:
        _reject('hardware changed before its explicit quiet opening')
    if phase in ('awaiting-open', 'opening', 'upload'):
        if inputs is not None or marker['start_step'] != 0 or any(marker[k] is not None for k in ('started_tick', 'finished_tick', 'stop_tick', 'quiet_tick')):
            _reject('initialization has premature pixel/start state')
        return True
    if (not isinstance(inputs, dict) or set(inputs) != {'profile', 'descriptor', 'descriptor_sha256',
            'source', 'source_bytes', 'source_capacity', 'source_width', 'source_height', 'source_stride_bytes',
            'source_sha256', 'width', 'height', 'stride_bytes', 'expected', 'packed_bytes',
            'packed_sha256', 'history_sha256', 'anchor_sha256'}
            or inputs['profile'] != marker['profile'] or type(inputs['descriptor']) is not bytes
            or type(inputs['packed_bytes']) is not bytes
            or _hash(inputs['descriptor']) != inputs['descriptor_sha256']
            or _hash(inputs['packed_bytes']) != inputs['packed_sha256']
            or _input_seal(inputs) != inputs['anchor_sha256']
            or inputs['history_sha256'] != marker['history_sha256']
            or inputs['source_capacity'] != inputs['source_bytes']
            or len(inputs['packed_bytes']) != inputs['stride_bytes']*inputs['height']
            or _plan(inputs['descriptor'], marker['profile'])
                != {k: inputs[k] for k in _plan(inputs['descriptor'], marker['profile'])}
            or any(device.registers[offset] != word for offset, word in inputs['expected'].items())):
        _reject('sealed stream descriptor, geometry or virtual pixels changed')
    descriptor = (inputs['descriptor'][:76]+bytes(4) if historical else
                  bytes(device.board.read_memory(0xc9ba40, 80, cpu_id=0)))
    if descriptor != inputs['descriptor'] and not (marker['quiet_tick'] is not None
            and descriptor == inputs['descriptor'][:76]+bytes(4) and (historical or _closed(device))
            or phase in ('finished', 'closing', 'idle-acknowledged')
            and descriptor == inputs['descriptor'][:76]+bytes(4)
            and _jpeg_acknowledged(device, marker)):
        _reject('copied stream descriptor changed outside its stock unregister')
    step = marker['start_step']
    if type(step) is not int or not 0 <= step <= 3 or phase == 'ready' and step >= 3:
        _reject('start ledger changed')
    if phase != 'ready':
        if (step != 3 or type(marker['started_tick']) is not int
                or not marker['armed_tick'] <= marker['started_tick'] <= device.board.virtual_ticks):
            _reject('stream lacks its actual start')
    elif marker['started_tick'] is not None:
        _reject('ready stream has a premature start timestamp')
    if phase in ('finished', 'closing', 'idle-acknowledged'):
        if (type(marker['finished_tick']) is not int
                or not marker['started_tick'] <= marker['finished_tick'] <= device.board.virtual_ticks):
            _reject('stream EOF chronology changed')
    elif marker['finished_tick'] is not None:
        _reject('stream has premature EOF')
    if phase in ('closing', 'idle-acknowledged'):
        if (type(marker['stop_tick']) is not int
                or not marker['finished_tick'] <= marker['stop_tick'] <= device.board.virtual_ticks):
            _reject('quiet request chronology changed')
    elif marker['stop_tick'] is not None:
        _reject('stream has a premature quiet request')
    control = {'ready': 0x10000, 'streaming': 0x10001, 'finished': 0x10000,
               'closing': 0, 'idle-acknowledged': 0x10000}[phase]
    # 97AD18 unregisters the kernel owner but retains register148=21. A real
    # next quiet initializer carries that word until97AD74 writes21 again.
    irq_enable = marker['entry_words'][0x148] if phase == 'ready' and step < 2 else 0x21
    if (device.state != 'idle' or device.registers[0] != control
            or device.registers[0x144] != 0 or device.registers[0x148] != irq_enable):
        _reject('stream phase lost its live control/status projection')
    if phase == 'closing':
        if device.pending_action != dict(kind='jpeg-stream-idle-ack',
                due_tick=marker['stop_tick']+device.idle_ticks):
            _reject('quiet response timer changed')
    elif device.pending_action is not None:
        _reject('stream has an unowned pending hardware action')
    if phase == 'idle-acknowledged':
        if (type(marker['quiet_tick']) is not int
                or not marker['stop_tick']+device.idle_ticks <= marker['quiet_tick'] <= device.board.virtual_ticks
                or device.registers[0] != 0x10000):
            _reject('quiet completion chronology/control changed')
    elif marker['quiet_tick'] is not None:
        _reject('stream has premature quiet completion')
    if source and _hash(device.board.read_memory(inputs['source'], inputs['source_bytes'], cpu_id=0)) != inputs['source_sha256']:
        _reject('original streaming input changed before consumption')
    return True


def arm_initialization(device, profile):
    from . import still_resize_successor as previous
    from .still_resize_thumbnail import PROFILE as THUMBNAIL_PROFILE
    from .still_resize_devices import PROFILE as DEVICE_PROFILE
    if (profile != PROFILE or device.profile != DEVICE_PROFILE
            or device.request is None or _marker(device) is not None
            or device.request.get('scene_profile') != THUMBNAIL_PROFILE
            or device.state not in ('idle', 'closed') or device.pending_action is not None
            or device.board.failure is not None):
        _reject('requires the genuinely closed two-job preview predecessor')
    device._validate()
    previous._drained(device)
    if not _closed(device):
        _reject('predecessor registration is not closed')
    last = dict(request=deepcopy({k: v for k, v in device.request.items() if k != 'history'}),
        registers=dict(device.registers), uploads=tuple(device.uploads),
        ram_descriptor=bytes(device.board.read_memory(0xc9ba40, 80, cpu_id=0)))
    last['sha256'] = previous._digest(last)
    previous._closed(last)
    history = deepcopy(device.request['history'])+[last]
    device.request['jpeg_stream'] = dict(profile=PROFILE, phase='awaiting-open',
        armed_tick=device.board.virtual_ticks, history=history,
        history_sha256=_seal([item['sha256'] for item in history]),
        entry_uploads=tuple(device.uploads), entry_words=dict(device.registers), input=None,
        start_step=0, started_tick=None, finished_tick=None, stop_tick=None, quiet_tick=None)
    return summary(device)


def setup_allowed(device):
    marker = _marker(device)
    return marker is not None and marker['phase'] == 'upload'


def arm_scene(device, profile, *, source_sha256, source_capacity):
    if (not setup_allowed(device) or profile != _marker(device)['profile'] or device.state != 'idle'
            or device.pending_action is not None or device.board.failure is not None):
        _reject('requires the explicit quiet opening and copied class1 upload')
    validate(device)
    descriptor = bytes(device.board.read_memory(0xc9ba40, 80, cpu_id=0))
    plan = _plan(descriptor, profile)
    if any(device.registers[o] != w for o, w in plan['expected'].items()):
        _reject('whole upload differs from the copied request')
    source = bytes(device.board.read_memory(plan['source'], plan['source_bytes'], cpu_id=0))
    last = _marker(device)['history'][1]['request']
    if (type(source_capacity) is not int or source_capacity != plan['source_bytes']
            or type(source_sha256) is not str or source_sha256 != _hash(source)
            or profile == PROFILE and (plan['source'] != last['plan']['destination']-13*640
                or any(_hash(device.board.read_memory(address, len(row), cpu_id=0)) != _hash(row)
                    for address, row in last['rows']))):
        _reject('requires the independent original allocation/hash and unchanged prior thumbnail rows')
    packed = _pixels(source, profile)
    marker = _marker(device)
    inputs = dict(profile=profile, descriptor=descriptor, descriptor_sha256=_hash(descriptor),
        **plan, source_capacity=source_capacity, source_sha256=source_sha256,
        packed_bytes=packed, packed_sha256=_hash(packed),
        history_sha256=marker['history_sha256'])
    inputs['anchor_sha256'] = _input_seal(inputs)
    marker.update(phase='ready', input=inputs)
    return input_view(device)


def input_view(device, *, require_started=False):
    if not validate(device) or _marker(device)['input'] is None:
        _reject('stream pixels were not explicitly admitted')
    marker = _marker(device)
    if require_started and marker['phase'] not in ('streaming', 'finished', 'closing', 'idle-acknowledged'):
        _reject('JPEG consumption precedes real97AD74 start')
    closed = marker['phase'] == 'idle-acknowledged' and _closed(device)
    if not closed:
        validate(device, source=True)
    result = deepcopy(marker['input'])
    result['state'] = 'closed' if closed else marker['phase']
    return result


def before_write(device, offset, value):
    marker = _marker(device)
    if marker is None:
        return False
    validate(device)
    phase = marker['phase']
    if phase == 'awaiting-open':
        if ((offset, value) != (0, 0)
                or bytes(device.board.read_memory(0xc9ba38, 0x140, cpu_id=0)) != bytes(0x140)):
            _reject('requires whole97ACD4 clear and quiet opening')
        marker['phase'] = 'opening'
        return False
    if phase == 'opening':
        if ((offset, value) != (0x144, 0x3f) or device.state != 'idle'
                or device.pending_action is not None or device.registers[0] != 0x10000):
            _reject('opening requires its real quiet ACK/status clear')
        device.registers[offset] = 0
        marker['phase'] = 'upload'
        return True
    if phase == 'upload':
        return False
    if phase == 'ready':
        step = marker['start_step']
        expected = ((0x140, 0), (0x148, 0x21), (0, 0x10001))[step]
        if (offset, value) != expected:
            _reject('requires exact real97AD74 start stores')
        if step == 2:
            validate(device, source=True)
            device._route(drained=True)
            marker.update(phase='streaming', started_tick=device.board.virtual_ticks)
        marker['start_step'] += 1
        device.registers[offset] = value
        return True
    if phase == 'finished':
        if (offset, value) != (0, 0) or device.pending_action is not None:
            _reject('JPEG EOF requires real97AD18 quiet stop next')
        due = device.board.virtual_ticks+device.idle_ticks
        if due >= 1 << 64:
            _reject('quiet stop clock overflows')
        device.registers[0] = 0
        device.pending_action = dict(kind='jpeg-stream-idle-ack', due_tick=due)
        marker.update(phase='closing', stop_tick=device.board.virtual_ticks)
        return True
    _reject('unknown active streaming or duplicate quiet write')


def complete(device, expected_sha256):
    if not validate(device, source=True):
        _reject('EOF has no admitted owner')
    marker = _marker(device)
    jpeg = next((x for x in device.board.devices if getattr(x, 'BASE', None) == 0xfff62000), None)
    job = getattr(jpeg, 'declared', None)
    action = job.get('pending_action') if isinstance(job, dict) else None
    if (marker['phase'] != 'streaming' or expected_sha256 != marker['input']['anchor_sha256']
            or not isinstance(job, dict) or job.get('state') != 'encoding'
            or job.get('latch', {}).get('resize_stream', {}).get('anchor_sha256') != expected_sha256
            or not isinstance(action, dict) or action.get('kind') != 'jpeg-data-before-irq77'
            or action.get('cpu') != 0 or type(action.get('due_tick')) is not int
            or device.board.virtual_ticks < action['due_tick']):
        _reject('EOF requires its selected live JPEG hardware completion')
    device._route(drained=True)
    device.registers[0] &= ~1
    marker.update(phase='finished', finished_tick=device.board.virtual_ticks)


def tick(device):
    action = device.pending_action
    if action is None or action.get('kind') != 'jpeg-stream-idle-ack':
        return False
    if device.board.virtual_ticks < action['due_tick']:
        return False
    validate(device)
    marker = _marker(device)
    if (set(action) != {'kind', 'due_tick'} or marker['phase'] != 'closing'
            or device.registers[0] != 0):
        _reject('quiet ACK lost its actual stop request')
    device.registers[0] = 0x10000
    device.pending_action = None
    marker.update(phase='idle-acknowledged', quiet_tick=device.board.virtual_ticks)
    return True


def summary(device):
    if not validate(device):
        return None
    marker = _marker(device)
    inputs = marker['input']
    return dict(profile=marker['profile'],
        state='closed' if marker['phase'] == 'idle-acknowledged' and _closed(device) else marker['phase'],
        closed_predecessors=len(marker['history']), history_sha256=marker['history_sha256'],
        anchor_sha256=inputs['anchor_sha256'] if inputs is not None else None,
        started_tick=marker['started_tick'], finished_tick=marker['finished_tick'], quiet_tick=marker['quiet_tick'],
        limits=('One class1 mode0 unity main JPEG stream after a closed thumbnail stream; two closed preview jobs retained; '
                if 'previous' in marker else 'One class1 mode1 virtual JPEG stream after two closed preview jobs; ')
            +'no guest RAM output or IRQ74 completion; stock JPEG callback owns Resize stop/unregister.')
