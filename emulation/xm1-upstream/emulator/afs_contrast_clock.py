"""Explicit bounded contrast-AF sensor clocks, with a flat plain ledger.

This input supplies architectural source IRQs only. Stock subscribers own
messages, producer programming, pose sampling and scan decisions. Cadence
is a declared simulation input; physical X70 timing remains unmeasured.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .histogram_protocol import digest


PROFILE = 'declared-afs-contrast-flat-sensor-clock-ledger-unmeasured'
CONTROL_PROFILE = 'declared-afs-contrast-setup-clock-unmeasured'
PIXEL_PROFILE = 'declared-afs-contrast-pixel-pair-unmeasured'
MAX_PAIRS = 251
MAX_RECORDS = MAX_PAIRS + 4
EPOCH_KEYS = ('profile', 'generation', 'ordinal', 'attempt', 'frame', 'cpu',
              'source', 'physical_irq', 'asserted_tick')
SOURCE_FIXED = ('profile', 'generation', 'ordinal', 'attempt', 'frame', 'cpu',
                'source', 'physical_irq', 'ack_selector', 'requested_tick', 'due_tick')
IDENTITY_KEYS = ('profile', 'kind', 'ordinal', 'pair_id', 'frame', 'producer',
                 'requested_tick', 'phase_at_arm', 'control_phase', 'source_count_at_arm',
                 'data_count_at_arm', 'pose_index', 'contract')
RECORD_KEYS = set(IDENTITY_KEYS) | {'source0', 'source1', 'sampled_focus', 'consumed_tick',
                                 'canceled_tick', 'reason', 'input_sha256', 'state_sha256'}
SOURCE_KEYS = set(SOURCE_FIXED) | {'asserted_tick', 'acknowledged_tick', 'eoi_tick', 'write_cursor', 'state'}
CONTROL_PHASES = ('setup-first-source1', 'setup-second-source1', 'setup-source0', 'initialize-source0')
PREVIEW_OWNER_WORDS = ((0x1625fc0, 'client', 2), (0x1625fcc, 'policy', 0),
                       (0x1626068, 'mode', 1), (0x16260bc, 'split', 0),
                       (0x15fcd70, 'start_callback', 0x1245aa4),
                       (0x15fcd74, 'stop_callback', 0x1245bf8))


def _raw(source, address, size):
    value = bytes(source.board.read_memory(address, size, cpu_id=0))
    if len(value) != size:
        raise UnsupportedDevice('Contrast AF clock caller memory is truncated')
    return value


def _u(source, address, size=4):
    return int.from_bytes(_raw(source, address, size), 'little')


def _signed(source, address):
    return int.from_bytes(_raw(source, address, 2), 'little', signed=True)


def _legacy(source):
    if type(source.request) is not dict:
        raise UnsupportedDevice('Contrast AF clocks require the retained last metering source')
    return {key: value for key, value in source.request.items() if key not in ('af_stream', 'preview_return', 'capture_initialization', 'still_initialization')}


def _stream(source):
    return source.request.get('af_stream') if type(source.request) is dict else None


def _identity(record):
    value = {key: record[key] for key in IDENTITY_KEYS}
    for name in ('source0', 'source1'):
        edge = record[name]
        value[name] = None if edge is None else {key: edge[key] for key in SOURCE_FIXED}
    return value


def _seal(record):
    record['state_sha256'] = digest({key: value for key, value in record.items() if key != 'state_sha256'})


def _drained(source, line):
    return all(not bank.pending & (1 << line) and bank.active != line and bank.injected != line
               for bank in source.board.interrupts.banks)


def _route(source, inputs, number):
    line, vector, priority = ((0x27, 0x9df0b8, 10) if number == 0 else (0x29, 0x9df038, 9))
    if inputs._event_route(line) != 0:
        raise UnsupportedDevice('Contrast AF clock lacks its unique CPU0 physical route')
    bank = source.board.interrupts.banks[0]
    if (bank.config.get(0x400 + 4 * line) != vector
            or bank.config.get(0x800 + 4 * line) != priority
            or not bank.source_config & (1 << line) or not _drained(source, line)):
        raise UnsupportedDevice('Contrast AF clock route/vector/priority is not live and drained')


def _preview_owner(source, pixels):
    """The persistent IMGDRV clients observed after the real AF lens setup."""
    counts = (_u(source, 0xc6dc58, 1), _u(source, 0xc6dc98, 1))
    if counts == (2, 2 if pixels else 1):
        if any(_u(source, address) != 0 for address in (0x1625f40, 0x1625f44)):
            raise UnsupportedDevice('Contrast AF preview tokens remain live but their callbacks are absent')
        return None  # The separately qualified isolated AF caller ABI.
    if counts != (3, 3 if pixels else 2) or _u(source, 0x1625f5c) not in (5, 6):
        raise UnsupportedDevice('Contrast AF preview subscribers lack the exact live IMGDRV5/6 owner')
    if any(_u(source, address) != expected for address, _, expected in PREVIEW_OWNER_WORDS):
        raise UnsupportedDevice('Contrast AF persistent preview mode/client/callback owner changed')
    tokens = [_u(source, address) for address in (0x1625f40, 0x1625f44)]
    if not all(tokens) or tokens[0] == tokens[1]:
        raise UnsupportedDevice('Contrast AF persistent preview tokens are absent or aliased')
    return dict(source0_token=tokens[0], source1_token=tokens[1],
                **{name: expected for _, name, expected in PREVIEW_OWNER_WORDS})


def _expected_callbacks(number, pixels, preview):
    value = ((0x15f6e5c, 0x11f6008, 2), (0x1622440, 0x12fba84, 1)) if number == 0 else (
        ((0x15d5368, 0x1338a88, 2), (0x1622444, 0x12fc0e4, 1)) if pixels else
        ((0x1622444, 0x12fc0e4, 1),))
    if preview:
        value += ((0x1625f40 if number == 0 else 0x1625f44,
                   0x13267b4 if number == 0 else 0x1326768, 1),)
    return value


def _callbacks(source, number, pixels, preview=False):
    base = 0xc6dc58 + number * 0x40
    expected = _expected_callbacks(number, pixels, preview)
    count = _u(source, base, 1)
    tokens = [_u(source, base + 4 + 4 * index) for index in range(count)] if count <= 5 else []
    if (count != len(expected) or len(set(tokens)) != count or any(token == 0 for token in tokens)
            or any(tokens[index] != _u(source, address)
                   or (_u(source, base + 24 + 8 * index), _u(source, base + 28 + 8 * index)) != (callback, priority)
                   for index, (address, callback, priority) in enumerate(expected))
            or pixels and number == 1 and _u(source, 0x15d536c) != 1):
        raise UnsupportedDevice('Contrast AF clock subscribers differ from the qualified stock owner')
    return dict(source=number, tokens=tokens, callbacks=[[callback, priority] for _, callback, priority in expected])


def _phase(source):
    state = _u(source, 0x16223f8, 1)
    if _u(source, 0x1622490) != 1:
        raise UnsupportedDevice('Contrast AF clocks currently qualify only the stock kind1 scan')
    if state == 2:
        raise UnsupportedDevice('Stock AF state2 is still waiting for lens/mode setup')
    if state == 5:
        flags = tuple(_u(source, address) for address in (0x1622428, 0x162242c, 0x1622430, 0x1622434))
        if any(value not in (0, 1) for value in flags):
            raise UnsupportedDevice('Contrast AF setup flags are outside their stock Boolean domain')
        if flags[0] == 0: return 'control', CONTROL_PHASES[0], (1,), state
        if flags[1] == 0: return 'control', CONTROL_PHASES[1], (1,), state
        if flags[2] == 0: return 'control', CONTROL_PHASES[2], (0,), state
        raise UnsupportedDevice('Stock AF setup is waiting for its own completion callback')
    if state == 11: return 'control', CONTROL_PHASES[3], (0,), state
    if state in (12, 13): return 'pixel', None, (0, 1), state
    raise UnsupportedDevice('Contrast AF clocks have no qualified consumer in the current state')


def _producer(source, expected=None):
    device = source._device('ProducerControl')
    getter = getattr(device, 'afs_focus_source_anchor', None)
    if getter is None:
        raise UnsupportedDevice('Contrast AF pixel clock lacks its real producer owner')
    value = getter(expected=deepcopy(expected)) if expected is not None else getter()
    if (type(value) is not dict or set(value) != {'enabled_tick', 'sha256'}
            or type(value['enabled_tick']) is not int or not 0 <= value['enabled_tick'] <= source.board.virtual_ticks
            or type(value['sha256']) is not str or len(value['sha256']) != 64
            or expected is not None and value != expected):
        raise UnsupportedDevice('Contrast AF producer activation anchor changed')
    return deepcopy(value)


def _live(source, kind, numbers):
    inputs = source._prefix(historical=True)
    # This exact signature is observed at the real AF request. A subsequent
    # retune/generation requires its own actual signature, never a wildcard.
    if (inputs.event_enable_generation != 6 or inputs.registers.get(0x38c) != 0x10003
            or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1
            or inputs.registers.get(0x70) != 0 or inputs.busy is not False
            or inputs.phase_busy is not False or inputs.pending_command is not None
            or inputs.command_armed is not None):
        raise UnsupportedDevice('Contrast AF clock input gates differ from generation6/mask10003')
    if (_u(source, 0x15e32d0) != 7 or _u(source, 0x173d308) != 0x53
            or _u(source, 0x15f6a60) != 0):
        raise UnsupportedDevice('Contrast AF clock belongs to a different acquisition/mode owner')
    pixels = kind == 'pixel'
    if (_u(source, 0x15f68a4), _u(source, 0x15f68a8), _u(source, 0x15f6bf4)) != (
            (1, 0x12fc2cc, 1) if pixels else (0, 0, 0)):
        raise UnsupportedDevice('Contrast AF clock producer/client activation differs from its phase')
    callbacks = []
    preview = _preview_owner(source, pixels)
    for number in (0, 1):
        # Both stock subscribers belong to this AF consumer even when one
        # explicit control step offers only a single physical clock.
        _callbacks(source, number, pixels, preview is not None)
    for number in numbers:
        _route(source, inputs, number)
        callbacks.append(_callbacks(source, number, pixels, preview is not None))
    contract = dict(generation=6, mask=0x10003, sensor_mode=0x53,
                    prefix_sha256=digest(inputs.event_history), callbacks=callbacks)
    if preview is not None: contract['preview_owner'] = preview
    return inputs, contract


def _closed_previous(source, records):
    if not records: return
    previous = records[-1]
    if previous['consumed_tick'] is None:
        raise UnsupportedDevice('Contrast AF previous clock has not reached its stock consumer')
    if previous['kind'] != 'pixel': return
    getter = getattr(source._device('ProducerControl'), 'afs_focus_data_anchor', None)
    if getter is None:
        raise UnsupportedDevice('Next contrast AF pair requires the actual closed data owner')
    closure = getter(previous['pair_id'])
    if (type(closure) is not dict or closure.get('state') != 'acknowledged'
            or closure.get('pair_id') != previous['pair_id']
            or closure.get('input_sha256') != previous['input_sha256']
            or closure.get('producer') != previous['producer']
            or type(closure.get('acknowledged_tick')) is not int
            or not previous['consumed_tick'] <= closure['acknowledged_tick'] <= source.board.virtual_ticks
            or not _drained(source, 46)):
        raise UnsupportedDevice('Next contrast AF pair lacks its exact paired data ACK/EOI')


def admission(source):
    """Copied pure readiness; never mutates an owner after an early Feed."""
    result = dict(ready=False, kind=None, phase=None, source=None, reason=None)
    try:
        if source.board.failure is not None or source.profile is None:
            raise UnsupportedDevice('Restore/select the declared source before offering AF clocks')
        source._validate()
        if _legacy(source).get('frame') != 9 or _legacy(source).get('state') != 'acknowledged':
            raise UnsupportedDevice('Contrast AF clocks require the retained completed metering Source9')
        stream = validate(source)
        records = stream['records'] if stream is not None else []
        _closed_previous(source, records)
        result['phase'] = _u(source, 0x16223f8, 1)
        kind, phase, numbers, state = _phase(source)
        result.update(kind=kind, phase=state, source=list(numbers), control_phase=phase)
        _live(source, kind, numbers)
        if kind == 'pixel':
            _producer(source)
            count, data = _u(source, 0x16223fc, 2), _u(source, 0x16223fe, 2)
            pair_count = sum(record['kind'] == 'pixel' for record in records)
            if (count != data or count != pair_count or pair_count >= MAX_PAIRS
                    or state == 12 and pair_count != 0 or state == 13 and pair_count == 0
                    or any(_u(source, address, 2) != 0 for address in (0x1622400, 0x1622402, 0x16224a6))
                    or state == 13 and not count < min(_u(source, 0x1622404, 2), _u(source, 0x16224a4, 2))):
                raise UnsupportedDevice('Contrast AF next pair awaits stock source/data/zero-skip progression')
            # DataCount advances before the queued 3B55 arithmetic request.
            # A second request while child28 is in state5 takes its overrun
            # branch. 121D394 uses a halfword state; 12C0CE8/CF0 advances the
            # kind1 calculator's byte count before its real 3BA0 completion.
            if state == 13 and (_u(source, 0x15f8828, 2) != 4
                                or _u(source, 0x17a8eac, 1) != pair_count):
                raise UnsupportedDevice('Contrast AF next pair awaits stock calculator consumption')
        elif any(record.get('control_phase') == phase for record in records):
            raise UnsupportedDevice('Contrast AF setup phase has already received its one explicit clock')
        if len(records) >= MAX_RECORDS:
            raise UnsupportedDevice('Contrast AF flat ledger reached its stock bounded scan limit')
        result.update(ready=True)
    except UnsupportedDevice as error:
        result['reason'] = str(error)
    return result


def arm(source, profile, *, cadence_ticks=1000, exposure_ticks=1000):
    query = admission(source)
    expected = CONTROL_PROFILE if query['kind'] == 'control' else PIXEL_PROFILE
    if not query['ready'] or profile != expected:
        raise UnsupportedDevice(query['reason'] or 'Contrast AF explicit profile differs from the ready phase')
    if (type(cadence_ticks) is not int or not 1 <= cadence_ticks <= 400000
            or type(exposure_ticks) is not int or not 1 <= exposure_ticks <= 400000
            or source.board.virtual_ticks + cadence_ticks + exposure_ticks >= 1 << 64):
        raise UnsupportedDevice('Contrast AF cadence/exposure must be bounded declared positive ticks')
    kind, phase, numbers, state = _phase(source)
    inputs, contract = _live(source, kind, numbers)
    stream = _stream(source)
    records = stream['records'] if stream is not None else []
    ordinal = len(records) + 1
    pair_id = sum(record['kind'] == 'pixel' for record in records) + 1 if kind == 'pixel' else None
    now, frame = source.board.virtual_ticks, 9 + ordinal
    record = dict(profile=profile, kind=kind, ordinal=ordinal, pair_id=pair_id, frame=frame,
        producer=_producer(source) if kind == 'pixel' else None, requested_tick=now,
        phase_at_arm=state, control_phase=phase, source_count_at_arm=_u(source, 0x16223fc, 2),
        data_count_at_arm=_u(source, 0x16223fe, 2),
        pose_index=_u(source, 0x16223fe, 2) if kind == 'pixel' else None,
        contract=contract, source0=None, source1=None, sampled_focus=None, consumed_tick=None,
        canceled_tick=None, reason=None)
    for number in numbers:
        due = now + cadence_ticks + (exposure_ticks if kind == 'pixel' and number == 1 else 0)
        record['source' + str(number)] = dict(profile=profile, generation=contract['generation'],
            ordinal=ordinal, attempt=ordinal, frame=frame, cpu=0, source=number,
            physical_irq=hex(0x27 if number == 0 else 0x29), ack_selector=1 - number,
            requested_tick=now, due_tick=due, asserted_tick=None, acknowledged_tick=None,
            eoi_tick=None, write_cursor=len(inputs.writes), state='waiting')
    record['input_sha256'] = digest(_identity(record)); _seal(record)
    if stream is None:
        stream = dict(profile=PROFILE, legacy_source_sha256=digest(_legacy(source)), records=[])
        source.request['af_stream'] = stream
    stream['records'].append(record)
    source.board.record('Contrast AF clocks explicitly armed', profile=profile, kind=kind,
                        ordinal=ordinal, pair_id=pair_id, frame=frame,
                        cadence_ticks=cadence_ticks, exposure_ticks=exposure_ticks,
                        limits='Declared physical source clock only; no pixels, pose writes, firmware event or AF result')
    return deepcopy(record)


def validate(source):
    stream = _stream(source)
    if stream is None: return None
    if (type(stream) is not dict or set(stream) != {'profile', 'legacy_source_sha256', 'records'}
            or stream['profile'] != PROFILE or stream['legacy_source_sha256'] != digest(_legacy(source))
            or type(stream['records']) is not list or not 1 <= len(stream['records']) <= MAX_RECORDS):
        raise UnsupportedDevice('Contrast AF flat ledger or immutable metering prefix changed')
    previous, pairs, anchor, control_index = -1, 0, None, -1
    inputs = source._device('InputProcessingControl')
    for ordinal, record in enumerate(stream['records'], 1):
        if (type(record) is not dict or set(record) != RECORD_KEYS):
            raise UnsupportedDevice('Contrast AF flat record has an unknown plain schema')
        if (record.get('state_sha256') != digest(
                    {key: value for key, value in record.items() if key != 'state_sha256'})
                or record.get('input_sha256') != digest(_identity(record))
                or record.get('ordinal') != ordinal or record.get('frame') != 9 + ordinal
                or type(record.get('requested_tick')) is not int
                or not previous <= record['requested_tick'] <= source.board.virtual_ticks
                or record.get('kind') not in ('control', 'pixel')):
            raise UnsupportedDevice('Contrast AF flat record identity, seal or chronology changed')
        pixels = record['kind'] == 'pixel'
        if pixels:
            pairs += 1
            if (record['pair_id'] != pairs or record['pose_index'] != pairs - 1
                    or (record['source_count_at_arm'], record['data_count_at_arm']) != (pairs - 1, pairs - 1)
                    or record['phase_at_arm'] != (12 if pairs == 1 else 13) or record['control_phase'] is not None
                    or record['source0'] is None or record['source1'] is None):
                raise UnsupportedDevice('Contrast AF pair count/pose index/state changed')
            if anchor is None: anchor = _producer(source, expected=record['producer'])
            if record['producer'] != anchor:
                raise UnsupportedDevice('Contrast AF pairs changed producer activation')
        elif (record['pair_id'] is not None or record['producer'] is not None
              or record['control_phase'] not in CONTROL_PHASES or pairs != 0
              or record['pose_index'] is not None or record['sampled_focus'] is not None
              or (record['source_count_at_arm'], record['data_count_at_arm']) != (0, 0)):
            raise UnsupportedDevice('Contrast AF control record claims a numeric pair')
        else:
            index = CONTROL_PHASES.index(record['control_phase'])
            number = 1 if index < 2 else 0
            if (index <= control_index or record['phase_at_arm'] != (11 if index == 3 else 5)
                    or record['source' + str(number)] is None or record['source' + str(1 - number)] is not None):
                raise UnsupportedDevice('Contrast AF setup control order/physical source changed')
            control_index = index
        if record['profile'] != (PIXEL_PROFILE if pixels else CONTROL_PROFILE):
            raise UnsupportedDevice('Contrast AF record profile changed')
        contract = record['contract']
        if (type(contract) is not dict or set(contract) not in (
                    {'generation', 'mask', 'sensor_mode', 'prefix_sha256', 'callbacks'},
                    {'generation', 'mask', 'sensor_mode', 'prefix_sha256', 'callbacks', 'preview_owner'})
                or (contract['generation'], contract['mask'], contract['sensor_mode']) != (6, 0x10003, 0x53)
                or contract['prefix_sha256'] != digest(inputs.event_history)
                or type(contract['callbacks']) is not list or len(contract['callbacks']) != (2 if pixels else 1)):
            raise UnsupportedDevice('Contrast AF request lost its exact input generation and immutable prefix')
        preview = contract.get('preview_owner')
        if preview is not None:
            fixed = {name: expected for _, name, expected in PREVIEW_OWNER_WORDS}
            if (type(preview) is not dict or set(preview) != set(fixed) | {'source0_token', 'source1_token'}
                    or any(preview[key] != value for key, value in fixed.items())
                    or any(type(preview[name]) is not int or not 0 < preview[name] < 1 << 32
                           for name in ('source0_token', 'source1_token'))
                    or preview['source0_token'] == preview['source1_token']):
                raise UnsupportedDevice('Contrast AF retained persistent preview owner changed')
        for number in (0, 1):
            edge = record['source' + str(number)]
            if edge is None: continue
            if type(edge) is not dict or set(edge) != SOURCE_KEYS:
                raise UnsupportedDevice('Contrast AF physical source has an unknown plain schema')
            callback = [item for item in contract['callbacks'] if type(item) is dict and item.get('source') == number]
            expected_callbacks = [[callback, priority] for _, callback, priority in
                                  _expected_callbacks(number, pixels, preview is not None)]
            if (len(callback) != 1 or set(callback[0]) != {'source', 'tokens', 'callbacks'}
                    or callback[0]['callbacks'] != expected_callbacks
                    or type(callback[0]['tokens']) is not list or len(callback[0]['tokens']) != len(expected_callbacks)
                    or len(set(callback[0]['tokens'])) != len(expected_callbacks)
                    or any(type(token) is not int or not 0 < token < 1 << 32 for token in callback[0]['tokens'])):
                raise UnsupportedDevice('Contrast AF retained physical subscriber contract changed')
            if preview is not None and callback[0]['tokens'][-1] != preview['source' + str(number) + '_token']:
                raise UnsupportedDevice('Contrast AF retained preview callback lost its exact owner token')
            if (edge['state'] not in ('waiting', 'asserted', 'acknowledged', 'canceled')
                    or tuple(edge[key] for key in ('profile', 'generation', 'ordinal', 'attempt', 'frame', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
                       (record['profile'], record['contract']['generation'], ordinal, ordinal, record['frame'], 0, number,
                        hex(0x27 if number == 0 else 0x29), 1 - number)
                    or any(type(edge[key]) is not int for key in SOURCE_FIXED if key not in ('profile', 'physical_irq'))
                    or not record['requested_tick'] == edge['requested_tick'] < edge['due_tick'] <= record['requested_tick'] + 800000
                    or type(edge['write_cursor']) is not int or not 0 <= edge['write_cursor'] <= len(inputs.writes)):
                raise UnsupportedDevice('Contrast AF physical source identity/timer changed')
            for name, start in (('asserted_tick', edge['due_tick']), ('acknowledged_tick', edge['asserted_tick']), ('eoi_tick', edge['acknowledged_tick'])):
                value = edge[name]
                if value is not None and (type(value) is not int or start is None or not start <= value <= source.board.virtual_ticks):
                    raise UnsupportedDevice('Contrast AF physical source chronology changed')
            if ((edge['state'] in ('asserted', 'acknowledged')) != (edge['asserted_tick'] is not None)
                    or (edge['state'] == 'acknowledged') != (edge['acknowledged_tick'] is not None)):
                raise UnsupportedDevice('Contrast AF source state differs from its actual assertion/ACK')
        if record['consumed_tick'] is not None:
            if (type(record['consumed_tick']) is not int or not record['requested_tick'] <= record['consumed_tick'] <= source.board.virtual_ticks
                    or any(edge is not None and edge['eoi_tick'] is None for edge in (record['source0'], record['source1']))
                    or pixels and (type(record['sampled_focus']) is not int or not -32768 <= record['sampled_focus'] <= 32767)):
                raise UnsupportedDevice('Contrast AF record claims an unobserved stock completion')
        if record['canceled_tick'] is not None:
            if (type(record['canceled_tick']) is not int or not record['requested_tick'] <= record['canceled_tick'] <= source.board.virtual_ticks
                    or record['consumed_tick'] is not None or not record['reason']):
                raise UnsupportedDevice('Contrast AF request claims an unknown cancellation')
        elif record['reason'] is not None or any(edge is not None and edge['state'] == 'canceled' for edge in (record['source0'], record['source1'])):
            raise UnsupportedDevice('Contrast AF source claims an unrecorded cancellation')
        if ordinal < len(stream['records']) and record['consumed_tick'] is None:
            raise UnsupportedDevice('Contrast AF flat ledger has overlapping unconsumed requests')
        previous = record['consumed_tick'] if record['consumed_tick'] is not None else record['requested_tick']
    return stream


def pair_projection(record):
    if record['kind'] != 'pixel' or record['consumed_tick'] is None:
        raise UnsupportedDevice('Contrast AF pair has not reached its stock pose consumer')
    return dict(profile=record['profile'], pair_id=record['pair_id'], frame=record['frame'],
        producer=deepcopy(record['producer']), input_sha256=record['input_sha256'],
        source0={key: record['source0'][key] for key in EPOCH_KEYS},
        source1={key: record['source1'][key] for key in EPOCH_KEYS},
        sampled_focus=record['sampled_focus'], pose_index=record['pose_index'], consumed_tick=record['consumed_tick'])


def _consumed(source, record):
    if record['kind'] == 'pixel':
        count = record['source_count_at_arm'] + 1
        pose = _signed(source, 0x162254c + 2 * record['pose_index'])
        if (_u(source, 0x16223fc, 2) != count or pose == 0x7fff
                or pose != record['sampled_focus']): return False
        return True
    phase = record['control_phase']
    if phase == CONTROL_PHASES[3]:
        if _u(source, 0x16223f8, 1) != 12 or _u(source, 0x15f68a8) != 0x12fc2cc: return False
        _producer(source)
        return True
    address = {CONTROL_PHASES[0]: 0x1622428, CONTROL_PHASES[1]: 0x162242c, CONTROL_PHASES[2]: 0x1622430}[phase]
    return _u(source, address) == 1


def tick(source, board):
    stream = _stream(source)
    if stream is None or type(stream) is dict and stream.get('records') and (
            stream['records'][-1].get('consumed_tick') is not None or stream['records'][-1].get('canceled_tick') is not None):
        return
    if board is not source.board:
        raise UnsupportedDevice('Contrast AF source tick belongs to a different board')
    if board.failure is not None:
        return
    try:
        if type(stream) is not dict or type(stream.get('records')) is not list or not stream['records']:
            raise UnsupportedDevice('Contrast AF active stream has no known flat request')
        record = stream['records'][-1]
        inputs = source._device('InputProcessingControl')
        for number in (0, 1):
            edge = record['source' + str(number)]
            if edge is None: continue
            if edge['state'] == 'waiting':
                if record['kind'] == 'pixel' and number == 1 and record['source0']['eoi_tick'] is None:
                    continue
                if (inputs.event_enable_generation != record['contract']['generation']
                        or inputs.registers.get(0x38c) != record['contract']['mask']
                        or inputs.registers.get(0x48) != 1 or inputs.registers.get(0x4c) != 1):
                    validate(source)
                    record.update(canceled_tick=board.virtual_ticks, reason='Input owner/gates withdrew before the declared edge')
                    for remaining in (record['source0'], record['source1']):
                        if remaining is not None and remaining['state'] == 'waiting': remaining['state'] = 'canceled'
                    _seal(record)
                    board.record('Contrast AF clock request canceled', ordinal=record['ordinal'], reason=record['reason'])
                    return
                if board.virtual_ticks < edge['due_tick']: continue
                validate(source)
                _, live = _live(source, record['kind'], (number,))
                if (live['generation'] != record['contract']['generation']
                        or live.get('preview_owner') != record['contract'].get('preview_owner')
                        or live['callbacks'] != [item for item in record['contract']['callbacks'] if item['source'] == number]):
                    raise UnsupportedDevice('Contrast AF clock caller changed before its due assertion')
                edge.update(state='asserted', asserted_tick=board.virtual_ticks, write_cursor=len(inputs.writes))
                _seal(record)
                board.queue_irq(0, int(edge['physical_irq'], 16))
                board.record('Contrast AF physical sensor clock asserted', ordinal=record['ordinal'], pair_id=record['pair_id'], source=number,
                             physical_irq=edge['physical_irq'], tick=board.virtual_ticks,
                             limits='Architectural input only; stock ISR owns event, ACK and pose')
            elif edge['state'] == 'asserted':
                hits = [item for item in inputs.writes[edge['write_cursor']:] if item['address'] == hex(inputs.BASE + 0x388)
                        and item['value'] == hex(edge['ack_selector'])]
                if not hits: continue
                validate(source)
                if any(item['cpu'] != 0 for item in hits):
                    raise UnsupportedDevice('Contrast AF source ACK came from a different CPU')
                edge.update(state='acknowledged', acknowledged_tick=board.virtual_ticks, write_cursor=len(inputs.writes))
                _seal(record)
            if edge['state'] == 'acknowledged' and edge['eoi_tick'] is None and _drained(source, int(edge['physical_irq'], 16)):
                validate(source)
                edge['eoi_tick'] = board.virtual_ticks
                if number == 1 and record['kind'] == 'pixel': record['sampled_focus'] = _signed(source, 0x1622406)
                _seal(record)
        if (all(edge is None or edge['eoi_tick'] is not None for edge in (record['source0'], record['source1']))
                and _consumed(source, record)):
            validate(source)
            record['consumed_tick'] = board.virtual_ticks; _seal(record)
            board.record('Contrast AF clock reached its stock consumer', ordinal=record['ordinal'], pair_id=record['pair_id'],
                         sampled_focus=record['sampled_focus'], pose_index=record['pose_index'], tick=board.virtual_ticks)
    except UnsupportedDevice as error:
        board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='contrast-af-clock')


def due(source):
    stream = _stream(source)
    if stream is None or not stream['records']: return None
    record = stream['records'][-1]
    if record['consumed_tick'] is not None or record['canceled_tick'] is not None: return None
    values = [edge['due_tick'] for number, edge in enumerate((record['source0'], record['source1']))
              if edge is not None and edge['state'] == 'waiting'
              and not (number == 1 and record['kind'] == 'pixel' and record['source0']['eoi_tick'] is None)]
    return min(values) if values else None


def summary(source):
    """Compact validated host projection, without dumping scan/data history."""
    source._validate()
    stream = validate(source)
    records = stream['records'] if stream is not None else []
    last = records[-1] if records else None
    pairs = [record for record in records if record['kind'] == 'pixel' and record['consumed_tick'] is not None]
    return dict(profile=stream['profile'] if stream is not None else None,
                phase=_u(source, 0x16223f8, 1), records=len(records),
                pending=last['ordinal'] if last is not None and last['consumed_tick'] is None and last['canceled_tick'] is None else None,
                next_due_tick=due(source),
                last_pair_pose=dict(pair_id=pairs[-1]['pair_id'], sampled_focus=pairs[-1]['sampled_focus'],
                                    pose_index=pairs[-1]['pose_index']) if pairs else None)
