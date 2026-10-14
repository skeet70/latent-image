"""One explicit contrast-AF numeric producer initialization.

The completed AE owner stays sealed under ``measurement``.  This owner
retains a separate terminal projection and the actual CPU0 reset, idle stops,
desired setters and 22-access descriptor/setup/enable transaction.  Its
default result is an enabled request awaiting physical input: no DMA bytes,
interrupt, callback, lens position or focus decision are supplied.
"""
from copy import copy, deepcopy

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from . import producer_measurement
from . import afs_focus_data


PROFILE = 'declared-afs-contrast-numeric-producer-unmeasured'
ACCESS_LIMIT = 16384
RESET_POLL_LIMIT = 65536
RETIREMENT_PROFILE = 'declared-afs-drained-contrast-numeric-retirement-unmeasured'
RELEASE_PROFILE = 'declared-afs-retired-preview-producer-release-unmeasured'
#1203540 first disables cached maskD4 through10CB5C8. Later10CAD2C
#usesmaskFC before freeing its allocation. Each contains desired selector
#changes only, and the stock XOR16/17 operations apply in both stages.
RELEASE_SEQUENCE = (
    ('read', 0x10030204), ('write', 0x10030200),
    ('read', 0x10030200), ('write', 0x10030000),
    ('read', 0x10030000), ('read', 0x10030000), ('write', 0x10020000),
    ('read', 0x10020000), ('read', 0x10020000), ('write', 0x10000000),
)
ALLOCATION_RELEASE_SEQUENCE = (
    ('read', 0x10000000), ('write', 0x10000000),
    ('read', 0x10000000), ('write', 0x10000000),
    ('read', 0x10000000), ('write', 0x10000000),
    ('read', 0x10000000), ('read', 0x10000000), ('write', 0x10000000),
    ('read', 0x10000000), ('read', 0x10000000), ('write', 0x10010000),
    ('read', 0x10010000), ('read', 0x10010000), ('write', 0x10030000),
)
RELEASE_STAGES = {'release': ('mode-disable', RELEASE_SEQUENCE, 0xd4),
                  'allocation_release': ('allocation-release', ALLOCATION_RELEASE_SEQUENCE, 0)}
STATE_KEYS = {'phase', 'reset_due_tick', 'reset_requested_tick',
              'reset_acknowledged_tick', 'idle_step', 'request_step',
              'request_started_tick', 'request_armed_tick',
              'request_enabled_tick', 'request_route', 'words'}


def _state(words):
    return dict(phase='awaiting-reset-read', reset_due_tick=None,
                reset_requested_tick=None, reset_acknowledged_tick=None,
                idle_step=0, request_step=0, request_started_tick=None,
                request_armed_tick=None, request_enabled_tick=None,
                request_route=None, words=dict(words))


def _record(device, terminal):
    """Pure construction; isolated caller tests declare their prior ABI."""
    return dict(profile=PROFILE, cpu=0, armed_tick=device.board.virtual_ticks,
                terminal=terminal, terminal_sha256=hp.digest(terminal),
                state=_state(terminal['registers']), accesses=[])


def _idle(device):
    return (device.busy is False and device.dma_busy is False
            and all(value is None for value in
                    (device.pending_stop, device.pending_data, device.pending_ack)))


def _ae_digest(device):
    measurement = device.rearm.get('measurement') if type(device.rearm) is dict else None
    if type(measurement) is not dict:
        raise UnsupportedDevice('Contrast AF requires the retained AE owner')
    return hp.digest(measurement)


def _terminal_view(device, record):
    """Validate the old owner at its sealed historical hardware projection."""
    terminal = record['terminal']
    if (type(terminal) is not dict
            or set(terminal) != {'registers', 'measurement_sha256', 'source'}
            or terminal.get('measurement_sha256') != _ae_digest(device)
            or type(terminal.get('registers')) is not dict
            or type(terminal.get('source')) is not dict
            or record['terminal_sha256'] != hp.digest(terminal)):
        raise UnsupportedDevice('Contrast AF terminal AE history changed')
    view = copy(device)
    view.registers = dict(terminal['registers'])
    view.rearm = {key: value for key, value in device.rearm.items() if key != 'afs_focus'}
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
        raise UnsupportedDevice('Contrast AF terminal view requires one retained source')
    # The previous driver really unregistered IRQ46.  Its copied validation
    # reads that historical fact, while this new owner's route is checked live.
    for bank in view.board.interrupts.banks:
        bank.enabled &= ~(1 << 46)
        bank.pending &= ~(1 << 46)
        if bank.active == 46: bank.active = None
        if bank.injected == 46: bank.injected = None
    producer_measurement.validate(view)
    return view


def arm(device, profile=PROFILE):
    if (type(profile) is not str or profile != PROFILE
            or type(device.rearm) is not dict or 'afs_focus' in device.rearm):
        raise UnsupportedDevice('Unknown or repeated contrast AF producer admission')
    device._validate_rearm()
    retirement = producer_measurement.retirement_anchor(device)
    sources = [item for item in device.board.devices if type(item).__name__ == 'AfsFrameSource']
    if (retirement['state'] != 'completed' or not _idle(device)
            or len(sources) != 1 or sources[0].board is not device.board
            or sources[0].request is None or sources[0].request.get('state') != 'acknowledged'
            or sources[0].request.get('frame') != 9
            or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured'
            or not device._global_valid(device.registers[device.GLOBAL])
            or not device.registers[device.GLOBAL] & 0x10000000
            or any(device.registers[device.BASE + offset] != word for offset, word in
                   ((4, 0x33170000), (8, 0x10000), (0x884, 0), (0x890, 0), (0x8f8, 0)))
            or any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
                   or bank.active == 46 or bank.injected == 46
                   for bank in device.board.interrupts.banks)):
        raise UnsupportedDevice('Contrast AF admission requires completed AE controls, idle DMA and retired IRQ46')
    terminal = dict(registers=dict(device.registers), measurement_sha256=_ae_digest(device),
                    source=deepcopy(sources[0].request))
    record = _record(device, terminal)
    device.rearm['afs_focus'] = record
    device.board.record('Contrast AF producer explicitly admitted', profile=profile,
                       limits='Separate terminal AE projection; admission supplies no DMA, IRQ, callback or result')
    return deepcopy(record)


def _transition(device, state, event, prelude=None, data=None):
    """Pure hardware transaction projection, shared by live access and replay."""
    if (type(event) is not dict
            or set(event) != {'operation', 'cpu', 'address', 'word', 'old_word', 'tick'}
            or type(event['address']) is not int or event['address'] not in state['words']
            or type(event['word']) is not int or not 0 <= event['word'] < 1 << 32
            or type(event['old_word']) is not int or event['old_word'] != state['words'][event['address']]
            or type(event['tick']) is not int or not 0 <= event['tick'] <= device.board.virtual_ticks):
        raise UnsupportedDevice('Contrast AF producer access identity/projection changed')
    operation, address, word = event['operation'], event['address'], event['word']
    phase = state['phase']
    if operation == 'rotation':
        jobs = [*data.get('data_history', []), data.get('data')] if type(data) is dict else []
        rotations = [item['rotation'] for item in jobs
                     if type(item) is dict and type(item.get('rotation')) is dict]
        if (phase != 'enabled-awaiting-input'
                or sum(event in rotation['events'] for rotation in rotations) != 1 or event['cpu'] != 0):
            raise UnsupportedDevice('Contrast AF pointer store lost its qualified source rotation')
    elif operation == 'response':
        if (event['cpu'] is not None or phase != 'resetting' or address != device.GLOBAL
                or word != 0x10000000 or event['tick'] < state['reset_due_tick']):
            raise UnsupportedDevice('Contrast AF reset response lacks its actual clear/delay')
        state.update(reset_acknowledged_tick=event['tick'], phase='awaiting-reset-poll')
    else:
        if type(event['cpu']) is not int or event['cpu'] != 0 or operation not in ('read', 'write'):
            raise UnsupportedDevice('Contrast AF producer requires CPU0 WORD transactions')
        if operation == 'read' and word != event['old_word']:
            raise UnsupportedDevice('Contrast AF passive readback differs from its retained word')
        if phase == 'awaiting-reset-read':
            if (operation != 'read' or address != device.GLOBAL
                    or not device._global_valid(word) or not word & 0x10000000):
                raise UnsupportedDevice('Contrast AF must begin with stock GLOBAL bit28 read')
            state['phase'] = 'awaiting-reset-clear'
        elif phase == 'awaiting-reset-clear':
            if (operation, address, word) != ('write', device.GLOBAL, 0):
                raise UnsupportedDevice('Contrast AF must clear GLOBAL after its initial read')
            due = event['tick'] + device.stop_ticks
            if type(device.stop_ticks) is not int or not 1 <= device.stop_ticks or due >= 1 << 64:
                raise UnsupportedDevice('Contrast AF reset delay escapes the virtual clock')
            state.update(reset_requested_tick=event['tick'], reset_due_tick=due, phase='resetting')
        elif phase in ('resetting', 'awaiting-reset-poll'):
            expected = 0 if phase == 'resetting' else 0x10000000
            if (operation, address, word) != ('read', device.GLOBAL, expected):
                raise UnsupportedDevice('Contrast AF reset requires its exact clear/poll response')
            if phase == 'awaiting-reset-poll': state['phase'] = 'idle-stop'
        elif (phase == 'idle-stop' and state['idle_step'] == 0
                and prelude is not None and event['tick'] == prelude['tick']):
            if (operation, address, word) != ('write', device.GLOBAL + 4, 0):
                raise UnsupportedDevice('Contrast AF mode prelude lost its first whole geometry setter')
            state['phase'] = 'mode-configuration'
        elif phase == 'mode-configuration' and operation == 'write' and address == device.GLOBAL and word == 0:
            #11F70E8 resets the global producer (mask4), then programs its
            #mode geometry without DMA stops. If stock code invalidates its
            #cached reset,11F7A70 resets again before the mask2 idle stops.
            due = event['tick'] + device.stop_ticks
            if (not event['old_word'] & 0x10000000 or type(device.stop_ticks) is not int
                    or device.stop_ticks < 1 or due >= 1 << 64):
                raise UnsupportedDevice('Contrast AF second initializer reset lacks its ready mode prelude')
            state.update(reset_requested_tick=event['tick'], reset_due_tick=due,
                         reset_acknowledged_tick=None, phase='resetting')
        elif (phase == 'mode-configuration'
                and (operation, address, word) == (device.PREVIEW_IDLE_STOP[0][0],
                    device.BASE + device.PREVIEW_IDLE_STOP[0][1], device.PREVIEW_IDLE_STOP[0][2])):
            #12018B8 records17D54E0=1 after the mode reset.11F7A70
            #therefore skips a second GLOBAL reset unless stock code has
            #invalidated that cache, and begins the mandatory DMA stops.
            state.update(phase='idle-stop', idle_step=1)
        elif phase == 'idle-stop':
            kind, offset, value = device.PREVIEW_IDLE_STOP[state['idle_step']]
            if (operation, address, word) != (kind, device.BASE + offset, value):
                raise UnsupportedDevice('Contrast AF idle stop differs from the whole92CC5C driver')
            state['idle_step'] += 1
            if state['idle_step'] == len(device.PREVIEW_IDLE_STOP): state['phase'] = 'configuration'
        elif (type(data) is dict and type(data.get('retirement')) is dict
                and event in data['retirement']['accesses']):
            close = data['retirement']
            index = close['accesses'].index(event)
            kind, offset, expected = device.FRESH_SHUTDOWN_SEQUENCE[index]
            if (phase != 'enabled-awaiting-input' or event['cpu'] != 0
                    or (operation, address, word) != (kind, device.BASE + offset, expected)):
                raise UnsupportedDevice('Contrast AF retirement differs from the whole idle stop driver')
        elif (type(data) is dict and any(type(data.get(key)) is dict
                and event in data[key]['accesses'] for key in RELEASE_STAGES)):
            key = next(key for key in RELEASE_STAGES if type(data.get(key)) is dict
                       and event in data[key]['accesses'])
            index = data[key]['accesses'].index(event)
            kind, expected = RELEASE_STAGES[key][1][index]
            if (phase != 'enabled-awaiting-input' or event['cpu'] != 0
                    or (operation, address, word) != (kind, device.GLOBAL, expected)):
                raise UnsupportedDevice('Retired preview release differs from the whole10CA7F8 driver')
        elif (type(data) is dict and data.get('retirement') is not None and operation == 'write'
                and event['tick'] >= data['retirement']['started_tick']):
            raise UnsupportedDevice('Retired contrast AF requires a separate actual reinitialization')
        elif phase in ('mode-configuration', 'configuration', 'enabled-awaiting-input') and address in device.KNOWN_DESIRED_CONFIG:
            if ((operation == 'read' and address not in device.READABLE)
                    or operation == 'write' and not device._known_pair_valid(address, event['old_word'], word)):
                raise UnsupportedDevice('Contrast AF excludes unaudited desired setters')
        elif phase in ('configuration', 'activation'):
            sequence = device._preview_request_sequence()
            step = state['request_step']; kind, target, expected = sequence[step]
            if (operation != kind or address != target
                    or (not device._config_valid(address, word) if expected is None else word != expected)
                    or address == device.BASE + 0x524 and word != 0x200):
                raise UnsupportedDevice('Contrast AF requires its next exact descriptor/setup/enable access')
            if step == 0: state['request_started_tick'] = event['tick']
            state['request_step'] += 1
            state['phase'] = 'activation'
            if state['request_step'] == 18: state['request_armed_tick'] = event['tick']
            if state['request_step'] == 22:
                state.update(request_enabled_tick=event['tick'], phase='enabled-awaiting-input')
        elif phase in ('mode-configuration', 'enabled-awaiting-input') and operation == 'read' and address in device.READABLE:
            pass
        else:
            raise UnsupportedDevice('Contrast AF numeric output/rotation requires a separate physical-input law')
    state['words'][address] = word


def _plain(device, record):
    keys = {'profile', 'cpu', 'armed_tick', 'terminal', 'terminal_sha256', 'state', 'accesses'}
    expected_words = record.get('state', {}).get('words') if type(record) is dict else None
    if type(record) is dict and record.get('data') is not None:
        words = record['data'].get('ack_words') if type(record['data']) is dict else None
        if (type(words) is not dict or set(words) != {device.BASE + offset for offset in afs_focus_data.ACK_OFFSETS}
                or any(type(word) is not int or word not in (0, 1) for word in words.values())
                or type(expected_words) is not dict):
            raise UnsupportedDevice('Contrast AF numeric ACK projection changed')
        expected_words = {**expected_words, **words}
    if (type(record) is not dict
            or set(record) - {'mode_prelude', 'mode_prelude_sha256', 'data', 'data_history', 'retirement', 'release', 'allocation_release'} != keys
            or ('mode_prelude' in record) != ('mode_prelude_sha256' in record)
            or record['profile'] != PROFILE or type(record['cpu']) is not int or record['cpu'] != 0
            or type(record['armed_tick']) is not int or not 0 <= record['armed_tick'] <= device.board.virtual_ticks
            or type(record['accesses']) is not list or len(record['accesses']) > ACCESS_LIMIT + RESET_POLL_LIMIT
            or type(record['state']) is not dict or set(record['state']) != STATE_KEYS
            or type(record['state']['words']) is not dict or not _idle(device)
            or expected_words != device.registers
            or record['state']['phase'] not in ('awaiting-reset-read', 'awaiting-reset-clear', 'resetting',
                 'awaiting-reset-poll', 'idle-stop', 'mode-configuration', 'configuration', 'activation', 'enabled-awaiting-input')
            or any(type(record['state'][key]) is not int for key in ('idle_step', 'request_step'))
            or not 0 <= record['state']['idle_step'] <= len(device.PREVIEW_IDLE_STOP)
            or not 0 <= record['state']['request_step'] <= 22):
        raise UnsupportedDevice('Contrast AF current owner/plain projection changed')
    prelude = record.get('mode_prelude')
    if ('mode_prelude' in record and prelude is None
            or record['state']['phase'] == 'mode-configuration' and prelude is None):
        raise UnsupportedDevice('Contrast AF mode configuration lost its retained prelude')
    if prelude is not None and (type(prelude) is not dict
            or set(prelude) != {'tick', 'access_index', 'af_state', 'client'}
            or type(prelude['tick']) is not int or not record['armed_tick'] <= prelude['tick'] <= device.board.virtual_ticks
            or type(prelude['access_index']) is not int or not 0 <= prelude['access_index'] < len(record['accesses'])
            or prelude['af_state'] != 2 or prelude['client'] != 0
            or record.get('mode_prelude_sha256') != hp.digest(prelude)):
        raise UnsupportedDevice('Contrast AF mode prelude identity changed')
    if prelude is not None:
        event = record['accesses'][prelude['access_index']]
        if (type(event) is not dict or (event.get('operation'), event.get('cpu'), event.get('address'),
                event.get('word'), event.get('tick')) != ('write', 0, device.GLOBAL + 4, 0, prelude['tick'])):
            raise UnsupportedDevice('Contrast AF mode prelude lost its actual geometry access')
    return record


def validate(device, *, check_history=True):
    if type(device.rearm) is dict and 'capture_metstats' in device.rearm:
        from .producer_capture_metstats import historical_af_view
        return validate(historical_af_view(device), check_history=check_history)
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if record is None: return None
    _plain(device, record)
    if not check_history: return record
    _terminal_view(device, record)
    state = _state(record['terminal']['registers'])
    previous = record['armed_tick']; polls = ordinary = 0
    prior_event = None
    for index, event in enumerate(record['accesses']):
        if type(event) is not dict or type(event.get('tick')) is not int or not previous <= event['tick']:
            raise UnsupportedDevice('Contrast AF whole transaction chronology changed')
        if state['phase'] == 'resetting' and event.get('operation') == 'read': polls += 1
        else: ordinary += 1
        if polls > RESET_POLL_LIMIT or ordinary > ACCESS_LIMIT:
            raise UnsupportedDevice('Contrast AF whole transaction exceeds its bounded lifetime')
        if record.get('mode_prelude', {}).get('access_index') == index and event['tick'] != record['mode_prelude']['tick']:
            raise UnsupportedDevice('Contrast AF mode prelude lost its actual first setter')
        if (state['phase'] == 'mode-configuration'
                and (event['operation'], event['address'], event['word']) == ('write', device.GLOBAL, 0)
                and (prior_event is None or (prior_event['operation'], prior_event['address'], prior_event['word'])
                     != ('read', device.GLOBAL, event['old_word']))):
            raise UnsupportedDevice('Contrast AF initializer clear must follow its ready GLOBAL read')
        _transition(device, state, event, record.get('mode_prelude'), record); previous = event['tick']
        prior_event = event
    state['request_route'] = record['state']['request_route']
    if state != record['state']:
        raise UnsupportedDevice('Contrast AF replay differs from its current transaction projection')
    _route(device, state)
    if record.get('data') is not None: afs_focus_data.validate(device)
    _retirement_validate(device, record)
    _release_validate(device, record)
    return record


def _route(device, state):
    step = state['request_step']
    record = device.rearm.get('afs_focus', {})
    data = record.get('data')
    close = record.get('retirement')
    if close is not None and close.get('state') == 'completed':
        #92F6EC writes8F8 before the real kernel unregister. Retain the
        #registered route until that ARM call removes it; afterwards only
        #the exact idle absence is valid. The activation route stays sealed.
        banks = device.board.interrupts.banks
        if any(bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46 for bank in banks):
            raise UnsupportedDevice('Retired contrast AF cannot own an in-flight IRQ46')
        if not any(bank.enabled & (1 << 46) for bank in banks): return None
    if step >= 16 and data is not None:
        from .afs_statistics_frame import _route as numeric_route
        numeric_route(device, pending=data.get('state') in ('delivered', 'acknowledged'))
        route = dict(cpu=0, vector=0x92c8cc, priority=13, source_configuration=1)
    else: route = device._preview_request_route(0, step >= 16)
    if (step < 15 and route is not None
            or state['request_route'] != (route if step >= 16 else None)):
        raise UnsupportedDevice('Contrast AF actual IRQ46 registration order changed')
    return route


def access(device, operation, core, address, size, value=None):
    record = validate(device, check_history=False)
    if record is None: return False
    if type(size) is not int or size != 4:
        raise UnsupportedDevice('Contrast AF producer only admits WORD transactions')
    state = record['state']; step = state['request_step']
    if record.get('retirement') is not None: _retirement_validate(device, record)
    if record.get('release') is not None: _release_validate(device, record)
    _route(device, state)
    if _retirement_access(device, record, operation, core, address, size, value): return True
    if _release_access(device, record, operation, core, address, size, value): return True
    if record.get('data') is not None and afs_focus_data.access(device, operation, core, address, size, value):
        return True
    if state['phase'] == 'activation':
        route = device._preview_request_route(0, step >= 15)
        if step < 15 and route is not None:
            raise UnsupportedDevice('Contrast AF IRQ46 registration precedes its stock driver')
    else: route = state['request_route']
    old = device.registers.get(address)
    word = old if operation == 'read' else value
    if operation == 'write' and address in device.DESTINATIONS and state['request_step'] == 22:
        event = afs_focus_data.rotation_event(device, core, address, size, word)
    else:
        event = dict(operation=operation, cpu=core, address=address, word=word,
                     old_word=old, tick=device.board.virtual_ticks)
    if (state['phase'] == 'mode-configuration' and (operation, address, word) == ('write', device.GLOBAL, 0)
            and (not record['accesses'] or (record['accesses'][-1]['operation'], record['accesses'][-1]['address'],
                   record['accesses'][-1]['word']) != ('read', device.GLOBAL, old))):
        raise UnsupportedDevice('Contrast AF initializer clear must follow its ready GLOBAL read')
    prelude = record.get('mode_prelude')
    if (prelude is None and state['phase'] == 'idle-stop' and state['idle_step'] == state['request_step'] == 0
            and (operation, address, word) == ('write', device.GLOBAL + 4, 0)):
        client = int.from_bytes(device.board.read_memory(0x15f68a4, 4), 'little')
        af_state = int.from_bytes(device.board.read_memory(0x16223f8, 1), 'little')
        if client != 0 or af_state != 2:
            raise UnsupportedDevice('Contrast AF mode prelude requires the actual state2 idle client')
        prelude = dict(tick=event['tick'], access_index=len(record['accesses']), af_state=af_state, client=client)
    projection = {**state, 'words': dict(state['words'])}
    _transition(device, projection, event, prelude, record)
    if projection['request_step'] >= 16: projection['request_route'] = route
    if len(record['accesses']) >= ACCESS_LIMIT + RESET_POLL_LIMIT:
        raise UnsupportedDevice('Contrast AF transaction budget exhausted')
    record['accesses'].append(event); record['state'] = projection
    if prelude is not None and 'mode_prelude' not in record:
        record.update(mode_prelude=prelude, mode_prelude_sha256=hp.digest(prelude))
    if operation == 'write':
        device.registers[address] = word
        device.writes.append(dict(cpu=core, address=hex(address), value=hex(word), old=hex(old)))
    device.board.record('Contrast AF producer transaction retained', **event, phase=projection['phase'],
                       limits='Audited passive/reset/activation contract only; no numeric output or AF result')
    return True


def _retirement_validate(device, record):
    """Pure bounded stop history, preserving every earlier activation/job."""
    close = record.get('retirement')
    if close is None: return None
    sequence = device.FRESH_SHUTDOWN_SEQUENCE
    keys = {'profile', 'state', 'step', 'access_index', 'accesses', 'activation',
            'data', 'started_tick', 'completed_tick'}
    if (type(close) is not dict or set(close) != keys or close['profile'] != RETIREMENT_PROFILE
            or type(close['step']) is not int or not 3 <= close['step'] <= len(sequence)
            or close['state'] != ('completed' if close['step'] == len(sequence) else 'in-progress')
            or type(close['access_index']) is not int or close['access_index'] < 0
            or type(close['accesses']) is not list or len(close['accesses']) != close['step']
            or type(close['activation']) is not dict or type(close['data']) is not dict
            or record['accesses'][close['access_index']:close['access_index'] + close['step']] != close['accesses']
            or close['activation'] != source_anchor(device)
            or close['data'] != afs_focus_data.anchor(device, close['data'].get('pair_id'))
            or close['started_tick'] != close['accesses'][0]['tick']
            or close['completed_tick'] != (close['accesses'][-1]['tick'] if close['state'] == 'completed' else None)):
        raise UnsupportedDevice('Contrast AF retirement lost its exact closed data/driver identity')
    words = {device.BASE + offset: word for offset, word in device.FRESH_SHUTDOWN_PRIOR.items()}
    previous = close['data']['acknowledged_tick']
    for event, (kind, offset, expected) in zip(close['accesses'], sequence):
        address = device.BASE + offset
        if (type(event) is not dict or set(event) != {'operation', 'cpu', 'address', 'word', 'old_word', 'tick'}
                or (event['operation'], event['cpu'], event['address'], event['word'], event['old_word'])
                     != (kind, 0, address, expected, words[address])
                or type(event['tick']) is not int or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Contrast AF retirement reordered its whole92CD04/92F6EC accesses')
        previous = event['tick']; words[address] = expected
    return close


def _release_validate(device, record):
    for key in RELEASE_STAGES: _release_validate_stage(device, record, key)


def _release_validate_stage(device, record, key):
    """One bounded final selector release; all old data/stop history stays sealed."""
    release = record.get(key)
    if release is None:
        if key in record:
            raise UnsupportedDevice('Retired preview producer release marker was removed')
        return None
    kind, sequence, mask = RELEASE_STAGES[key]
    prior = record.get('release') if key == 'allocation_release' else None
    close = record.get('retirement')
    keys = {'profile', 'state', 'step', 'access_index', 'accesses',
            'kind', 'prior_sha256', 'retirement_sha256', 'owner', 'started_tick', 'completed_tick'}
    owner = release.get('owner') if type(release) is dict else None
    if (type(release) is not dict or set(release) != keys or release['profile'] != RELEASE_PROFILE
            or type(close) is not dict or close.get('state') != 'completed'
            or release['kind'] != kind
            or key == 'allocation_release' and (type(prior) is not dict or prior.get('state') != 'completed')
            or release['prior_sha256'] != (hp.digest(prior) if prior is not None else None)
            or release['retirement_sha256'] != hp.digest(close)
            or type(release['step']) is not int or not 2 <= release['step'] <= len(sequence)
            or release['state'] != ('completed' if release['step'] == len(sequence) else 'in-progress')
            or type(release['access_index']) is not int or release['access_index'] < 0
            or type(release['accesses']) is not list or len(release['accesses']) != release['step']
            or record['accesses'][release['access_index']:release['access_index'] + release['step']] != release['accesses']
            or type(owner) is not dict or set(owner) != {0x15f63a4, 0x15f64a4, 0x15f68a4, 0x15f68a8,
                    0x15f6bf4, 0x15f6e6c, 0x15f6e50, 0x15233bc, 0x160e4f8}
            or any(type(value) is not int for value in owner.values())
            or owner[0x15f63a4] != 3 or not 0 < owner[0x15233bc] < 1 << 32
            or owner[0x160e4f8] != mask
            or any(value != 0 for address, value in owner.items() if address not in (0x15f63a4, 0x15233bc, 0x160e4f8))
            or release['started_tick'] != release['accesses'][0].get('tick')
            or release['completed_tick'] != (release['accesses'][-1].get('tick') if release['state'] == 'completed' else None)):
        raise UnsupportedDevice('Retired preview producer release lost its owner/closed history')
    previous = prior['completed_tick'] if prior is not None else close['completed_tick']
    old = sequence[0][1]
    for event, (kind, word) in zip(release['accesses'], sequence):
        if (type(event) is not dict or set(event) != {'operation', 'cpu', 'address', 'word', 'old_word', 'tick'}
                or (event['operation'], event['cpu'], event['address'], event['word'], event['old_word'])
                   != (kind, 0, device.GLOBAL, word, old)
                or type(event['tick']) is not int or not previous <= event['tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('Retired preview producer release reordered the whole selector family')
        old = word; previous = event['tick']
    return release


def _release_access(device, record, operation, core, address, size, value):
    key = 'release'
    release = record.get(key)
    if release is not None and release['state'] == 'completed':
        key = 'allocation_release'; release = record.get(key)
        if release is not None and release['state'] == 'completed': return False
    kind, sequence, mask = RELEASE_STAGES[key]
    if release is None:
        if (operation, address, value) != ('write', device.GLOBAL, sequence[1][1]): return False
        close = record.get('retirement')
        if type(close) is not dict or close.get('state') != 'completed': return False
        read = lambda a: int.from_bytes(device.board.read_memory(a, 4), 'little')
        owner = {a: read(a) for a in (0x15f63a4, 0x15f64a4, 0x15f68a4, 0x15f68a8,
                                    0x15f6bf4, 0x15f6e6c, 0x15f6e50, 0x15233bc, 0x160e4f8)}
        prior = record['accesses'][-1:]
        from unicorn.arm_const import UC_ARM_REG_PC
        if (type(core) is not int or core != 0 or type(size) is not int or size != 4
                or type(close) is not dict or close.get('state') != 'completed'
                or not _idle(device) or owner[0x15f63a4] != 3 or not owner[0x15233bc]
                or owner[0x160e4f8] != mask
                or any(value != 0 for a, value in owner.items() if a not in (0x15f63a4, 0x15233bc, 0x160e4f8))
                or device.board.cpus[0].reg_read(UC_ARM_REG_PC) != 0x9333cc
                or any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
                       or bank.active == 46 or bank.injected == 46 for bank in device.board.interrupts.banks)
                or len(prior) != 1 or (prior[0].get('operation'), prior[0].get('cpu'),
                    prior[0].get('address'), prior[0].get('word')) != ('read', 0, device.GLOBAL, sequence[0][1])):
            raise UnsupportedDevice('Retired preview release requires the real idle10CA7F8 owner and retired IRQ46')
        validate(device)
        release = dict(profile=RELEASE_PROFILE, state='in-progress', step=1,
                       kind=kind, prior_sha256=(hp.digest(record['release']) if key == 'allocation_release' else None),
                       access_index=len(record['accesses']) - 1, accesses=deepcopy(prior),
                       retirement_sha256=hp.digest(close), owner=owner,
                       started_tick=prior[0]['tick'], completed_tick=None)
    kind, expected = sequence[release['step']]
    old = device.registers.get(address); word = old if operation == 'read' else value
    if (type(core) is not int or core != 0 or type(size) is not int or size != 4
            or (operation, address, word) != (kind, device.GLOBAL, expected)):
        raise UnsupportedDevice('Retired preview release requires its next exact CPU0 WORD access')
    release = {**release, 'accesses':list(release['accesses'])}
    event = dict(operation=operation, cpu=core, address=address, word=word,
                 old_word=old, tick=device.board.virtual_ticks)
    release['accesses'].append(event); release['step'] += 1
    if release['step'] == len(sequence):
        release.update(state='completed', completed_tick=device.board.virtual_ticks)
    projection = {**record['state'], 'words':dict(record['state']['words'])}
    _transition(device, projection, event, record.get('mode_prelude'), {**record, key:release})
    if len(record['accesses']) >= ACCESS_LIMIT + RESET_POLL_LIMIT:
        raise UnsupportedDevice('Contrast AF transaction budget exhausted')
    record[key] = release; record['accesses'].append(event); record['state'] = projection
    if operation == 'write':
        device.registers[address] = word
        device.writes.append(dict(cpu=core, address=hex(address), value=hex(word), old=hex(old)))
    device.board.record('Retired preview producer selector release retained', **event,
        state=release['state'], limits='Passive selector release only; no reset, input, data, IRQ, callback or AF result')
    return True


def _retirement_access(device, record, operation, core, address, size, value):
    close = record.get('retirement')
    sequence = device.FRESH_SHUTDOWN_SEQUENCE
    if close is not None and close['state'] == 'completed': return False
    if close is None:
        if (operation, address, value) != ('write', device.BASE + 4, 0x33170000): return False
        #The healthy warm boundary may already include the driver's two
        #reads. Adopt only those real retained accesses; never replay, skip
        #or manufacture CPU work when admitting the first stop store.
        prior = record['accesses'][-2:]
        current = record.get('data')
        read = lambda address, size: int.from_bytes(device.board.read_memory(address, size), 'little')
        if (type(core) is not int or core != 0 or type(size) is not int or size != 4
                or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured'
                or type(current) is not dict or current.get('state') != 'acknowledged'
                or current.get('rotation', {}).get('state') != 'completed'
                or any(device.registers[device.BASE + offset] != word for offset, word in device.FRESH_SHUTDOWN_PRIOR.items())
                or any(device.registers.get(device.BASE + offset) != 0 for offset in (0x984, 0x990, 0x9f8))
                or len(prior) != 2
                or [(event.get('operation'), event.get('cpu'), event.get('address'), event.get('word')) for event in prior]
                     != [('read', 0, device.BASE + 4, 0x33173317), ('read', 0, device.BASE + 8, 0x10000)]
                or any(bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46 for bank in device.board.interrupts.banks)
                or read(0x15f8828, 2) != 12 or read(0x16223f8, 1) != 13
                or read(0x15f68a4, 4) != 0 or read(0x15f68a8, 4) != 0x12fc2cc
                or read(0x15f6bf4, 4) != 4 or read(0x15f6e6c, 4) != 1):
            raise UnsupportedDevice('Contrast AF retirement requires a real normal stop and fully drained numerical owner')
        pair_id = current['latch']['pair_id']
        if (read(0x16223fc, 2) != pair_id or read(0x16223fe, 2) != pair_id
                or read(0x17a8eac, 1) != pair_id):
            raise UnsupportedDevice('Contrast AF stop awaits its actual source/data/calculator progress')
        from . import afs_frame_source
        pixels = [item for item in afs_frame_source.af_requests(device.board) if item.get('kind') == 'pixel']
        if len(pixels) != pair_id or any(item.get('consumed_tick') is None for item in pixels):
            raise UnsupportedDevice('Contrast AF drained stop cannot cancel a future physical pixel')
        validate(device)
        close = dict(profile=RETIREMENT_PROFILE, state='in-progress', step=2,
                     access_index=len(record['accesses']) - 2, accesses=deepcopy(prior),
                     activation=source_anchor(device), data=afs_focus_data.anchor(device, pair_id),
                     started_tick=prior[0]['tick'], completed_tick=None)
    step = close['step']; kind, offset, expected = sequence[step]
    old = device.registers.get(address); word = old if operation == 'read' else value
    if (type(core) is not int or core != 0 or type(size) is not int or size != 4
            or (operation, address, word) != (kind, device.BASE + offset, expected)):
        raise UnsupportedDevice('Contrast AF stop requires its next exact CPU0 WORD access')
    event = dict(operation=operation, cpu=core, address=address, word=word, old_word=old, tick=device.board.virtual_ticks)
    close['accesses'].append(event); close['step'] += 1
    if close['step'] == len(sequence): close.update(state='completed', completed_tick=device.board.virtual_ticks)
    record['retirement'] = close
    projection = {**record['state'], 'words': dict(record['state']['words'])}
    _transition(device, projection, event, record.get('mode_prelude'), record)
    record['accesses'].append(event); record['state'] = projection
    if operation == 'write':
        device.registers[address] = word
        device.writes.append(dict(cpu=core, address=hex(address), value=hex(word), old=hex(old)))
    device.board.record('Contrast AF drained numerical retirement retained', **event, state=close['state'],
        limits='Existing idle-zero convention; no cancelled input, data, IRQ, callback or AF result supplied')
    return True


def tick(device, board):
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if record is not None and record.get('data') is not None: return afs_focus_data.tick(device, board)
    if record is None or record.get('state', {}).get('phase') != 'resetting': return False
    if board is not device.board:
        raise UnsupportedDevice('Contrast AF reset belongs to another board')
    state = record['state']
    deadline = _deadline(device, record)
    if board.virtual_ticks < deadline: return True
    validate(device)
    event = dict(operation='response', cpu=None, address=device.GLOBAL, word=0x10000000,
                 old_word=device.registers[device.GLOBAL], tick=board.virtual_ticks)
    projection = {**state, 'words': dict(state['words'])}; _transition(device, projection, event, record.get('mode_prelude'))
    record['accesses'].append(event); record['state'] = projection
    device.registers[device.GLOBAL] = 0x10000000
    board.record('Contrast AF GLOBAL reset acknowledged', profile=PROFILE,
                 limits='Existing declared delayed bit28 law only; no data, IRQ or callback')
    return True


def due(device):
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if record is not None and record.get('data') is not None: return afs_focus_data.due(device)
    if record is None or record.get('state', {}).get('phase') != 'resetting': return None
    return _deadline(device, record)


def _deadline(device, record):
    _plain(device, record)
    state = record['state']; requested = state['reset_requested_tick']; deadline = state['reset_due_tick']
    if (type(requested) is not int or not record['armed_tick'] <= requested <= device.board.virtual_ticks
            or type(deadline) is not int or type(device.stop_ticks) is not int
            or deadline != requested + device.stop_ticks or not requested < deadline < 1 << 64
            or state['reset_acknowledged_tick'] is not None or state['words'][device.GLOBAL] != 0):
        raise UnsupportedDevice('Contrast AF reset deadline differs from its actual clear')
    return deadline


def source_anchor(device, expected=None):
    """Pure current activation anchor, with no Source/public history recursion."""
    if type(device.rearm) is dict and 'capture_metstats' in device.rearm:
        from .producer_capture_metstats import historical_af_view
        return source_anchor(historical_af_view(device), expected)
    record = device.rearm.get('afs_focus') if type(device.rearm) is dict else None
    if record is not None: _plain(device, record)
    if (record is None or record.get('profile') != PROFILE or type(record.get('state')) is not dict
            or record['state'].get('request_step') != 22
            or record['state'].get('phase') != 'enabled-awaiting-input'
            or type(record.get('terminal')) is not dict
            or record.get('terminal_sha256') != hp.digest(record['terminal'])):
        raise UnsupportedDevice('Contrast AF source requires a complete real activation')
    state = record['state']
    # Region/parameter setters after activation describe desired windows.
    # They must not rewrite an earlier sensor request's activation identity.
    # Their own current projection is latched separately by the data owner.
    endings = [index for index, event in enumerate(record['accesses'])
               if event.get('tick') == state['request_enabled_tick']
               and (event.get('operation'), event.get('address'), event.get('word'))
                   == ('write', device.BASE + 8, 0x10000)]
    if len(endings) != 1:
        raise UnsupportedDevice('Contrast AF activation lost its real enable event')
    prefix = record['accesses'][:endings[0] + 1]
    projected = _state(record['terminal']['registers'])
    previous = record['armed_tick']
    for event in prefix:
        if event['tick'] < previous:
            raise UnsupportedDevice('Contrast AF activation anchor chronology changed')
        _transition(device, projected, event, record.get('mode_prelude')); previous = event['tick']
    if (projected['request_step'] != 22 or projected['phase'] != 'enabled-awaiting-input'
            or projected['request_enabled_tick'] != state['request_enabled_tick']):
        raise UnsupportedDevice('Contrast AF activation anchor lost its complete driver')
    anchor = dict(enabled_tick=state['request_enabled_tick'], sha256=hp.digest(dict(
        profile=record['profile'], terminal_sha256=record['terminal_sha256'],
        accesses=prefix, route=state['request_route'])))
    if expected is not None and (type(expected) is not dict or set(expected) != {'enabled_tick', 'sha256'}
                                 or expected != anchor):
        raise UnsupportedDevice('Contrast AF activation selector changed')
    return anchor


def summary(device):
    record = validate(device)
    if record is None: return None
    state = record['state']
    close = record.get('retirement')
    release = record.get('release')
    allocation = record.get('allocation_release')
    return dict(profile=record['profile'], phase=state['phase'], armed_tick=record['armed_tick'],
                access_count=len(record['accesses']), idle_step=state['idle_step'],
                request_step=state['request_step'], enabled_tick=state['request_enabled_tick'],
                mode_prelude=deepcopy(record.get('mode_prelude')),
                retirement=(dict(profile=close['profile'], state=close['state'], step=close['step'],
                                 pair_id=close['data']['pair_id'], completed_tick=close['completed_tick'])
                            if close is not None else None),
                release=(dict(profile=release['profile'], state=release['state'], step=release['step'],
                              completed_tick=release['completed_tick']) if release is not None else None),
                allocation_release=(dict(state=allocation['state'], step=allocation['step'],
                    completed_tick=allocation['completed_tick']) if allocation is not None else None),
                due_tick=due(device), route=state['request_route'],
                limits='Numeric output, physical source and focus outcome are not supplied')
