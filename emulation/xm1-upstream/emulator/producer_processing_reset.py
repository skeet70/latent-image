"""One explicit post-PFP GLOBAL reset, without producer reactivation.

The optional container owns the new physical GLOBAL word. The predecessor
registers/arm/data remain historical; reset alone provides no DMA or IRQ.
"""
from copy import deepcopy

from .hardware import UnsupportedDevice
from .histogram_protocol import digest
from . import producer_still_capture as still, afs_still_numeric as numeric

PROFILE = 'declared-post-prefilter-producer-global-reset-unmeasured'
ATTRIBUTE = 'processing_reset'
POLL_LIMIT = 4096
WORDS = {0x15e32d0: 4, 0x15d502c: 0, 0x15f63a4: 3, 0x15f63a8: 0,
    0x15f64a4: 0, 0x15f68a4: 0, 0x15f68a8: 0, 0x15f6bf4: 0,
    0x15f6e50: 1, 0x15f6e60: 0, 0x173d308: 1,
    0x17d54dc: 1, 0x17d54e0: 0, 0x17d54ec: 0,
    0x152b724: 6, 0xbe391c: 0, 0xbe3920: 0}


def owner(device):
    return getattr(device, ATTRIBUTE, None)


def _route(device):
    if any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
           or bank.active == 46 or bank.injected == 46
           or any(item.get('line') == 46 for item in bank.active_stack)
           for bank in device.board.interrupts.banks):
        raise UnsupportedDevice('Processing GLOBAL reset requires retired drained numeric IRQ46')


def _prefilter_anchor(device):
    matches = [item for item in device.board.devices if type(item).__name__ == 'StillProcessingControl']
    if len(matches) != 1:
        raise UnsupportedDevice('Processing GLOBAL reset requires the completed four-segment PFP owner')
    processing = matches[0]
    empty = processing.prefilter_empty_summary()
    third = processing.prefilter_third_summary()
    if (empty is None or empty['configuration'] is None
            or empty['configuration']['state'] != 'complete'
            or third is None or third['job']['state'] != 'complete'):
        raise UnsupportedDevice('Processing GLOBAL reset precedes genuine empty/PFP closure')
    return dict(empty_contract=empty['configuration']['contract_sha256'],
        empty_closed_tick=empty['configuration']['closed_tick'],
        third_output_sha256=third['job']['contract']['output_sha256'])


def _prior(device):
    device._validate_rearm()
    data = numeric.anchor(device)
    if data['state'] != 'acknowledged':
        raise UnsupportedDevice('Processing GLOBAL reset requires actual finite numeric retirement')
    return dict(registers_sha256=digest(device.registers), activation=still.source_anchor(device),
                numeric=data, prefilter=_prefilter_anchor(device))


def _seal(record):
    record['seal'] = digest({key: value for key, value in record.items() if key not in ('seal', 'prior')})


def arm(device, profile=PROFILE):
    if (profile != PROFILE or owner(device) is not None or device.board.failure is not None
            or not device._idle() or device.busy or device.registers[device.GLOBAL] not in (0x10000000, 0x10030000)
            or device.stop_profile != 'stock-idle-stop-handshakes-unmeasured'):
        raise UnsupportedDevice('Processing GLOBAL reset requires its explicit single closed-owner profile')
    read = lambda address: int.from_bytes(device.board.read_memory(address, 4, cpu_id=0), 'little')
    handles = dict(subscription=read(0x15f6e5c), allocation=read(0x15233bc))
    if (any(read(address) != value for address, value in WORDS.items())
            or not all(0 < value <= 0xffff for value in handles.values())
            or handles['subscription'] == handles['allocation']):
        raise UnsupportedDevice('Processing GLOBAL reset requires the actual12018B8 cached owner/mask4 caller')
    _route(device)
    prior = _prior(device)
    record = dict(profile=PROFILE, state='awaiting-read', armed_tick=device.board.virtual_ticks,
        prior=prior, prior_sha256=digest(prior), handles=handles,
        initial_word=device.registers[device.GLOBAL], word=device.registers[device.GLOBAL],
        requested_tick=None, due_tick=None, ready_tick=None, closed_tick=None,
        acknowledged_poll_count=None, polls=0, busy_polls=0, ready_polls=0, seal=None)
    _seal(record); setattr(device, ATTRIBUTE, record)
    return summary(device)


def validate(device, *, full=False):
    record = owner(device)
    if record is None: return None
    if (type(record) is not dict or record.get('profile') != PROFILE
            or record.get('state') not in ('awaiting-read', 'awaiting-clear', 'waiting', 'ready', 'complete')
            or record.get('seal') != digest({key: value for key, value in record.items() if key not in ('seal', 'prior')})
            or record['prior_sha256'] != digest(record['prior'])
            or record['initial_word'] not in (0x10000000, 0x10030000)
            or device.registers[device.GLOBAL] != record['initial_word']
            or any(type(record[key]) is not int or not 0 <= record[key] <= POLL_LIMIT
                   for key in ('polls', 'busy_polls', 'ready_polls'))
            or record['polls'] != record['busy_polls'] + record['ready_polls']):
        raise UnsupportedDevice('Processing GLOBAL reset changed its bounded owner/history')
    expected = (record['initial_word'] if record['state'] in ('awaiting-read', 'awaiting-clear') else
                0 if record['state'] == 'waiting' else 0x10000000)
    if (record['word'] != expected
            or record['due_tick'] is not None and record['due_tick'] != record['requested_tick'] + device.stop_ticks
            or record['ready_tick'] is not None and record['acknowledged_poll_count'] != record['busy_polls']
            or (record['state'] == 'complete') != (record['closed_tick'] is not None)):
        raise UnsupportedDevice('Processing GLOBAL reset status/poll boundary changed')
    _route(device)
    if full and record['prior'] != _prior(device):
        raise UnsupportedDevice('Processing GLOBAL reset altered its closed numeric/PFP predecessor')
    return record


def access(device, operation, cpu, address, size, value=None):
    record = validate(device)
    if record is None: return False, None
    if cpu != 0 or type(cpu) is not int or size != 4 or type(size) is not int or address != device.GLOBAL:
        raise UnsupportedDevice('Post-PFP producer owns only the next exact CPU0 GLOBAL reset transaction')
    state, now = record['state'], device.board.virtual_ticks
    if operation == 'read' and state == 'awaiting-read':
        record['state'] = 'awaiting-clear'
    elif operation == 'write' and state == 'awaiting-clear' and type(value) is int and value == 0:
        validate(device, full=True)
        record.update(state='waiting', word=0, requested_tick=now, due_tick=now + device.stop_ticks)
    elif operation == 'read' and state in ('waiting', 'ready'):
        if record['polls'] >= POLL_LIMIT:
            raise UnsupportedDevice('Processing GLOBAL reset exceeds bounded physical polling')
        record['polls'] += 1
        record['busy_polls' if state == 'waiting' else 'ready_polls'] += 1
        if state == 'ready': record.update(state='complete', closed_tick=now)
    else:
        raise UnsupportedDevice('Processing GLOBAL reset excludes reordered/repeated reset and fresh activation')
    _seal(record)
    return True, record['word'] if operation == 'read' else None


def tick(device, board):
    record = owner(device)
    if record is None or record['state'] != 'waiting' or board.virtual_ticks < record['due_tick']:
        return
    validate(device, full=True)
    record.update(state='ready', word=0x10000000, ready_tick=board.virtual_ticks,
                  acknowledged_poll_count=record['polls'])
    _seal(record)


def due(device):
    record = owner(device)
    return record['due_tick'] if record is not None and record['state'] == 'waiting' else None


def summary(device):
    record = validate(device, full=True)
    return deepcopy({key: value for key, value in record.items() if key != 'seal'}) if record is not None else None
