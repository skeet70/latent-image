"""Explicit pre-exposure numeric initialization, preserving completed AF.

The generation8 initialization clock enters stock 12FA834/12F9FCC. Its
mask46 initializer uses the audited GLOBAL reset, DMA idle stops, desired
setters and 22-access activation. A separately armed declared scene supplies
consumer bytes followed by physical IRQ46; stock firmware owns callbacks,
publications, finite retirement and exposure decisions.
"""
from copy import copy, deepcopy
import hashlib

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import producer_afs_focus as af
from .virtual_source import ram_span


PROFILE = 'declared-afs-generation8-capture-metering-producer-unmeasured'
CLOCK_PROFILE = 'declared-afs-generation8-capture-initialization-source0-clock-unmeasured'
KEY = 'capture_metstats'
DATA_PROFILE = 'declared-afs-generation8-capture-scene-metering-first-unmeasured'
FINAL_DATA_PROFILE = 'declared-afs-generation8-capture-scene-metering-final-unmeasured'
DATA_TICKS = 1024
ACK_OFFSETS = (0x88c, 0x8f4, 0x98c, 0x9f4)


def _owner(device):
    return device.rearm.get(KEY) if type(device.rearm) is dict else None


def _record(device, terminal, clock):
    return dict(profile=PROFILE, cpu=0, armed_tick=device.board.virtual_ticks,
        terminal=terminal, terminal_sha256=hp.digest(terminal),
        clock=clock, clock_sha256=hp.digest(clock),
        state=af._state(terminal['registers']), accesses=[])


def historical_af_view(device):
    """A sealed historical projection; the current device is never modified."""
    record = _owner(device)
    if record is None: return device
    terminal = record.get('terminal')
    if (type(terminal) is not dict or set(terminal) != {'registers', 'afs_focus_sha256', 'source'}
            or type(terminal['registers']) is not dict or type(terminal['source']) is not dict
            or record.get('terminal_sha256') != hp.digest(terminal)
            or terminal['afs_focus_sha256'] != hp.digest(device.rearm.get('afs_focus'))):
        raise UnsupportedDevice('Capture metering terminal AF history changed')
    view = copy(device)
    view.registers = dict(terminal['registers'])
    view.rearm = {key: value for key, value in device.rearm.items() if key != KEY}
    view.board = copy(device.board)
    view.board.interrupts = copy(device.board.interrupts)
    view.board.interrupts.banks = [copy(bank) for bank in device.board.interrupts.banks]
    view.board.devices = []
    sources = 0
    for item in device.board.devices:
        proxy = view if item is device else copy(item)
        proxy.board = view.board
        if type(item).__name__ == 'AfsFrameSource':
            sources += 1
            proxy.request = deepcopy(terminal['source'])
        view.board.devices.append(proxy)
    if sources != 1:
        raise UnsupportedDevice('Capture metering requires one retained source history')
    # Old AF had genuinely retired IRQ46 before this new registrar. Its
    # historical validation must not depend on the new live numeric route.
    for bank in view.board.interrupts.banks:
        bank.enabled &= ~(1 << 46)
        bank.pending &= ~(1 << 46)
        if bank.active == 46: bank.active = None
        if bank.injected == 46: bank.injected = None
    return view


def _read(board, address, size=4):
    return int.from_bytes(board.read_memory(address, size), 'little')


def arm(device, profile=PROFILE):
    if (profile != PROFILE or type(profile) is not str or type(device.rearm) is not dict
            or KEY in device.rearm or 'afs_focus' not in device.rearm):
        raise UnsupportedDevice('Unknown or repeated capture metering admission')
    af.validate(device)
    previous = device.rearm['afs_focus']
    close = previous.get('retirement', {})
    sources = [item for item in device.board.devices if type(item).__name__ == 'AfsFrameSource']
    if len(sources) != 1 or not callable(getattr(sources[0], 'capture_initialization_summary', None)):
        raise UnsupportedDevice('Capture metering requires the exact external initialization clock')
    clock = sources[0].capture_initialization_summary()
    now = device.board.virtual_ticks
    if (type(clock) is not dict or clock.get('profile') != CLOCK_PROFILE
            or tuple(clock.get(key) for key in ('generation', 'cpu', 'source', 'physical_irq', 'state'))
                != (8, 0, 0, '0x27', 'waiting')
            or not clock.get('requested_tick', -1) <= now < clock.get('due_tick', -1)
            or close.get('state') != 'completed' or not af._idle(device)
            or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured'
            or (_read(device.board, 0x15d4908, 1), _read(device.board, 0x15d490c),
                _read(device.board, 0x15f8814, 2), _read(device.board, 0x15f8816, 2),
                _read(device.board, 0x15f63a4), _read(device.board, 0x15f68a4)) != (3, 1, 3, 1, 2, 0)
            or device.registers[device.GLOBAL] != 0x10030000
            or any(device.registers[device.BASE + offset] != word for offset, word in
                   ((4, 0x33170000), (8, 0x10000), (0x884, 0), (0x890, 0), (0x8f8, 0)))
            or any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
                   or bank.active == 46 or bank.injected == 46
                   for bank in device.board.interrupts.banks)):
        raise UnsupportedDevice('Capture metering awaits the real gen8 clock, retired AF and idle numeric client')
    source = {key: deepcopy(value) for key, value in sources[0].request.items()
              if key != 'capture_initialization'}
    terminal = dict(registers=dict(device.registers), afs_focus_sha256=hp.digest(previous), source=source)
    record = _record(device, terminal, deepcopy(clock))
    device.rearm[KEY] = record
    device.board.record('Capture metering producer explicitly admitted', profile=profile,
        limits='Separate historical AF projection; no DMA, IRQ, callback, histogram or capture result')
    return deepcopy(record)


def _plain(device, record):
    expected_words = record.get('state', {}).get('words') if type(record) is dict else None
    data = record.get('data') if type(record) is dict else None
    if type(data) is dict:
        ack = data.get('ack_words')
        if (type(ack) is not dict or set(ack) != {device.BASE + o for o in ACK_OFFSETS}
                or type(expected_words) is not dict):
            raise UnsupportedDevice('Capture metering ACK projection changed')
        expected_words = {**expected_words, **ack}
    if (type(record) is not dict
            or set(record) - {'data', 'data_history', 'acquisition_rotation', 'stop_rotation', 'retirement', 'allocation_release'} != {'profile', 'cpu', 'armed_tick', 'terminal', 'terminal_sha256',
                              'clock', 'clock_sha256', 'state', 'accesses'}
            or record.get('profile') != PROFILE or type(record.get('cpu')) is not int or record['cpu'] != 0
            or type(record['armed_tick']) is not int or not 0 <= record['armed_tick'] <= device.board.virtual_ticks
            or type(record['accesses']) is not list or len(record['accesses']) > af.ACCESS_LIMIT + af.RESET_POLL_LIMIT
            or type(record['state']) is not dict or set(record['state']) != af.STATE_KEYS
            or expected_words != device.registers or not af._idle(device)):
        raise UnsupportedDevice('Capture metering current plain hardware projection changed')
    return record


def _route(device, state):
    record = _owner(device)
    data = record.get('data') if type(record) is dict else None
    close = record.get('retirement') if type(record) is dict else None
    if type(close) is dict and close.get('state') == 'completed':
        banks = device.board.interrupts.banks
        if not any(bank.enabled & (1 << 46) for bank in banks):
            if (any(bank.pending & (1 << 46) or bank.injected == 46 for bank in banks)
                    or banks[1].active == 46):
                raise UnsupportedDevice('Capture retirement cannot retain another numeric assertion')
            return None
    if state['request_step'] >= 16 and data is not None:
        from .afs_statistics_frame import _route as numeric_route
        numeric_route(device, pending=data['state'] in ('delivered', 'acknowledged'))
        route = dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)
        if state['request_route'] != route:
            raise UnsupportedDevice('Capture numeric route differs from its real initializer')
        return route
    route = device._preview_request_route(0, state['request_step'] >= 16)
    if (state['request_step'] < 15 and route is not None
            or state['request_route'] != (route if state['request_step'] >= 16 else None)):
        raise UnsupportedDevice('Capture metering IRQ46 registration order changed')
    return route


def validate(device, *, check_history=True):
    record = _owner(device)
    if record is None: return None
    _plain(device, record)
    if not check_history: return record
    af.validate(historical_af_view(device))
    if record['clock_sha256'] != hp.digest(record['clock']):
        raise UnsupportedDevice('Capture metering latched clock changed')
    state = af._state(record['terminal']['registers'])
    previous = record['armed_tick']
    for event in record['accesses']:
        if type(event) is not dict or type(event.get('tick')) is not int or event['tick'] < previous:
            raise UnsupportedDevice('Capture metering whole transaction chronology changed')
        af._transition(device, state, event, data=_effects(record))
        previous = event['tick']
    state['request_route'] = record['state']['request_route']
    if state != record['state']:
        raise UnsupportedDevice('Capture metering replay differs from current hardware')
    _route(device, state)
    _validate_data_history(device, record)
    _validate_rotation(device, record)
    _validate_retirement(device, record)
    _validate_release(device, record)
    return record


def access(device, operation, cpu, address, size, value=None):
    record = validate(device, check_history=False)
    if record is None: return False
    if type(size) is not int or size != 4:
        raise UnsupportedDevice('Capture metering requires CPU0 WORD transactions')
    state = record['state']
    route = _route(device, state)
    if _data_access(device, record, operation, cpu, address, size, value): return True
    if _retirement_access(device, record, operation, cpu, address, size, value): return True
    if _release_access(device, record, operation, cpu, address, size, value): return True
    if state['phase'] == 'activation':
        route = device._preview_request_route(0, state['request_step'] >= 15)
    old = device.registers.get(address)
    event = dict(operation=operation, cpu=cpu, address=address,
        word=old if operation == 'read' else value, old_word=old, tick=device.board.virtual_ticks)
    if operation == 'write' and state['request_step'] == 22 and address in device.DESTINATIONS:
        event = _rotation_event(device, record, event)
    projection = {**state, 'words': dict(state['words'])}
    af._transition(device, projection, event, data=_effects(record))
    if projection['request_step'] >= 16: projection['request_route'] = route
    if len(record['accesses']) >= af.ACCESS_LIMIT + af.RESET_POLL_LIMIT:
        raise UnsupportedDevice('Capture metering bounded transaction exhausted')
    record['accesses'].append(event); record['state'] = projection
    if operation == 'write':
        device.registers[address] = value
        device.writes.append(dict(cpu=cpu, address=hex(address), value=hex(value), old=hex(old)))
    return True


def tick(device, board):
    record = _owner(device)
    if record is not None and _data_tick(device, record, board): return True
    if record is None or record['state']['phase'] != 'resetting': return False
    if board.virtual_ticks < record['state']['reset_due_tick']: return False
    validate(device)
    event = dict(operation='response', cpu=None, address=device.GLOBAL, word=0x10000000,
        old_word=device.registers[device.GLOBAL], tick=board.virtual_ticks)
    projection = {**record['state'], 'words': dict(record['state']['words'])}
    af._transition(device, projection, event)
    record['accesses'].append(event); record['state'] = projection
    device.registers[device.GLOBAL] = 0x10000000
    return True


def due(device):
    record = _owner(device)
    if record is not None and record['state']['phase'] == 'resetting':
        return record['state']['reset_due_tick']
    data = record.get('data') if type(record) is dict else None
    if data is not None:
        return data['ack_due_tick'] if data['ack_due_tick'] is not None else data['due_tick'] if data['state'] == 'pending' else None
    return None


def summary(device):
    record = validate(device)
    if record is None: return None
    state = record['state']
    return dict(profile=PROFILE, phase=state['phase'], accesses=len(record['accesses']),
        idle_step=state['idle_step'], request_step=state['request_step'],
        enabled_tick=state['request_enabled_tick'], route=state['request_route'], due_tick=due(device),
        data=(dict(sample=record['data']['sample'], state=record['data']['state'],
                   completed_tick=record['data']['completed_tick'], acknowledged_tick=record['data']['acknowledged_tick'])
              if record.get('data') is not None else None),
        stop_rotation_step=len(record.get('stop_rotation', {}).get('events', [])),
        acquisition_rotation_step=len(record.get('acquisition_rotation', {}).get('events', [])),
        retirement=deepcopy(record.get('retirement')),
        allocation_release=(dict(state=record['allocation_release']['state'],
                                 accesses=len(record['allocation_release']['accesses']),
                                 completed_tick=record['allocation_release']['completed_tick'])
                            if record.get('allocation_release') is not None else None),
        limits='Declared scene statistics and two explicit completions; stock firmware owns publications and decisions')


def _effects(record):
    close = record.get('retirement')
    # The shared transition guard needs the first physical close boundary;
    # the capture ledger derives it from its own exact eight-access history.
    if close is not None and not close.get('accesses'):
        raise UnsupportedDevice('Capture retirement has no first physical close access')
    close = dict(close, started_tick=close['accesses'][0]['tick']) if close is not None else None
    events = [event for key in ('acquisition_rotation', 'stop_rotation')
              for event in record.get(key, {}).get('events', [])]
    return dict(data=dict(rotation=dict(events=events)), retirement=close,
                allocation_release=record.get('allocation_release'))


def source_anchor(device):
    """Pure retained initialization identity; no source/history recursion."""
    record = _owner(device)
    if record is None or record['state']['request_step'] != 22:
        raise UnsupportedDevice('Capture scene requires its complete actual activation')
    endings = [i for i, event in enumerate(record['accesses'])
        if (event.get('operation'), event.get('address'), event.get('word'), event.get('tick')) ==
        ('write', device.BASE + 8, 0x10000, record['state']['request_enabled_tick'])]
    if (len(endings) != 1 or record['terminal_sha256'] != hp.digest(record['terminal'])
            or record['clock_sha256'] != hp.digest(record['clock'])):
        raise UnsupportedDevice('Capture activation lost its exact initializer prefix')
    state = af._state(record['terminal']['registers'])
    prefix = record['accesses'][:endings[0] + 1]
    previous = record['armed_tick']
    for event in prefix:
        if event['tick'] < previous:
            raise UnsupportedDevice('Capture activation chronology changed')
        af._transition(device, state, event); previous = event['tick']
    if state['request_step'] != 22 or state['request_enabled_tick'] != record['state']['request_enabled_tick']:
        raise UnsupportedDevice('Capture activation descriptor/setup/enable changed')
    route = dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)
    if (record['state']['request_route'] not in (route, None)
            or record['state']['request_route'] is None
                and record.get('retirement', {}).get('state') != 'completed'):
        raise UnsupportedDevice('Capture activation lost its original numeric route before retirement')
    return dict(enabled_tick=state['request_enabled_tick'], sha256=hp.digest(dict(
        terminal_sha256=record['terminal_sha256'], clock_sha256=record['clock_sha256'],
        accesses=prefix, route=route)))


def _source(device):
    sources = [item for item in device.board.devices if type(item).__name__ == 'AfsFrameSource']
    if len(sources) != 1:
        raise UnsupportedDevice('Capture scene requires one real clock owner')
    return sources[0]


def _initial_epoch(device):
    marker = _source(device).capture_initialization_summary()
    fields = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'input_sha256',
              'asserted_tick', 'acknowledged_tick', 'eoi_tick')
    if (marker is None or marker.get('state') != 'acknowledged' or marker.get('eoi_tick') is None
            or marker['input_sha256'] != _owner(device)['clock']['input_sha256']):
        raise UnsupportedDevice('Capture scene awaits its real initialization ACK/EOI')
    return {key: deepcopy(marker[key]) for key in fields}


def _clock_epoch(marker):
    fields = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'input_sha256',
              'asserted_tick', 'acknowledged_tick', 'eoi_tick')
    if (type(marker) is not dict or marker.get('state') != 'acknowledged'
            or marker.get('eoi_tick') is None):
        raise UnsupportedDevice('Capture scene requires its actual acquisition/stop ACK/EOI')
    return {key: deepcopy(marker[key]) for key in fields}


def _seal_data(data):
    data['state_sha256'] = hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})


def arm_data(device, profile=DATA_PROFILE):
    """Declare a future scene transport; admission produces no guest effects."""
    record = validate(device)
    if record is None:
        raise UnsupportedDevice('Capture scene requires an admitted producer')
    sample = 1 if record.get('data') is None else 2
    previous = record.get('data')
    if (profile != (DATA_PROFILE if sample == 1 else FINAL_DATA_PROFILE)
            or sample == 2 and (previous.get('sample') != 1 or previous.get('state') != 'acknowledged'
                               or record.get('data_history'))
            or (_read(device.board, 0x15d4908), _read(device.board, 0x15d4888, 1),
                _read(device.board, 0x15f63a4), _read(device.board, 0x15f68a4))
                    != (5, sample - 1, 3, 1 if sample == 1 else 3)
            or _read(device.board, 0x15f68a8) != 0x12f604c
            or any(bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46
                   for bank in device.board.interrupts.banks)):
        raise UnsupportedDevice('Capture scene requires first continuing or final finite numeric request')
    initial = _initial_epoch(device)
    activation = source_anchor(device)
    now = device.board.virtual_ticks
    if (not activation['enabled_tick'] < now or type(device.ack_ticks) is not int or device.ack_ticks < 1
            or now + DATA_TICKS + 2 * device.ack_ticks >= 1 << 64):
        raise UnsupportedDevice('Capture scene completion must follow its actual enable and positive delay')
    source = _source(device)
    inputs = source._prefix(historical=True)
    if (inputs.event_enable_generation != 8 or inputs.registers.get(0x38c) != 0x10201
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.busy or inputs.phase_busy or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Capture scene transport requires unchanged generation8 sensor gates')
    acquisition = source.capture_acquisition_start_summary()
    acquisition_epoch = _clock_epoch(acquisition)
    if (acquisition.get('contract', {}).get('producer') != activation
            or type(record.get('acquisition_rotation')) is not dict
            or len(record['acquisition_rotation']['events']) != 9):
        raise UnsupportedDevice('Capture scene requires the complete real acquisition bank rotation')
    stop = None
    if sample == 2:
        stop = source.capture_initialization_summary().get('finite_stop')
        rotation = record.get('stop_rotation')
        if (type(stop) is not dict or stop.get('state') != 'acknowledged' or stop.get('eoi_tick') is None
                or type(rotation) is not dict or len(rotation['events']) != 9
                or any(bank.pending & (1 << 0x27) or bank.active == 0x27 or bank.injected == 0x27
                       for bank in device.board.interrupts.banks)):
            raise UnsupportedDevice('Final capture scene requires the real finite-stop clock and complete bank rotation')
        stop = _clock_epoch(stop)
    from . import afs_statistics_frame as statistics
    from .afs_scene_fixture import consumer_payloads
    scene, _ = statistics._scene(device)
    image = bytes(device.board.read_memory(int(scene['address'], 16), scene['bytes']))
    if hashlib.sha256(image).hexdigest() != scene['payload_sha256']:
        raise UnsupportedDevice('Capture scene bytes differ from the validated displayed source')
    payload = consumer_payloads(image, width=scene['width'], height=scene['height'], stride_bytes=scene['stride_bytes'])
    # The fourth plane is consumed by 130C594(selector6). This explicitly
    # monochrome experiment uses the same luma histogram in all four planes.
    histogram = next(raw for register, offset, raw in payload['spans'] if register == device.BASE + 0x508 and offset == 0)
    base = 0x5fecaa60 if sample == 1 else 0x5fecd410
    offsets = (0, 0x800, 0x850, 0x1850, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0)
    completed_destinations = acquisition['contract']['destinations'] if sample == 1 else {
        event['address']: event['word'] for event in record['acquisition_rotation']['events']}
    if (set(completed_destinations) != set(device.DESTINATIONS)
            or tuple(completed_destinations[a] - base for a in device.DESTINATIONS) != offsets):
        raise UnsupportedDevice('Capture completed-frame bank differs from its physical clock latch')
    programmed_bank = 0x5fecd410 if sample == 1 else 0x5fecaa60
    programmed = {address: programmed_bank + offset for address, offset in zip(device.DESTINATIONS, offsets)}
    if any(device.registers[address] != value for address, value in programmed.items()):
        raise UnsupportedDevice('Capture next programmed bank differs from its qualified rotation')
    plan = []; spans = []
    for register, offset, raw in payload['spans']:
        if (register, offset) == (device.BASE + 0x520, 0x200): raw = histogram
        address = completed_destinations[register] + offset
        low, high = ram_span(address, len(raw))
        if address & 3 or any(low < end and start < high for start, end in spans):
            raise UnsupportedDevice('Capture consumer windows must be disjoint aligned DRAM spans')
        spans.append((low, high)); plan.append(dict(register=register, offset=offset, address=address,
                                                   bytes=len(raw), payload_hex=raw.hex()))
    prior = {device.BASE + offset: device.registers[device.BASE + offset] for offset in ACK_OFFSETS}
    if tuple(prior.values()) != ((0, 1, 0, 0) if sample == 1 else (0, 0, 0, 0)):
        raise UnsupportedDevice('Capture scene ACK latches differ from setup or the previous paired completion')
    latch = dict(activation=activation, initial_clock=initial, acquisition_start=acquisition_epoch, finite_stop=stop,
        input_sha256=hp.digest(dict(scene=scene, activation=activation, initial=initial,
                                   acquisition=acquisition_epoch, stop=stop, sample=sample)),
        scene=scene, bank=base, plan=plan,
        programmed_destinations=programmed,
        desired={a: device.registers[a] for a in device.KNOWN_DESIRED_CONFIG},
        assumptions='Declared frozen YYCbCr monochrome scene; four luma histogram planes, horizontal gradients and luma sums; other consumer windows zero. Sensor crop, exposure/flash response and ISP calibration are unmeasured; no camera decision is supplied.')
    data = dict(profile=profile, sample=sample, state='pending', armed_tick=now, due_tick=now + DATA_TICKS,
        completed_tick=None, acknowledged_tick=None, latch=latch, latch_sha256=hp.digest(latch),
        prior_ack_words=prior, ack_words=dict(prior), ack_channel=0, ack_step=0, ack_due_tick=None, events=[])
    _seal_data(data)
    if sample == 2: record['data_history'] = [deepcopy(previous)]
    record['data'] = data
    device.board.record('Capture scene metering transport explicitly armed', profile=profile, sample=sample,
        due_tick=data['due_tick'], bytes=sum(span['bytes'] for span in plan), limits=latch['assumptions'])
    return deepcopy(data)


def _validate_ack(device, data):
    words = dict(data['prior_ack_words']); channel = step = 0; due_tick = acknowledged = None
    previous = data['completed_tick']
    if tuple(words.values()) != ((0, 1, 0, 0) if data['sample'] == 1 else (0, 0, 0, 0)):
        raise UnsupportedDevice('Capture paired ACK initial latches changed')
    for event in data['events']:
        if (data['state'] == 'pending' or type(event) is not dict
                or set(event) != {'operation', 'cpu', 'address', 'word', 'tick'}
                or channel >= 2 or type(event['tick']) is not int
                or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Capture paired ACK chronology or event identity changed')
        previous = event['tick']; clear = device.BASE + 0x88c + channel * 0x100; flag = clear + 0x68
        kind, address, word = event['operation'], event['address'], event['word']
        if type(word) is not int or word not in (0, 1):
            raise UnsupportedDevice('Capture ACK WORD domain changed')
        if kind == 'response':
            if event['cpu'] is not None or step not in (2, 3) or due_tick is None or address != flag or word or previous < due_tick:
                raise UnsupportedDevice('Capture ACK clear requires its delayed hardware request')
            words[flag] = 0; due_tick = None
        else:
            if type(event['cpu']) is not int or event['cpu'] != 0:
                raise UnsupportedDevice('Capture ACK requires the real CPU0 ISR')
            if kind == 'write' and step == 0 and address == clear and word == 0: step = 1; words[clear] = 0
            elif kind == 'write' and step == 1 and address == flag and word == 1: step = 2; words[flag] = 1; due_tick = previous + device.ack_ticks
            elif kind == 'read' and step == 2 and address == clear and word == 0: step = 3
            elif kind == 'read' and step == 3 and address == flag and word == words[flag]:
                if word: step = 2
                else: channel += 1; step = 0; acknowledged = previous if channel == 2 else None
            else: raise UnsupportedDevice('Capture ACK skips the real ordered paired transactions')
    if (data['ack_words'] != words or data['ack_channel'] != channel or data['ack_step'] != step
            or data['ack_due_tick'] != due_tick or data['acknowledged_tick'] != acknowledged
            or (data['state'] == 'acknowledged') != (channel == 2)):
        raise UnsupportedDevice('Capture ACK retained projection differs from its event replay')


def _validate_data(device, data):
    if (type(data) is not dict or set(data) != {'profile', 'sample', 'state', 'armed_tick', 'due_tick',
            'completed_tick', 'acknowledged_tick', 'latch', 'latch_sha256', 'prior_ack_words', 'ack_words',
            'ack_channel', 'ack_step', 'ack_due_tick', 'events', 'state_sha256'}
            or data['sample'] not in (1, 2) or type(data['sample']) is not int
            or data['profile'] != (DATA_PROFILE if data['sample'] == 1 else FINAL_DATA_PROFILE)
            or data['state'] not in ('pending', 'delivered', 'acknowledged')
            or data['state_sha256'] != hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})
            or data['latch_sha256'] != hp.digest(data['latch'])
            or data['latch']['activation'] != source_anchor(device)
            or data['latch']['initial_clock'] != _initial_epoch(device)
            or data['latch']['acquisition_start'] != _clock_epoch(_source(device).capture_acquisition_start_summary())
            or type(data['armed_tick']) is not int or not 0 <= data['armed_tick'] < data['due_tick'] == data['armed_tick'] + DATA_TICKS
            or type(data['events']) is not list or len(data['events']) > 4096):
        raise UnsupportedDevice('Capture scene data seal, clock, activation or timeline changed')
    if data['state'] == 'pending':
        if data['completed_tick'] is not None or data['acknowledged_tick'] is not None:
            raise UnsupportedDevice('Capture scene claims completion before its physical transport')
    elif type(data['completed_tick']) is not int or not data['due_tick'] <= data['completed_tick'] <= device.board.virtual_ticks:
        raise UnsupportedDevice('Capture scene DMA precedes its declared transport delay')
    _validate_ack(device, data)
    return data


def _validate_data_history(device, record):
    history = record.get('data_history', [])
    data = record.get('data')
    if type(history) is not list or len(history) > 1 or history and (data is None or data.get('sample') != 2):
        raise UnsupportedDevice('Capture scene history exceeds the exact two-completion request')
    for sample, item in enumerate((*history, data) if data is not None else (), 1):
        if item['sample'] != sample or sample == 1 and history and item['state'] != 'acknowledged':
            raise UnsupportedDevice('Capture scene completed history lost its first sample')
        _validate_data(device, item)


def data_anchor(device, sample=1):
    """Pure sealed completion projection; no Source or full owner recursion."""
    record = _owner(device)
    items = record.get('data_history', []) + [record.get('data')] if record is not None else []
    matches = [item for item in items if type(item) is dict and item.get('sample') == sample]
    if type(sample) is not int or sample not in (1, 2) or len(matches) != 1:
        raise UnsupportedDevice('Capture scene closure requires its unique sample identity')
    data = matches[0]
    if (data.get('state') != 'acknowledged'
            or data['state_sha256'] != hp.digest({key: value for key, value in data.items() if key != 'state_sha256'})
            or data['latch_sha256'] != hp.digest(data['latch'])
            or data['latch']['activation'] != source_anchor(device)):
        raise UnsupportedDevice('Capture scene closure requires the actual sealed paired ACK')
    _validate_ack(device, data)
    result = {key: deepcopy(data[key]) for key in ('profile', 'sample', 'state', 'completed_tick',
        'acknowledged_tick', 'ack_channel', 'ack_step', 'ack_due_tick', 'ack_words', 'state_sha256')}
    result.update({key: deepcopy(data['latch'][key]) for key in ('activation', 'initial_clock', 'input_sha256', 'bank')})
    return result


def _data_access(device, record, operation, cpu, address, size, value):
    data = record.get('data')
    if data is None or address not in {device.BASE + offset for offset in ACK_OFFSETS}: return False
    if (data['state'] != 'delivered' or cpu != 0 or type(cpu) is not int or type(size) is not int or size != 4):
        raise UnsupportedDevice('Capture ACK requires delivered scene data and the CPU0 WORD ISR')
    _validate_data(device, data)
    channel, step = data['ack_channel'], data['ack_step']
    clear = device.BASE + 0x88c + channel * 0x100; flag = clear + 0x68
    word = device.registers[address] if operation == 'read' else value
    if device.registers[address] != data['ack_words'][address]:
        raise UnsupportedDevice('Capture ACK hardware latch changed')
    if operation == 'write' and step == 0 and address == clear and word == 0: next_step = 1
    elif operation == 'write' and step == 1 and address == flag and word == 1: next_step = 2
    elif operation == 'read' and step == 2 and address == clear and word == 0: next_step = 3
    elif operation == 'read' and step == 3 and address == flag and word == data['ack_words'][address]: next_step = 2 if word else 0
    else: raise UnsupportedDevice('Capture ACK must perform the next real paired transaction')
    event = dict(operation=operation, cpu=cpu, address=address, word=word, tick=device.board.virtual_ticks)
    data['events'].append(event); data['ack_step'] = next_step
    if operation == 'write':
        device.registers[address] = word; data['ack_words'][address] = word
        if word == 1: data['ack_due_tick'] = event['tick'] + device.ack_ticks
    if operation == 'read' and step == 3 and not word:
        data['ack_channel'] += 1
        if data['ack_channel'] == 2: data.update(state='acknowledged', acknowledged_tick=event['tick'])
    _seal_data(data)
    return True


def _data_tick(device, record, board):
    data = record.get('data')
    if data is None or data['state'] == 'acknowledged': return False
    due_tick = data['ack_due_tick'] if data['ack_due_tick'] is not None else data['due_tick'] if data['state'] == 'pending' else None
    if due_tick is None or board.virtual_ticks < due_tick: return False
    validate(device)
    if data['state'] == 'pending':
        inputs = _source(device)._prefix(historical=True)
        if (inputs.event_enable_generation != 8 or inputs.registers.get(0x38c) != 0x10201
                or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
                or _read(board, 0x15d4908) != 5 or _read(board, 0x15f68a4) != (1 if data['sample'] == 1 else 3)):
            raise UnsupportedDevice('Capture scene live request or sensor gates changed before DMA')
        if any(device.registers[address] != word for address, word in data['latch']['desired'].items()):
            raise UnsupportedDevice('Capture scene desired configuration changed before DMA')
        if any(device.registers[address] != word for address, word in data['latch']['programmed_destinations'].items()):
            raise UnsupportedDevice('Capture scene destination changed before DMA')
        for span in data['latch']['plan']:
            board.write_memory(span['address'], bytes.fromhex(span['payload_hex']))
        data.update(state='delivered', completed_tick=board.virtual_ticks)
        _seal_data(data)
        board.queue_irq(0, 46)
        board.record('Declared capture scene numeric data delivered', sample=data['sample'],
            tick=board.virtual_ticks, limits='Consumer bytes precede the real IRQ46; no message, callback or result flag')
    else:
        address = device.BASE + 0x8f4 + data['ack_channel'] * 0x100
        device.registers[address] = 0; data['ack_words'][address] = 0; data['ack_due_tick'] = None
        data['events'].append(dict(operation='response', cpu=None, address=address, word=0, tick=board.virtual_ticks))
        _seal_data(data)
    return True


def _rotation_event(device, record, event):
    source = _source(device)
    key = 'acquisition_rotation' if record.get('data') is None else 'stop_rotation'
    stop = (source.capture_acquisition_start_summary() if key == 'acquisition_rotation'
            else source.capture_initialization_summary().get('finite_stop'))
    if (type(stop) is not dict or stop.get('state') not in ('asserted', 'acknowledged')
            or stop.get('asserted_tick') is None
            or key == 'stop_rotation' and (record['data'].get('sample') != 1
                                          or record['data'].get('state') != 'acknowledged')):
        raise UnsupportedDevice('Capture pointer rotation requires its explicit acquisition/finite-stop clock')
    fields = ('profile', 'generation', 'cpu', 'source', 'physical_irq', 'input_sha256', 'asserted_tick')
    epoch = {key: deepcopy(stop[key]) for key in fields}
    rotation = record.get(key)
    if rotation is None: rotation = dict(epoch=epoch, events=[])
    if rotation['epoch'] != epoch:
        raise UnsupportedDevice('Capture finite-stop rotation identity changed')
    bank = 0x5fecd410 if key == 'acquisition_rotation' else 0x5fecaa60
    expected = [(device.BASE + offset, bank + delta) for offset, delta in
        zip((0x508, 0x500, 0x510, 0x514, 0x518, 0x51c, 0x520, 0x50c, 0x504),
            (0x850, 0, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0, 0x1850, 0x800))]
    step = len(rotation['events'])
    if step >= 9 or (event['cpu'], event['address'], event['word']) != (0, *expected[step]):
        raise UnsupportedDevice('Capture clock permits exactly its one real alternating bank rotation')
    event = dict(event, operation='rotation')
    rotation['events'].append(event); record[key] = rotation
    return event


def _validate_rotation(device, record):
    for key in ('acquisition_rotation', 'stop_rotation'):
        _validate_one_rotation(device, record, key)


def _validate_one_rotation(device, record, key):
    rotation = record.get(key)
    if rotation is None: return None
    if (type(rotation) is not dict or set(rotation) != {'epoch', 'events'}
            or type(rotation['events']) is not list or not 1 <= len(rotation['events']) <= 9):
        raise UnsupportedDevice('Capture stop rotation plain ledger changed')
    source = _source(device)
    stop = (source.capture_acquisition_start_summary() if key == 'acquisition_rotation'
            else source.capture_initialization_summary().get('finite_stop'))
    if stop is None or rotation['epoch'] != {key: stop[key] for key in rotation['epoch']}:
        raise UnsupportedDevice('Capture stop rotation lost its physical clock identity')
    bank = 0x5fecd410 if key == 'acquisition_rotation' else 0x5fecaa60
    expected = [(device.BASE + offset, bank + delta) for offset, delta in
        zip((0x508, 0x500, 0x510, 0x514, 0x518, 0x51c, 0x520, 0x50c, 0x504),
            (0x850, 0, 0x1990, 0x1d90, 0x2190, 0x21b0, 0x23b0, 0x1850, 0x800))]
    previous = rotation['epoch']['asserted_tick']
    for event, (address, word) in zip(rotation['events'], expected):
        if (event not in record['accesses']
                or (event['operation'], event['cpu'], event['address'], event['word']) != ('rotation', 0, address, word)
                or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Capture stop rotation lost its actual pointer access')
        previous = event['tick']
    return rotation


def _retirement_access(device, record, operation, cpu, address, size, value):
    close = record.get('retirement')
    sequence = device.FRESH_SHUTDOWN_SEQUENCE
    if close is None:
        data = record.get('data')
        if (type(data) is not dict or data.get('sample') != 2 or data.get('state') != 'acknowledged'
                or (operation, address, device.registers.get(address) if operation == 'read' else value)
                    != ('read', device.BASE + 4, 0x33173317)):
            return False
        if _read(device.board, 0x15f68a4) != 0:
            #11F8EF8 clears client3 before1202870/10CB2F4's real close.
            raise UnsupportedDevice('Capture numeric retirement precedes the real finite client closure')
        close = dict(state='in-progress', accesses=[], completed_tick=None,
                     activation=source_anchor(device), data=data_anchor(device, 2))
        record['retirement'] = close
    if close['state'] == 'completed': return False
    step = len(close['accesses']); kind, offset, expected = sequence[step]
    word = device.registers.get(address) if operation == 'read' else value
    if (cpu, operation, address, word) != (0, kind, device.BASE + offset, expected):
        raise UnsupportedDevice('Capture finite numeric retirement must follow the whole eight-access driver')
    event = dict(operation=operation, cpu=cpu, address=address, word=word,
                 old_word=device.registers[address], tick=device.board.virtual_ticks)
    close['accesses'].append(event)
    projection = {**record['state'], 'words': dict(record['state']['words'])}
    af._transition(device, projection, event, data=_effects(record))
    record['accesses'].append(event); record['state'] = projection
    if operation == 'write': device.registers[address] = word
    if len(close['accesses']) == len(sequence): close.update(state='completed', completed_tick=event['tick'])
    return True


def _validate_retirement(device, record):
    close = record.get('retirement')
    if close is None: return None
    sequence = device.FRESH_SHUTDOWN_SEQUENCE
    if (type(close) is not dict or set(close) != {'state', 'accesses', 'completed_tick', 'activation', 'data'}
            or type(close['accesses']) is not list or not 1 <= len(close['accesses']) <= len(sequence)
            or close['state'] != ('completed' if len(close['accesses']) == len(sequence) else 'in-progress')
            or close['activation'] != source_anchor(device) or close['data'] != data_anchor(device, 2)):
        raise UnsupportedDevice('Capture finite retirement history changed')
    for event, (operation, offset, word) in zip(close['accesses'], sequence):
        if event not in record['accesses'] or (event['cpu'], event['operation'], event['address'], event['word']) != (0, operation, device.BASE + offset, word):
            raise UnsupportedDevice('Capture finite retirement lost an exact stock access')
    expected_tick = close['accesses'][-1]['tick'] if close['state'] == 'completed' else None
    if close['completed_tick'] != expected_tick:
        raise UnsupportedDevice('Capture retirement completion lost its final hardware access')
    return close


RELEASE_OWNER_ZERO = (0x15f63a8, 0x15f64a4, 0x15f66a4, 0x15f68a4, 0x15f68a8,
                      0x15f6bf4, 0x15f6e5c, 0x15f6e60, 0x15f6e6c, 0x160e4f8)
RELEASE_OWNER = (*RELEASE_OWNER_ZERO, 0x15f63a4, 0x15f6e50, 0x15233bc)


def _release_access(device, record, operation, cpu, address, size, value):
    """Actual capture11F9810→10CAD2C(2,1E)→10CA7F8(FC,0)."""
    release = record.get('allocation_release')
    sequence = af.ALLOCATION_RELEASE_SEQUENCE
    if release is None:
        if (operation, address, value) != ('write', device.GLOBAL, 0x10000000): return False
        close = record.get('retirement')
        if type(close) is not dict or close.get('state') != 'completed': return False
        prior = record['accesses'][-1:]
        from unicorn.arm_const import UC_ARM_REG_PC
        owner = {a: _read(device.board, a) for a in RELEASE_OWNER}
        if (type(close) is not dict or close.get('state') != 'completed'
                or cpu != 0 or device.board.cpus[0].reg_read(UC_ARM_REG_PC) != 0x9333cc
                or any(owner[a] for a in RELEASE_OWNER_ZERO)
                or owner[0x15f63a4] != 3 or owner[0x15f6e50] != 1 or not owner[0x15233bc]
                or (_read(device.board, 0x15d4908), _read(device.board, 0x15d4888, 1)) != (5, 0)
                or any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
                       or bank.active == 46 or bank.injected == 46 for bank in device.board.interrupts.banks)
                or len(prior) != 1 or (prior[0]['operation'], prior[0]['cpu'], prior[0]['address'], prior[0]['word'])
                   != ('read', 0, device.GLOBAL, 0x10000000)):
            raise UnsupportedDevice('Capture FC allocation release requires its real retired idle class/owner')
        validate(device)
        release = dict(state='in-progress', accesses=deepcopy(prior), completed_tick=None,
                       retirement_sha256=hp.digest(close), owner=owner)
    if release['state'] == 'completed': return False
    step = len(release['accesses']); kind, expected = sequence[step]
    old = device.registers[address]; word = old if operation == 'read' else value
    if (cpu, operation, address, word) != (0, kind, device.GLOBAL, expected):
        raise UnsupportedDevice('Capture FC release must follow its fifteen exact field accesses')
    event = dict(operation=operation, cpu=cpu, address=address, word=word, old_word=old,
                 tick=device.board.virtual_ticks)
    release = {**release, 'accesses': [*release['accesses'], event]}
    if len(release['accesses']) == len(sequence): release.update(state='completed', completed_tick=event['tick'])
    projection = {**record['state'], 'words': dict(record['state']['words'])}
    af._transition(device, projection, event, data={**_effects(record), 'allocation_release': release})
    record['allocation_release'] = release
    record['accesses'].append(event); record['state'] = projection
    if operation == 'write': device.registers[address] = word
    return True


def _validate_release(device, record):
    release = record.get('allocation_release')
    if release is None: return None
    sequence = af.ALLOCATION_RELEASE_SEQUENCE
    close = record.get('retirement')
    if (type(release) is not dict
            or set(release) != {'state', 'accesses', 'completed_tick', 'retirement_sha256', 'owner'}
            or type(close) is not dict or close.get('state') != 'completed'
            or release['retirement_sha256'] != hp.digest(close)
            or type(release['accesses']) is not list or not 2 <= len(release['accesses']) <= len(sequence)
            or release['state'] != ('completed' if len(release['accesses']) == len(sequence) else 'in-progress')
            or type(release['owner']) is not dict or set(release['owner']) != set(RELEASE_OWNER)
            or any(type(word) is not int or not 0 <= word < 1 << 32 for word in release['owner'].values())
            or any(release['owner'][a] for a in RELEASE_OWNER_ZERO)
            or release['owner'][0x15f63a4] != 3 or release['owner'][0x15f6e50] != 1
            or not release['owner'][0x15233bc]
            or release['completed_tick'] != (release['accesses'][-1]['tick'] if release['state'] == 'completed' else None)):
        raise UnsupportedDevice('Capture FC release lost its retired class/owner or finite ledger')
    previous = close['completed_tick']; old = sequence[0][1]
    for event, (operation, word) in zip(release['accesses'], sequence):
        if (event not in record['accesses']
                or (event['operation'], event['cpu'], event['address'], event['word'], event['old_word'])
                   != (operation, 0, device.GLOBAL, word, old)
                or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Capture FC release lost its exact fifteen-access transaction')
        old = word; previous = event['tick']
    return release
