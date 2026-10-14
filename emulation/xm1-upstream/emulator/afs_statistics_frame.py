"""One image-derived consumer-statistics lifetime for an explicit AF-S edge.

The parent owns MMIO. This helper latches the current destination tuple, reads
the last completed modeled YYCbCr scene, publishes declared numeric windows
before IRQ46, and accepts the real92C8CC paired ACK transaction. First-job
history remains unchanged. No metadata, callbacks or AF decisions are set.
"""
from copy import deepcopy
import hashlib

from .hardware import UnsupportedDevice
from . import histogram_protocol as hp
from .afs_scene_fixture import CONSUMER_PROFILE as PROFILE, consumer_payloads
from .virtual_source import ram_span


ACK_OFFSETS = (0x88c, 0x8f4, 0x98c, 0x9f4)
EVENT_LIMIT = 4096
CONTINUATION_PROFILE = 'declared-frozen-yycbcr-second-consumer-statistics-unmeasured'


def _state_hash(data):
    return hp.digest({key:value for key,value in data.items() if key != 'state_sha256'})


def _seal(data):
    data['state_sha256'] = _state_hash(data)


def _anchor(record):
    fields = ('descriptor_step', 'setup_step', 'enable_step', 'descriptor_accesses',
              'setup_accesses', 'enable_accesses', 'started_tick', 'armed_tick',
              'enabled_tick', 'route', 'original_history_sha256')
    return hp.digest({key: record[key] for key in fields})


def _source(device, input_sha256=None):
    from . import afs_frame_source
    markers = afs_frame_source.requests(device.board)
    if type(markers) is not list or not 1 <= len(markers) <= 5:
        raise UnsupportedDevice('AF-S statistics require one bounded source history')
    matches = [marker for marker in markers if input_sha256 is None or _input_hash(marker) == input_sha256]
    if input_sha256 is None:
        matches = matches[-1:]
    if len(matches) != 1:
        raise UnsupportedDevice('AF-S statistics lost their immutable source identity')
    marker = matches[0]
    if marker is None or marker['state'] == 'canceled':
        raise UnsupportedDevice('AF-S statistics require an explicitly armed source edge')
    return marker


def _epoch(board, frame):
    from . import afs_frame_source
    observed = afs_frame_source.epoch_for(board,frame)
    return observed if observed is None or observed['frame'] == frame else None


def _input_hash(marker):
    fields = ('profile', 'generation', 'ordinal', 'attempt', 'frame', 'cpu', 'source',
              'physical_irq', 'ack_selector', 'requested_tick', 'due_tick',
              'delay_ticks', 'prefix_sha256', 'producer')
    return hp.digest({key: marker[key] for key in fields})


def _route(device, *, pending=False):
    banks = device.board.interrupts.banks
    if len(banks) != 2 or [i for i, bank in enumerate(banks) if bank.enabled & (1 << 46)] != [0]:
        raise UnsupportedDevice('AF-S statistics require the unique IRQ46 owner')
    bank = banks[0]
    if (bank.config.get(0x400 + 46 * 4) != 0x92c8cc
            or bank.config.get(0x800 + 46 * 4) != 13 or not bank.source_config & (1 << 46)
            or not pending and (bank.pending & (1 << 46) or bank.active == 46 or bank.injected == 46)):
        raise UnsupportedDevice('AF-S statistics differ from the stock IRQ46 route')


def _scene(device):
    matches = [item for item in device.board.devices if type(item).__name__ == 'ImageWorkControl']
    if len(matches) != 1 or matches[0].board is not device.board:
        raise UnsupportedDevice('AF-S statistics require one modeled image source')
    # A closed image owns no idle timer. Audit its real retained source/DMA
    # admission when this consumer requests the scene, before reading bytes.
    # Tiny explicitly isolated image fixtures intentionally have no protocol.
    admission_validator=getattr(matches[0],'_validate_afs_data',None)
    if admission_validator is not None:admission_validator()
    history = matches[0].data_history
    if not history or history[-1].get('state') != 'acknowledged':
        raise UnsupportedDevice('AF-S statistics require a completed YYCbCr frame')
    item = history[-1]['outputs'][0]
    address, size = int(item['address'], 16), item['bytes']
    ram_span(address, size)
    if type(size) is not int or not 1 <= size <= 4 * 1024 * 1024:
        raise UnsupportedDevice('AF-S statistics source exceeds its byte bound')
    raw = bytes(device.board.read_memory(address, size))
    if hashlib.sha256(raw).hexdigest() != item['payload_sha256']:
        raise UnsupportedDevice('AF-S statistics source RAM differs from completed image data')
    payload = consumer_payloads(raw, width=item['width'], height=item['height'], stride_bytes=item['stride_bytes'])
    scene = {key: item[key] for key in ('address', 'bytes', 'width', 'height', 'stride_bytes', 'payload_sha256')}
    scene['epoch'] = deepcopy(history[-1]['epoch'])
    return scene, payload


def _arm(device, profile, lifetime):
    if (profile != (PROFILE if lifetime == 1 else CONTINUATION_PROFILE)
            or type(device.rearm) is not dict):
        raise UnsupportedDevice('Unknown or repeated AF-S statistics experiment')
    device._validate_rearm()
    record, marker = device.rearm, _source(device)
    prior = validate(device)
    archived = record.get('afs_statistics_history',[])
    if lifetime == 1:
        if prior is not None or archived or marker['frame'] != 5:
            raise UnsupportedDevice('AF-S first statistics require the first explicit source edge')
    elif (prior is None or prior['lifetime'] != 1 or prior['state'] != 'acknowledged'
            or archived or marker['frame'] != 6
            or record.get('rotation_count') != 5
            or not record.get('rotations') or record['rotations'][-1].get('state') != 'completed'):
        raise UnsupportedDevice('AF-S second statistics require first paired ACK and its completed rotation')
    now = device.board.virtual_ticks
    if (record['state'] != 'enabled-awaiting-fresh-input' or marker['state'] != 'waiting'
            or not record['enabled_tick'] <= now < marker['due_tick']
            or any(type(v) is not int or not 1 <= v < 1 << 64 for v in (device.data_ticks, device.ack_ticks))
            or marker['due_tick'] + device.data_ticks + 2 * device.ack_ticks >= 1 << 64):
        raise UnsupportedDevice('AF-S statistics require an enabled request and future source edge')
    _route(device)
    scene, payload = _scene(device)
    plan, ranges = [], []
    for register, offset, raw in payload['spans']:
        address = device.registers[register] + offset
        begin, end = ram_span(address, len(raw))
        if address & 3 or any(begin < high and low < end for low, high in ranges):
            raise UnsupportedDevice('AF-S statistics latched windows are unaligned or overlapping')
        ranges.append((begin, end))
        plan.append(dict(register=register, offset=offset, address=address, bytes=len(raw), payload_hex=raw.hex()))
    prior_ack = {device.BASE + offset: device.registers[device.BASE + offset] for offset in ACK_OFFSETS}
    if any(type(value) is not int or value not in (0, 1) for value in prior_ack.values()):
        raise UnsupportedDevice('AF-S statistics ACK latches have unsupported values')
    latch = dict(anchor=_anchor(record), input_sha256=_input_hash(marker), scene=scene, plan=plan,
                 geometry={hex(address):device.registers[address] for address in device.REARM_GEOMETRY_ORDER},
                 configuration_sha256=record['configuration_sha256'],
                 retained_configuration_sha256=record['retained_configuration_sha256'],
                 assumptions=payload['assumptions'], data_ticks=device.data_ticks, ack_ticks=device.ack_ticks,
                 source_frame=marker['frame'], prior_statistics_sha256=hp.digest(prior) if prior is not None else None)
    data = dict(profile=profile, lifetime=lifetime, state='waiting-source', armed_tick=now, latch=latch,
                latch_sha256=hp.digest(latch), source_epoch=None, due_tick=None, completed_tick=None,
                ack_channel=0, ack_step=0, ack_due_tick=None, acknowledged_tick=None,
                prior_ack_words=prior_ack, ack_words=dict(prior_ack), events=[])
    _seal(data)
    if prior is not None:
        record['afs_statistics_history'] = [deepcopy(prior)]
    record['afs_statistics'] = data
    device.board.record('AF-S image-derived consumer statistics armed', profile=profile, scene=scene,
                        consumer_bytes=sum(span['bytes'] for span in plan), spans=len(plan),
                        limits=payload['assumptions'])
    return deepcopy(data)


def arm(device, profile=PROFILE):
    return _arm(device,profile,1)


def arm_next(device, profile=CONTINUATION_PROFILE):
    return _arm(device,profile,2)


def _validate_data(device, record, data, *, check_route=False):
    if type(data) is not dict:
        raise UnsupportedDevice('AF-S statistics lifetime is not plain state')
    marker = _source(device,data['latch']['input_sha256'])
    if (data.get('lifetime') not in (1,2)
            or data.get('profile') != (PROFILE if data['lifetime'] == 1 else CONTINUATION_PROFILE)
            or data.get('state_sha256') != _state_hash(data)
            or data.get('state') not in ('waiting-source', 'pending', 'delivered', 'acknowledged')
            or hp.digest(data['latch']) != data.get('latch_sha256')
            or data['latch']['anchor'] != _anchor(record)
            or data['latch']['input_sha256'] != _input_hash(marker)
            or data['latch']['source_frame'] != marker['frame']
            or marker['frame'] != 4 + data['lifetime']
            or data['latch']['data_ticks'] != device.data_ticks or data['latch']['ack_ticks'] != device.ack_ticks
            or type(data.get('armed_tick')) is not int
            or not 0 <= data['armed_tick'] <= device.board.virtual_ticks
            or not data['armed_tick'] < marker['due_tick']
            or type(data.get('events')) is not list or len(data['events']) > EVENT_LIMIT
            or type(data.get('ack_channel')) is not int or not 0 <= data['ack_channel'] <= 2
            or type(data.get('ack_step')) is not int or not 0 <= data['ack_step'] <= 3
            or (data['state'] == 'acknowledged') != (data['ack_channel'] == 2)):
        raise UnsupportedDevice('AF-S statistics plain request identity or state changed')
    if data['state'] in ('waiting-source', 'pending'):
        if (data['completed_tick'] is not None or data['ack_channel'] != 0 or data['ack_step'] != 0
                or data['ack_due_tick'] is not None or data['acknowledged_tick'] is not None
                or data['events'] or data['ack_words'] != data['prior_ack_words']):
            raise UnsupportedDevice('AF-S statistics claim acknowledgment before delivery')
    if data['state'] == 'waiting-source' and (data['source_epoch'] is not None or data['due_tick'] is not None):
        raise UnsupportedDevice('AF-S statistics claim a source before its real edge')
    if data['state'] in ('pending', 'delivered', 'acknowledged'):
        if (data['source_epoch'] != _epoch(device.board,marker['frame'])
                or data['due_tick'] != data['source_epoch']['asserted_tick'] + device.data_ticks):
            raise UnsupportedDevice('AF-S statistics claim a changed or unobserved source epoch')
    if data['state'] in ('delivered', 'acknowledged'):
        if (type(data['completed_tick']) is not int
                or not data['due_tick'] <= data['completed_tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('AF-S statistics delivery precedes the real source edge')
    if (set(data['ack_words']) != {device.BASE + offset for offset in ACK_OFFSETS}
            or set(data['prior_ack_words']) != set(data['ack_words'])
            or any(type(word) is not int or word not in (0,1) for word in data['ack_words'].values())
            or any(type(word) is not int or word not in (0,1) for word in data['prior_ack_words'].values())):
        raise UnsupportedDevice('AF-S statistics ACK projection domain changed')
    if data['ack_due_tick'] is not None:
        if (data['state'] != 'delivered' or data['ack_channel'] >= 2 or data['ack_step'] not in (2,3)
                or type(data['ack_due_tick']) is not int or not 0 <= data['ack_due_tick'] < 1 << 64
                or data['ack_words'][device.BASE + 0x8f4 + data['ack_channel']*0x100] != 1):
            raise UnsupportedDevice('AF-S statistics ACK timer lacks its actual request')
    if data['state'] == 'acknowledged':
        if (data['ack_step'] != 0 or data['ack_due_tick'] is not None
                or type(data['acknowledged_tick']) is not int
                or not data['completed_tick'] <= data['acknowledged_tick'] <= device.board.virtual_ticks):
            raise UnsupportedDevice('AF-S statistics final acknowledgment is inconsistent')
    elif data['acknowledged_tick'] is not None:
        raise UnsupportedDevice('AF-S statistics claim an unobserved final acknowledgment')
    if check_route:
        _route(device, pending=data['state'] in ('delivered', 'acknowledged'))
    return data


def validate(device, *, check_route=False):
    record = device.rearm
    data = record.get('afs_statistics') if type(record) is dict else None
    history = record.get('afs_statistics_history',[]) if type(record) is dict else []
    if type(history) is not list or len(history) > 1 or data is None and history:
        raise UnsupportedDevice('AF-S statistics exceed the exact two-frame experiment')
    if data is None:return None
    for previous in history:
        _validate_data(device,record,previous)
        if previous['lifetime'] != 1 or previous['state'] != 'acknowledged':
            raise UnsupportedDevice('AF-S statistics history contains an incomplete first frame')
    current = _validate_data(device,record,data,check_route=check_route)
    if (len(history) != current['lifetime'] - 1
            or current['latch']['prior_statistics_sha256'] != (hp.digest(history[0]) if history else None)):
        raise UnsupportedDevice('AF-S statistics prior lifetime identity changed')
    return current


def project(device, words):
    """Project this new lifetime's ACK words into the parent's expected view."""
    data = validate(device)
    if data is not None:
        words.update(data['ack_words'])
    return words


def access(device, operation, core, address, size, value=None):
    if address not in tuple(device.BASE+offset for offset in ACK_OFFSETS):
        return False
    data = validate(device)
    if data is None or address not in data['ack_words']:
        return False
    if (data['state'] != 'delivered' or core != 0 or type(core) is not int or type(size) is not int or size != 4
            or len(data['events']) >= EVENT_LIMIT):
        raise UnsupportedDevice('AF-S statistics ACK requires delivered data and its WORD owner')
    channel, step = data['ack_channel'], data['ack_step']
    clear, flag = device.BASE + 0x88c + channel * 0x100, device.BASE + 0x8f4 + channel * 0x100
    word = device.registers[address] if operation == 'read' else value
    if type(word) is not int or word not in (0, 1) or device.registers[address] != data['ack_words'][address]:
        raise UnsupportedDevice('AF-S statistics live ACK latch changed')
    if operation == 'write' and step == 0 and address == clear and word == 0:
        next_step = 1
    elif operation == 'write' and step == 1 and address == flag and word == 1:
        next_step = 2
    elif operation == 'read' and step == 2 and address == clear and word == 0:
        next_step = 3
    elif operation == 'read' and step == 3 and address == flag:
        next_step = 2 if word == 1 else 0
    else:
        raise UnsupportedDevice('AF-S statistics require the next whole paired ACK access')
    now = device.board.virtual_ticks
    if (type(now) is not int or not 0 <= now < (1 << 64) - device.ack_ticks
            or operation == 'write' and step == 1 and data['ack_due_tick'] is not None):
        raise UnsupportedDevice('AF-S statistics ACK timer is unbounded or repeated')
    data['events'].append(dict(operation=operation, cpu=core, address=address, word=word, tick=now))
    data['ack_step'] = next_step
    if operation == 'write':
        old = device.registers[address]
        device.registers[address] = data['ack_words'][address] = word
        device.writes.append(dict(cpu=core,address=hex(address),value=hex(word),old=hex(old)))
        if step == 1:
            data['ack_due_tick'] = now + device.ack_ticks
    if operation == 'read' and address == flag and word == 0:
        data['ack_channel'] += 1
        if data['ack_channel'] == 2:
            data.update(state='acknowledged', acknowledged_tick=now)
    _seal(data)
    return True


def tick(device, board):
    record=device.rearm
    current=record.get('afs_statistics') if type(record) is dict else None
    if current is None or (type(current) is dict and current.get('state')=='acknowledged'
            and type(current.get('ack_channel')) is int and current['ack_channel']==2
            and type(current.get('ack_step')) is int and current['ack_step']==0
            and current.get('ack_due_tick') is None):
        return False
    data = validate(device)
    if data is None:
        return False
    if board is not device.board:
        raise UnsupportedDevice('AF-S statistics tick belongs to another board')
    if data['state'] == 'waiting-source':
        observed = _epoch(board,data['latch']['source_frame'])
        if observed is not None:
            data.update(state='pending', source_epoch=observed, due_tick=observed['asserted_tick'] + device.data_ticks)
            _seal(data)
    if data['state'] == 'pending' and board.virtual_ticks >= data['due_tick']:
        device._validate_rearm()
        validate(device, check_route=True)
        scene = data['latch']['scene']
        raw = bytes(board.read_memory(int(scene['address'], 16), scene['bytes']))
        if hashlib.sha256(raw).hexdigest() != scene['payload_sha256']:
            raise UnsupportedDevice('AF-S statistics frozen source changed before delivery')
        for span in data['latch']['plan']:
            board.write_memory(span['address'], bytes.fromhex(span['payload_hex']), cpu_id=0)
        data.update(state='delivered', completed_tick=board.virtual_ticks)
        _seal(data)
        board.queue_irq(0, 46)
        board.record('AF-S image-derived statistics delivered before IRQ46', profile=data['profile'],
                     source_epoch=data['source_epoch'], scene_sha256=scene['payload_sha256'],
                     limits=data['latch']['assumptions'])
    elif data['state'] == 'delivered' and data['ack_due_tick'] is not None and board.virtual_ticks >= data['ack_due_tick']:
        address = device.BASE + 0x8f4 + data['ack_channel'] * 0x100
        if device.registers[address] != data['ack_words'][address] or data['ack_words'][address] != 1:
            raise UnsupportedDevice('AF-S statistics ACK response lacks its actual request')
        device.registers[address] = data['ack_words'][address] = 0
        data['ack_due_tick'] = None
        _seal(data)
    # A completed lifetime owns no timer. Let the parent advance later
    # shutdown/preview lifetimes rather than hiding their pending work.
    return data['state'] != 'acknowledged'


def due(device):
    record=device.rearm
    current=record.get('afs_statistics') if type(record) is dict else None
    if current is None or (type(current) is dict and current.get('state')=='acknowledged'
            and type(current.get('ack_channel')) is int and current['ack_channel']==2
            and type(current.get('ack_step')) is int and current['ack_step']==0
            and current.get('ack_due_tick') is None):
        return None
    data = validate(device)
    if data is None:
        return None
    return data['due_tick'] if data['state'] == 'pending' else data['ack_due_tick'] if data['state'] == 'delivered' else None
