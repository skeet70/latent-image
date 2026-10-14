"""Explicit sensor clock opportunities across the S1 mode-change boundary.

This independent input owns no MMIO, DMA, guest memory or callbacks. The
firmware's source0 ISR/subscribers and selector1 write acknowledge its IRQ.
The original seven-event or selected eight-event card startup remains unchanged. Two inputs close the original
statistics lifetime; a separate third initializes the actual later measurement
request, and a fourth offers its first post-enable frame. No cadence or physical
X70 timing is implied; a separate consumer/data model must supply pixels.
"""
from __future__ import annotations

from copy import deepcopy
import hashlib
import json
import struct

from .hardware import UnsupportedDevice
from .input_processing_devices import REC_DISPLAY_EVENT_PROFILES
from . import afs_startup_scope


PROFILE = 'declared-afs-post-s1-one-source0-edge-unmeasured'
CONTINUATION_PROFILE = 'declared-afs-second-source0-edge-after-paired-ack-unmeasured'
MEASUREMENT_PROFILE = 'declared-afs-generation4-measurement-source0-edge-unmeasured'
MEASUREMENT_FRAME_PROFILE = 'declared-afs-generation4-first-pixel-source0-edge-unmeasured'
MEASUREMENT_NEXT_FRAME_PROFILE = 'declared-afs-generation4-second-pixel-source0-edge-after-paired-ack-unmeasured'
MEASUREMENT_PIXEL_PROFILES = (MEASUREMENT_FRAME_PROFILE, MEASUREMENT_NEXT_FRAME_PROFILE)
MEASUREMENT_PROFILES = (MEASUREMENT_PROFILE,) + MEASUREMENT_PIXEL_PROFILES
AF_CONTROL_PROFILE = 'declared-afs-contrast-setup-clock-unmeasured'
AF_PIXEL_PROFILE = 'declared-afs-contrast-pixel-pair-unmeasured'
PROFILES = (PROFILE,)
IRQ = 0x27
MEASUREMENT_SLOTS = struct.pack('<18I',
    0x4c, 0x50, 0, 0x0352014d, 0, 0,
    0x50, 0x50, 0, 0x02580340, 0, 0,
    0x50, 0x50, 0xb, 0x025804cc, 0, 0) + bytes(120)
MEASUREMENT_ACTIVE_SLOTS = b''.join(
    MEASUREMENT_SLOTS[index:index + 20] + struct.pack('<I', 1)
    for index in (0, 24, 48)) + bytes(120)


def _digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                    allow_nan=False).encode()).hexdigest()


def _source(board):
    devices = [device for device in board.devices if type(device).__name__ == 'AfsFrameSource']
    if len(devices) != 1 or devices[0].board is not board:
        raise UnsupportedDevice('AF input reader requires one same-board frame source')
    return devices[0]


def _legacy_request(record):
    """Project the retained Sources5..9 independently of later AF clocks."""
    return deepcopy({key: value for key, value in record.items() if key not in ('af_stream', 'preview_return', 'capture_initialization', 'still_initialization')})


def af_requests(board):
    """Copied flat AF records; no clocks are admitted by this read interface.

    A separate, qualified active clock owner must validate any ``af_stream``
    before these records can become numeric input.  Until that owner exists,
    an injected or unknown ledger is an explicit refusal rather than a frame.
    """
    source = _source(board)
    source._validate()
    if source.request is None or 'af_stream' not in source.request:
        return []
    validator = getattr(source, '_validate_af_stream', None)
    if validator is None:
        raise UnsupportedDevice('Contrast AF sensor clocks have not been admitted')
    stream = validator()
    if type(stream) is not dict or type(stream.get('records')) is not list:
        raise UnsupportedDevice('Contrast AF stream lacks its validated flat records')
    return deepcopy(stream['records'])


def af_pending_pair(board):
    """The one admitted pixel pair before its source0 edge, or None."""
    records = af_requests(board)
    matches = [record for record in records if record.get('kind') == 'pixel'
               and record.get('source0', {}).get('state') == 'waiting']
    if len(matches) > 1:
        raise UnsupportedDevice('Contrast AF has more than one waiting pixel pair')
    return deepcopy(matches[0]) if matches else None


def af_pair(board, pair_id):
    """A pose-consumed pixel pair, or None before physical/stock completion."""
    if type(pair_id) is not int or not 1 <= pair_id <= 251:
        raise UnsupportedDevice('Contrast AF pair identity is outside its stock scan bound')
    records = af_requests(board)
    matches = [record for record in records if record.get('kind') == 'pixel'
               and record.get('pair_id') == pair_id]
    if len(matches) > 1:
        raise UnsupportedDevice('Contrast AF pair identity is ambiguous')
    if not matches or matches[0].get('consumed_tick') is None:
        return None
    source = _source(board)
    projector = getattr(source, '_af_pair_projection', None)
    if projector is None:
        raise UnsupportedDevice('Contrast AF pair lacks its stock pose-consumption projection')
    return deepcopy(projector(matches[0]))


def validate(board):
    """Return a copied arm marker for a data consumer, without guest effects."""
    devices = [device for device in board.devices if type(device).__name__ == 'AfsFrameSource']
    if len(devices) != 1 or devices[0].board is not board:
        raise UnsupportedDevice('AF-S data consumer requires one same-board frame source')
    source = devices[0]
    source._validate()
    if source.request is None:
        raise UnsupportedDevice('AF-S data consumer requires an explicitly armed source')
    return _legacy_request(source.request)


def epoch(board):
    """Return the asserted external frame identity, or None before assertion."""
    validate(board)
    source = next(device for device in board.devices if type(device).__name__ == 'AfsFrameSource')
    return source.source_epoch()


def requests(board):
    """Copied source lifetimes in chronological order, including measurement."""
    devices = [device for device in board.devices if type(device).__name__ == 'AfsFrameSource']
    if len(devices) != 1 or devices[0].board is not board:
        raise UnsupportedDevice('AF-S input history requires one same-board source')
    source = devices[0]
    source._validate()
    if source.request is None:
        return []
    return deepcopy(source.request.get('history', [])) + [_legacy_request(source.request)]


def epoch_for(board, frame):
    """Resolve a retained physical source epoch, including the closed first."""
    if type(frame) is not int or frame not in (5, 6, 7, 8, 9):
        raise UnsupportedDevice('AF-S source frame is outside the bounded input experiment')
    matches = [record for record in requests(board) if record['frame'] == frame]
    if not matches or matches[0]['state'] not in ('asserted', 'acknowledged'):
        return None
    return {key: matches[0][key] for key in ('profile', 'generation', 'ordinal', 'attempt',
                   'frame', 'cpu', 'source', 'physical_irq', 'asserted_tick')}


class AfsFrameSource:
    def __init__(self, board, *, profile=None, ticks=400000):
        if profile is not None and profile not in PROFILES:
            raise ValueError('Unknown AF-S frame source profile')
        if type(ticks) is not int or not 1 <= ticks < 1 << 64:
            raise ValueError('AF-S frame source delay must be positive virtual ticks')
        self.board, self.profile, self.ticks = board, profile, ticks
        self.request = None
        board.record('Declared AF-S frame source attached', profile=profile, ticks=ticks,
                     limits='Disabled until explicit arm; two shooting, one measurement-init and two post-enable edges, no image/result/RAM/callback')

    def contains(self, address, size):
        return False

    def af_admission(self):
        from . import afs_contrast_clock
        return afs_contrast_clock.admission(self)

    def af_summary(self):
        from . import afs_contrast_clock
        return afs_contrast_clock.summary(self)

    def arm_af_control(self, profile=AF_CONTROL_PROFILE, *, cadence_ticks=1000):
        from . import afs_contrast_clock
        if profile != AF_CONTROL_PROFILE:
            raise UnsupportedDevice('Unknown explicit contrast AF control clock profile')
        query = self.af_admission()
        if query['kind'] != 'control':
            raise UnsupportedDevice(query['reason'] or 'Stock AF is not awaiting a control clock')
        return afs_contrast_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def arm_af_pair(self, profile=AF_PIXEL_PROFILE, *, cadence_ticks=1000, exposure_ticks=1000):
        from . import afs_contrast_clock
        if profile != AF_PIXEL_PROFILE:
            raise UnsupportedDevice('Unknown explicit contrast AF pixel clock profile')
        query = self.af_admission()
        if query['kind'] != 'pixel':
            raise UnsupportedDevice(query['reason'] or 'Stock AF is not awaiting a pixel pair')
        return afs_contrast_clock.arm(self, profile, cadence_ticks=cadence_ticks, exposure_ticks=exposure_ticks)

    def arm_preview_return(self, profile, *, cadence_ticks=1000):
        from . import afs_preview_return_clock
        return afs_preview_return_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def preview_return_summary(self):
        from . import afs_preview_return_clock
        return afs_preview_return_clock.summary(self)

    def arm_still_initialization(self, profile, *, cadence_ticks=1000):
        from . import afs_still_initialization_clock
        return afs_still_initialization_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_initialization_summary(self):
        from . import afs_still_initialization_clock
        return afs_still_initialization_clock.summary(self)

    def arm_still_initialization_next(self, profile, *, cadence_ticks=1000):
        from . import afs_still_qualifying_clock
        return afs_still_qualifying_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_qualifying_clock_summary(self):
        from . import afs_still_qualifying_clock
        return afs_still_qualifying_clock.summary(self)

    def arm_still_readout_start(self, profile, *, cadence_ticks=1000):
        from . import afs_still_readout_clock
        return afs_still_readout_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_readout_clock_summary(self):
        from . import afs_still_readout_clock
        return afs_still_readout_clock.summary(self)

    def arm_still_tail_boundary(self, profile, *, cadence_ticks=1000):
        from . import afs_still_tail_clock
        return afs_still_tail_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_tail_clock_summary(self):
        from . import afs_still_tail_clock
        return afs_still_tail_clock.summary(self)

    def arm_still_image_boundary(self, profile, *, cadence_ticks=1000):
        from . import afs_still_image_clock
        return afs_still_image_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_image_clock_summary(self):
        from . import afs_still_image_clock
        return afs_still_image_clock.summary(self)

    def arm_still_cleanup_start(self, profile, *, cadence_ticks=1000):
        from . import afs_still_cleanup_clock
        return afs_still_cleanup_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def arm_still_cleanup_finish(self, profile, *, cadence_ticks=1000):
        from . import afs_still_cleanup_clock
        return afs_still_cleanup_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def still_cleanup_clock_summary(self):
        from . import afs_still_cleanup_clock
        return afs_still_cleanup_clock.summary(self)

    def arm_capture_initialization(self, profile, *, cadence_ticks=1000):
        from . import afs_capture_clock
        return afs_capture_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def capture_initialization_summary(self):
        from . import afs_capture_clock
        return afs_capture_clock.summary(self)

    def arm_capture_acquisition_start(self, profile, *, cadence_ticks=1000):
        from . import afs_capture_start_clock
        return afs_capture_start_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def capture_acquisition_start_summary(self):
        from . import afs_capture_start_clock
        return afs_capture_start_clock.summary(self)

    def arm_capture_finite_stop(self, profile, *, cadence_ticks=1000):
        from . import afs_capture_stop_clock
        return afs_capture_stop_clock.arm(self, profile, cadence_ticks=cadence_ticks)

    def capture_finite_stop_summary(self):
        from . import afs_capture_stop_clock
        self._validate()
        return afs_capture_stop_clock.summary(self)

    def _validate_af_stream(self):
        from . import afs_contrast_clock
        return afs_contrast_clock.validate(self)

    def _af_pair_projection(self, record):
        from . import afs_contrast_clock
        return afs_contrast_clock.pair_projection(record)

    def _device(self, name):
        matches = [device for device in self.board.devices if type(device).__name__ == name]
        if len(matches) != 1 or matches[0].board is not self.board:
            raise UnsupportedDevice('AF-S source requires one same-board ' + name)
        return matches[0]

    def _prefix(self, *, withdrawn=False, historical=False):
        source = self._device('InputProcessingControl')
        source._validate_rec_stream()
        selected = self.startup_scope()
        if (source.event_profile not in REC_DISPLAY_EVENT_PROFILES
                or (source.event_attempted_generation,
                    source.event_arms, source.event_assertions, source.event_acknowledgments,
                    source.event_extra_attempts, source.event_ordinal) != selected['counters']
                or (source.event_enable_generation < 3 if historical else source.event_enable_generation != 3)
                or source.event_pending or source.event_due is not None
                or source.event_followup is not None or source.event_stream_blocked and not (withdrawn or historical)
                or len(source.event_history) != selected['history_count']
                or any(event['state'] != 'acknowledged' for event in source.event_history)
                or source.last_event['cpu'] != 0):
            raise UnsupportedDevice('AF-S source requires its exact exhausted seven/eight-ACK generation3 prefix')
        return source

    def startup_scope(self):
        """Pure selected startup identities; this query offers no new edge."""
        return afs_startup_scope.scope(self._device('InputProcessingControl').event_profile)

    def _producer_anchor(self, *, live=False, historical=False, expected=None, view=None):
        # The view belongs only to one synchronous validation traversal. It
        # avoids replaying the same immutable archive for every nested copy,
        # while a later public call always resolves the live records again.
        if view is None:
            view = {}
        if 'producer' not in view:
            view['producer'] = self._device('ProducerControl')
        producer = view['producer']
        if live:
            producer._validate_rearm()
        record = producer.rearm
        fields = ('descriptor_step', 'setup_step', 'enable_step', 'descriptor_accesses',
                  'setup_accesses', 'enable_accesses', 'started_tick', 'armed_tick',
                  'enabled_tick', 'route', 'original_history_sha256')
        if historical and expected is not None:
            if 'historical_anchors' not in view:
                records = getattr(producer, 'rearm_records', None)
                candidates = records() if records is not None else [record]
                if type(candidates) is not list or not 1 <= len(candidates) <= 2:
                    raise UnsupportedDevice('AF-S source producer history exceeds its two-lifetime bound')
                view['historical_anchors'] = [(candidate, {
                    'enabled_tick': candidate['enabled_tick'],
                    'sha256': _digest({key: candidate[key] for key in fields})})
                    for candidate in candidates if type(candidate) is dict and set(fields) <= set(candidate)]
            matches = [candidate for candidate, anchor in view['historical_anchors'] if anchor == expected]
            if len(matches) != 1:
                raise UnsupportedDevice('AF-S source historical producer anchor is absent or ambiguous')
            record = matches[0]
        if 'original_history_sha256' not in view:
            view['original_history_sha256'] = producer._rearm_history_hash()
        if (type(record) is not dict or not set(fields) <= set(record)
                or (record['descriptor_step'], record['setup_step'], record['enable_step']) != (10, 7, 2)
                or not historical and record.get('state') != 'enabled-awaiting-fresh-input'
                or type(record['enabled_tick']) is not int
                or not 0 <= record['enabled_tick'] <= self.board.virtual_ticks
                or type(record['route']) is not int or record['route'] != 0
                or record['original_history_sha256'] != view['original_history_sha256']):
            raise UnsupportedDevice('AF-S source requires the actual enabled fresh producer arm')
        if historical and expected is not None:
            return deepcopy(expected)
        return {'enabled_tick': record['enabled_tick'], 'sha256': _digest({key: record[key] for key in fields})}

    def _measurement_contract(self, *, pixels=False, continuing=False):
        """Read only the actual accepted request and registered source0 clients."""
        raw = lambda address, size: bytes(self.board.read_memory(address, size, cpu_id=0))
        word = lambda address: int.from_bytes(raw(address, 4), 'little')
        callback_list = raw(0xc6dc58, 40)
        if len(callback_list) != 40:
            raise UnsupportedDevice('AF-S measurement source callback list is truncated')
        words = struct.unpack('<10I', callback_list)
        fixed = ((0x15e32d0, 5), (0x15d4688, 2), (0x15d468c, 1),
                 (0x15d476c, 1 if continuing else 0),
                 (0x15d4770, 0x10102 if continuing else 1 if pixels else 0), (0x15d47bc, 0x12421d0),
                 (0x15d4880, 3), (0x15d4884, 0x1613ab4),
                 (0x15f6bf4, 1 if pixels else 0), (0x15f6a60, 0))
        if pixels:
            fixed += ((0x15f68a4, 1), (0x15f68a8, 0x12f604c), (0x173d308, 0x50))
        slots = raw(0x15d47c0, 192)
        if (any(word(address) != value for address, value in fixed)
                or slots != (MEASUREMENT_ACTIVE_SLOTS if pixels else MEASUREMENT_SLOTS) or words[0] != 2
                or words[1] == 0 or words[2] == 0 or words[1] == words[2]
                or any(words[index] for index in (3, 4, 5))
                or tuple(words[6:10]) != (0x11f6008, 2, 0x12f8704, 1)
                or words[1] != word(0x15f6e5c) or words[2] != word(0x15d4690)):
            raise UnsupportedDevice('AF-S measurement requires its exact generation4 request and producer phase')
        return dict(request_pointer=0x15e77c8, callback=0x12421d0, descriptor_pointer=0x1613ab4,
                    descriptor_count=3, slots_sha256=hashlib.sha256(slots).hexdigest(),
                    producer_handle=words[1], measurement_handle=words[2],
                    subscribers_sha256=hashlib.sha256(callback_list).hexdigest())

    def _measurement_producer_anchor(self, *, expected=None, view=None):
        # One local view per exact activation. A later initializer may own a
        # different current activation while frames8/9 retain the old one.
        if expected is not None and (type(expected) is not dict
                or set(expected) != {'enabled_tick', 'sha256'}
                or type(expected['enabled_tick']) is not int
                or not 0 <= expected['enabled_tick'] <= self.board.virtual_ticks
                or type(expected['sha256']) is not str or len(expected['sha256']) != 64):
            raise UnsupportedDevice('Measurement source expected activation anchor is malformed')
        key = 'current' if expected is None else _digest(expected)
        anchors = view.setdefault('measurement_producers', {}) if view is not None else {}
        if key in anchors:
            return deepcopy(anchors[key])
        producer = self._device('ProducerControl')
        getter = getattr(producer, 'measurement_source_anchor', None)
        if getter is None:
            raise UnsupportedDevice('Measurement pixel input requires the enabled new producer anchor')
        anchor = getter() if expected is None else getter(expected=deepcopy(expected))
        if (type(anchor) is not dict or set(anchor) != {'enabled_tick', 'sha256'}
                or type(anchor['enabled_tick']) is not int
                or not 0 <= anchor['enabled_tick'] <= self.board.virtual_ticks
                or type(anchor['sha256']) is not str or len(anchor['sha256']) != 64
                or expected is not None and anchor != expected):
            raise UnsupportedDevice('Measurement pixel input has an unknown enabled producer anchor')
        anchors[key] = deepcopy(anchor)
        return deepcopy(anchor)

    def _measurement_data_anchor(self, previous):
        """Consume a sealed, copied ACK anchor without calling a source consumer."""
        from .afs_measurement_data import PROFILE as data_profile
        from .afs_statistics_frame import _input_hash
        producer = self._device('ProducerControl')
        getter = getattr(producer, 'measurement_data_anchor', None)
        if getter is None:
            raise UnsupportedDevice('Next measurement frame requires the real completed data anchor')
        anchor = getter(frame=8)
        keys = {'profile', 'state', 'source_epoch', 'input_sha256', 'producer',
                'acknowledged_tick', 'ack_channel', 'ack_step', 'ack_due_tick', 'ack_words', 'state_sha256'}
        expected_epoch = {key: previous[key] for key in ('profile', 'generation', 'ordinal', 'attempt',
                         'frame', 'cpu', 'source', 'physical_irq', 'asserted_tick')}
        if (type(anchor) is not dict or set(anchor) != keys
                or previous.get('state') != 'acknowledged' or previous.get('frame') != 8
                or anchor.get('profile') != data_profile or anchor.get('state') != 'acknowledged'
                or anchor.get('source_epoch') != expected_epoch
                or anchor.get('input_sha256') != _input_hash(previous)
                or anchor.get('producer') != previous.get('measurement_producer')
                or type(anchor.get('acknowledged_tick')) is not int
                or not previous['asserted_tick'] <= anchor['acknowledged_tick'] <= self.board.virtual_ticks
                or type(anchor.get('ack_channel')) is not int or anchor['ack_channel'] != 2
                or type(anchor.get('ack_step')) is not int or anchor['ack_step'] != 0
                or anchor.get('ack_due_tick') is not None
                or anchor.get('ack_words') != {producer.BASE + offset: 0 for offset in (0x88c, 0x8f4, 0x98c, 0x9f4)}
                or type(anchor.get('state_sha256')) is not str or len(anchor['state_sha256']) != 64):
            raise UnsupportedDevice('Next measurement frame lost the real frame8 paired ACK/activation identity')
        return deepcopy(anchor)

    def _measurement_gates(self, source):
        return (source.event_enable_generation == 4 and source.registers.get(0x48) == 1
                and source.registers.get(0x4c) == 1 and source.registers.get(0x70) == 0
                and source.registers.get(0x38c) == 0x10201
                and source.busy is False and source.phase_busy is False
                and source.pending_command is None and source.command_armed is None)

    def _gates(self, source):
        return (source.registers.get(0x48) == 1 and source.registers.get(0x4c) == 1
                and source.registers.get(0x70) == 0 and source.registers.get(0x38c) == 0x10203
                and source.busy is False and source.phase_busy is False
                and source.pending_command is None and source.command_armed is None)

    def _route(self, source):
        banks = self.board.interrupts.banks
        if len(banks) != 2 or any(type(bank.enabled) is not int or not 0 <= bank.enabled < 1 << 128
                                  for bank in banks):
            raise UnsupportedDevice('AF-S source IRQ enable banks have an unknown domain')
        if source._event_route(IRQ) != 0:
            return False
        bank = banks[0]
        if (type(bank.config) is not dict
                or type(bank.source_config) is not int or not 0 <= bank.source_config < 1 << 128
                or type(bank.pending) is not int or not 0 <= bank.pending < 1 << 128):
            raise UnsupportedDevice('AF-S source IRQ registration has an unknown domain')
        return (bank.config.get(0x400 + 4 * IRQ) == 0x9df0b8
                and bank.config.get(0x800 + 4 * IRQ) == 10
                and bool(bank.source_config & (1 << IRQ))
                and not bank.pending & (1 << IRQ)
                and bank.active != IRQ and bank.injected != IRQ)

    def arm(self, profile=PROFILE):
        """Accept one explicit future external input; never arm from guest state."""
        if profile != PROFILE or self.profile != PROFILE or self.request is not None:
            raise UnsupportedDevice('Unknown, disabled or repeated AF-S source experiment')
        if self.board.failure is not None:
            raise UnsupportedDevice('Restore before arming input after a strict failure')
        source = self._prefix()
        anchor = self._producer_anchor(live=True)
        if (self.startup_scope()['ordinal_offset'] and
                (self._device('ProducerControl').rearm.get('rotation_count') != 4
                 or self._device('ProducerControl').rearm['rotations'][-1]['state'] != 'completed')):
            raise UnsupportedDevice('Card AF-S admission requires the four completed stock startup rotations')
        now = self.board.virtual_ticks
        if (type(now) is not int or not 0 <= now < (1 << 64) - self.ticks
                or not self._gates(source) or not self._route(source)):
            raise UnsupportedDevice('AF-S source requires a live idle source0 owner/route and bounded delay')
        identity = afs_startup_scope.identity(source.event_profile, PROFILE)
        record = dict(profile=PROFILE, generation=identity[1], ordinal=identity[2], attempt=identity[3], frame=identity[4],
                      cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                      requested_tick=now, due_tick=now + self.ticks, delay_ticks=self.ticks,
                      asserted_tick=None, acknowledged_tick=None, canceled_tick=None, reason=None,
                      state='waiting', prefix_sha256=_digest(source.event_history), producer=anchor,
                      write_cursor=len(source.writes))
        self.request = record
        self.board.record('AF-S external source0 edge explicitly armed', **record,
                          limits='One future edge; selected startup ledger retained; no DMA/status/result/guest callback')
        return deepcopy(record)

    def arm_next(self, profile):
        """Offer exactly one second input after the first hardware lifetime closed."""
        if (profile != CONTINUATION_PROFILE or self.board.failure is not None
                or self.request is None or self.request.get('profile') != PROFILE
                or self.request.get('state') != 'acknowledged'):
            raise UnsupportedDevice('Second AF-S input requires the explicit continuation profile and first ACK')
        source = self._validate()
        # The retained first lifetime may survive a stock retune. Offering a
        # second edge still requires the exact live generation3 prefix.
        source = self._prefix()
        producer = self._device('ProducerControl')
        producer._validate_rearm()
        from .afs_statistics_frame import validate as validate_statistics, _route as statistics_route
        data = validate_statistics(producer)
        current_epoch = self.source_epoch()
        if (data is None or data['state'] != 'acknowledged' or data['ack_channel'] != 2
                or data['ack_step'] != 0 or data['ack_due_tick'] is not None
                or data['source_epoch'] != current_epoch):
            raise UnsupportedDevice('Second AF-S input requires the first whole paired statistics ACK')
        record = producer.rearm
        rotations = record['rotations']
        if (record['rotation_count'] != 5 or len(rotations) != 5
                or rotations[-1]['state'] != 'completed' or rotations[-1]['ordinal'] != 5
                or type(rotations[-1]['completed_tick']) is not int
                or not self.request['asserted_tick'] <= rotations[-1]['completed_tick'] <= self.request['acknowledged_tick']):
            raise UnsupportedDevice('Second AF-S input requires its completed fifth stock bank rotation')
        statistics_route(producer)
        now = self.board.virtual_ticks
        if (type(now) is not int or not data['acknowledged_tick'] <= now < (1 << 64) - self.ticks
                or not self._gates(source) or not self._route(source)):
            raise UnsupportedDevice('Second AF-S input requires drained IRQ27/46 and the unchanged source0 route')
        history = [deepcopy(self.request)]
        identity = afs_startup_scope.identity(source.event_profile, profile)
        request = dict(profile=profile, generation=identity[1], ordinal=identity[2], attempt=identity[3], frame=identity[4],
                       cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                       requested_tick=now, due_tick=now + self.ticks, delay_ticks=self.ticks,
                       asserted_tick=None, acknowledged_tick=None, canceled_tick=None, reason=None,
                       state='waiting', prefix_sha256=_digest(source.event_history),
                       producer=self._producer_anchor(), write_cursor=len(source.writes),
                       history=history, history_sha256=_digest(history))
        self.request = request
        self.board.record('Second AF-S external source0 edge explicitly armed', **request,
                          limits='Second and final input; previous lifetime retained; no repeating cadence or AF result')
        return deepcopy(request)

    def arm_measurement(self, profile):
        """Offer one source0 edge for the stock post-retune three-slot request."""
        if (profile != MEASUREMENT_PROFILE or self.board.failure is not None
                or self.request is None or self.request.get('profile') != CONTINUATION_PROFILE
                or self.request.get('state') != 'acknowledged'):
            raise UnsupportedDevice('Measurement input requires its named profile and two closed shooting inputs')
        source = self._validate()
        producer = self._device('ProducerControl')
        from .afs_statistics_frame import validate as validate_statistics
        statistics = validate_statistics(producer)
        if (statistics is None or statistics['state'] != 'acknowledged'
                or statistics['source_epoch'] != self.source_epoch()
                or statistics['ack_channel'] != 2 or statistics['ack_step'] != 0
                or statistics['ack_due_tick'] is not None
                or producer.rearm.get('rotation_count') != 6
                or len(producer.rearm.get('rotations', [])) != 6
                or producer.rearm['rotations'][-1]['state'] != 'completed'):
            raise UnsupportedDevice('Measurement input requires the second statistics ACK and completed rotation')
        if any(bank.enabled & (1 << 46) or bank.pending & (1 << 46)
               or bank.active == 46 or bank.injected == 46 for bank in self.board.interrupts.banks):
            raise UnsupportedDevice('Measurement input requires the actually retired statistics IRQ46')
        now = self.board.virtual_ticks
        if (type(now) is not int or not statistics['acknowledged_tick'] <= now < (1 << 64) - self.ticks
                or not self._measurement_gates(source) or not self._route(source)):
            raise UnsupportedDevice('Measurement input requires the live generation4 source0 route and drained IRQs')
        contract = self._measurement_contract()
        history = deepcopy(self.request['history'] + [self.request])
        identity = afs_startup_scope.identity(source.event_profile, profile)
        request = dict(profile=profile, generation=identity[1], ordinal=identity[2], attempt=identity[3], frame=identity[4],
                       cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                       requested_tick=now, due_tick=now + self.ticks, delay_ticks=self.ticks,
                       asserted_tick=None, acknowledged_tick=None, canceled_tick=None, reason=None,
                       state='waiting', prefix_sha256=_digest(source.event_history),
                       producer=self._producer_anchor(historical=True), write_cursor=len(source.writes),
                       history=history, history_sha256=_digest(history), measurement=contract,
                       measurement_sha256=_digest(contract))
        self.request = request
        self.board.record('AF-S post-retune measurement source0 edge explicitly armed', **request,
                          limits='One new measurement edge; previous two identities retained; no pixels or AF result')
        return deepcopy(request)

    def arm_measurement_frame(self, profile):
        """Offer the first pixel-bearing clock after the real producer enable."""
        if (profile != MEASUREMENT_FRAME_PROFILE or self.board.failure is not None
                or self.request is None or self.request.get('profile') != MEASUREMENT_PROFILE
                or self.request.get('state') != 'acknowledged'):
            raise UnsupportedDevice('Measurement frame requires its named profile and acknowledged initialization edge')
        source = self._validate()
        producer = self._device('ProducerControl')
        producer._validate_rearm()
        from .afs_statistics_frame import _route as statistics_route
        statistics_route(producer)
        anchor = self._measurement_producer_anchor()
        now = self.board.virtual_ticks
        if (type(now) is not int or not self.request['acknowledged_tick'] <= anchor['enabled_tick'] <= now
                or not self.request['asserted_tick'] < anchor['enabled_tick']
                or not now < (1 << 64) - self.ticks
                or not self._measurement_gates(source) or not self._route(source)):
            raise UnsupportedDevice('Measurement frame requires the completed post-edge enable and live drained source0 route')
        contract = self._measurement_contract(pixels=True)
        history = deepcopy(self.request['history'] + [self.request])
        identity = afs_startup_scope.identity(source.event_profile, profile)
        request = dict(profile=profile, generation=identity[1], ordinal=identity[2], attempt=identity[3], frame=identity[4],
                       cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                       requested_tick=now, due_tick=now + self.ticks, delay_ticks=self.ticks,
                       asserted_tick=None, acknowledged_tick=None, canceled_tick=None, reason=None,
                       state='waiting', prefix_sha256=_digest(source.event_history),
                       producer=self._producer_anchor(historical=True), write_cursor=len(source.writes),
                       history=history, history_sha256=_digest(history), measurement=contract,
                       measurement_sha256=_digest(contract), measurement_producer=anchor)
        self.request = request
        self.board.record('AF-S post-enable first measurement frame explicitly armed', **request,
                          limits='One post-enable edge; initialization is retained separately; no DMA or focus result')
        return deepcopy(request)

    def arm_next_measurement_frame(self, profile):
        """Offer only the next pixel clock after frame8's paired data ACK."""
        if (profile != MEASUREMENT_NEXT_FRAME_PROFILE or self.board.failure is not None
                or self.request is None or self.request.get('profile') != MEASUREMENT_FRAME_PROFILE
                or self.request.get('state') != 'acknowledged'):
            raise UnsupportedDevice('Next measurement frame requires its named profile and acknowledged first pixel input')
        source = self._validate()
        producer = self._device('ProducerControl')
        producer._validate_rearm()
        from .afs_statistics_frame import _route as statistics_route
        statistics_route(producer)
        data = self._measurement_data_anchor(self.request)
        rotation = producer.rearm.get('measurement', {}).get('rotation')
        if (type(rotation) is not dict or rotation.get('state') != 'completed'
                or rotation.get('step') != 9 or len(rotation.get('accesses', [])) != 9
                or rotation.get('source_epoch') != data['source_epoch']):
            raise UnsupportedDevice('Next measurement frame requires the complete frame8-owned bank rotation')
        now = self.board.virtual_ticks
        if (type(now) is not int or not max(self.request['acknowledged_tick'], data['acknowledged_tick']) <= now
                or not now < (1 << 64) - self.ticks
                or not self._measurement_gates(source) or not self._route(source)):
            raise UnsupportedDevice('Next measurement frame requires both prior ACKs and live drained source0/statistics routes')
        contract = self._measurement_contract(pixels=True, continuing=True)
        anchor = self._measurement_producer_anchor()
        if anchor != self.request['measurement_producer']:
            raise UnsupportedDevice('Next measurement frame must retain the same enabled producer activation')
        history = deepcopy(self.request['history'] + [self.request])
        identity = afs_startup_scope.identity(source.event_profile, profile)
        request = dict(profile=profile, generation=identity[1], ordinal=identity[2], attempt=identity[3], frame=identity[4],
                       cpu=0, source=0, physical_irq='0x27', ack_selector=1,
                       requested_tick=now, due_tick=now + self.ticks, delay_ticks=self.ticks,
                       asserted_tick=None, acknowledged_tick=None, canceled_tick=None, reason=None,
                       state='waiting', prefix_sha256=_digest(source.event_history),
                       producer=self._producer_anchor(historical=True), write_cursor=len(source.writes),
                       history=history, history_sha256=_digest(history), measurement=contract,
                       measurement_sha256=_digest(contract), measurement_producer=anchor,
                       measurement_previous_data=data, measurement_previous_data_sha256=_digest(data))
        self.request = request
        self.board.record('AF-S second measurement pixel frame explicitly armed', **request,
                          limits='One future source0 clock after actual paired data ACK and stock index progress; no DMA, result or next iris input')
        return deepcopy(request)

    def _validate(self):
        record = self.request
        if record is None:
            return None
        if type(record) is not dict:
            raise UnsupportedDevice('AF-S source request is not a plain record')
        terminal = record.get('state') == 'acknowledged'
        measurement = record.get('profile') in MEASUREMENT_PROFILES
        source = self._prefix(withdrawn=record.get('state') == 'canceled',
                              historical=terminal or measurement)
        self._validate_record(record, source, terminal=terminal, anchor_view={})
        if 'preview_return' in record:
            from . import afs_preview_return_clock
            afs_preview_return_clock.validate(self)
        if 'capture_initialization' in record:
            from . import afs_capture_clock
            afs_capture_clock.validate(self)
        if 'still_initialization' in record:
            from . import afs_still_initialization_clock
            afs_still_initialization_clock.validate(self)
        return source

    def _validate_record(self, record, source, *, historical=False, terminal=False, anchor_view=None):
        if anchor_view is None:
            anchor_view = {}
        second = record.get('profile') == CONTINUATION_PROFILE
        measurement = record.get('profile') == MEASUREMENT_PROFILE
        continuing = record.get('profile') == MEASUREMENT_NEXT_FRAME_PROFILE
        pixels = record.get('profile') in MEASUREMENT_PIXEL_PROFILES
        identity = afs_startup_scope.identity(source.event_profile, record.get('profile'))
        if measurement or pixels:
            history = record.get('history')
            profiles = ((PROFILE, CONTINUATION_PROFILE, MEASUREMENT_PROFILE, MEASUREMENT_FRAME_PROFILE)
                        if continuing else (PROFILE, CONTINUATION_PROFILE, MEASUREMENT_PROFILE)
                        if pixels else (PROFILE, CONTINUATION_PROFILE))
            if (pixels and historical and (not terminal or record.get('state') != 'acknowledged')
                    or type(history) is not list or len(history) != len(profiles)
                    or any(type(item) is not dict or item.get('state') != 'acknowledged' for item in history)
                    or tuple(item.get('profile') for item in history) != profiles
                    or record.get('history_sha256') != _digest(history)
                    or any(history[index].get('history') != history[:index]
                           for index in range(1, len(history)))):
                raise UnsupportedDevice('Measurement input lacks its immutable closed source lifetimes')
            for item in history:
                self._validate_record(item, source, historical=True, terminal=True, anchor_view=anchor_view)
            if not history[-1]['acknowledged_tick'] <= record.get('requested_tick', -1):
                raise UnsupportedDevice('Measurement input precedes the second source acknowledgment')
            contract = record.get('measurement')
            if (type(contract) is not dict or record.get('measurement_sha256') != _digest(contract)
                    or contract.get('request_pointer') != 0x15e77c8
                    or contract.get('callback') != 0x12421d0
                    or contract.get('descriptor_pointer') != 0x1613ab4
                    or contract.get('descriptor_count') != 3
                    or contract.get('slots_sha256') != hashlib.sha256(
                        MEASUREMENT_ACTIVE_SLOTS if pixels else MEASUREMENT_SLOTS).hexdigest()
                    or any(type(contract.get(key)) is not int or contract[key] <= 0
                           for key in ('producer_handle', 'measurement_handle'))
                    or contract['producer_handle'] == contract['measurement_handle']
                    or type(contract.get('subscribers_sha256')) is not str
                    or len(contract['subscribers_sha256']) != 64):
                raise UnsupportedDevice('Measurement input request/subscriber identity changed')
            if pixels:
                anchor = self._measurement_producer_anchor(expected=record.get('measurement_producer'), view=anchor_view)
                if (record.get('measurement_producer') != anchor
                        or not history[2]['asserted_tick'] < anchor['enabled_tick']
                        or not history[2]['acknowledged_tick'] <= anchor['enabled_tick'] <= record.get('requested_tick', -1)):
                    raise UnsupportedDevice('Measurement frame lost its immutable post-edge producer activation')
            if continuing:
                data = self._measurement_data_anchor(history[-1])
                if (record.get('measurement_previous_data') != data
                        or record.get('measurement_previous_data_sha256') != _digest(data)
                        or not data['acknowledged_tick'] <= record.get('requested_tick', -1)):
                    raise UnsupportedDevice('Next measurement frame lost its immutable completed first data')
            if record.get('state') == 'waiting' and contract != self._measurement_contract(pixels=pixels, continuing=continuing):
                raise UnsupportedDevice('Measurement request changed before its external edge')
        elif second:
            history = record.get('history')
            if (type(history) is not list or len(history) != 1
                    or type(history[0]) is not dict or history[0].get('profile') != PROFILE
                    or history[0].get('state') != 'acknowledged'
                    or record.get('history_sha256') != _digest(history)):
                raise UnsupportedDevice('Second AF-S input lacks its immutable closed first lifetime')
            self._validate_record(history[0], source, historical=True, terminal=terminal, anchor_view=anchor_view)
            if not history[0]['acknowledged_tick'] <= record.get('requested_tick', -1):
                raise UnsupportedDevice('Second AF-S input precedes the first source acknowledgment')
        elif 'history' in record or 'history_sha256' in record:
            raise UnsupportedDevice('First AF-S input must not claim another source lifetime')
        if (self.profile != PROFILE or type(record) is not dict
                or any(type(record.get(key)) is not int for key in
                       ('generation', 'ordinal', 'attempt', 'frame', 'cpu', 'source',
                        'ack_selector', 'due_tick', 'delay_ticks'))
                or tuple(record.get(key) for key in ('profile', 'generation', 'ordinal', 'attempt',
                    'frame', 'cpu', 'source', 'physical_irq', 'ack_selector')) !=
                    identity
                or record.get('prefix_sha256') != _digest(source.event_history)
                or record.get('producer') != self._producer_anchor(
                    historical=terminal or historical or measurement or pixels,
                    expected=record.get('producer') if terminal or historical or measurement or pixels else None,
                    view=anchor_view)
                or type(record.get('requested_tick')) is not int
                or not 0 <= record['requested_tick'] <= self.board.virtual_ticks
                or record.get('delay_ticks') != self.ticks
                or record.get('due_tick') != record['requested_tick'] + self.ticks
                or type(record.get('write_cursor')) is not int
                or not 0 <= record['write_cursor'] <= len(source.writes)
                or record.get('state') not in ('waiting', 'asserted', 'acknowledged', 'canceled')):
            raise UnsupportedDevice('AF-S source identity, prefix, producer or timer changed')
        if record['state'] in ('asserted', 'acknowledged'):
            if type(record.get('asserted_tick')) is not int or not record['due_tick'] <= record['asserted_tick'] <= self.board.virtual_ticks:
                raise UnsupportedDevice('AF-S source assertion time is inconsistent')
        elif record.get('asserted_tick') is not None:
            raise UnsupportedDevice('AF-S source claims an unobserved assertion')
        if record['state'] == 'acknowledged':
            if type(record.get('acknowledged_tick')) is not int or not record['asserted_tick'] <= record['acknowledged_tick'] <= self.board.virtual_ticks:
                raise UnsupportedDevice('AF-S source acknowledgment time is inconsistent')
        elif record.get('acknowledged_tick') is not None:
            raise UnsupportedDevice('AF-S source claims an unobserved acknowledgment')
        if record['state'] == 'canceled':
            if type(record.get('canceled_tick')) is not int or not record['requested_tick'] <= record['canceled_tick'] <= self.board.virtual_ticks or not record.get('reason'):
                raise UnsupportedDevice('AF-S source cancellation time is inconsistent')
        elif record.get('canceled_tick') is not None or record.get('reason') is not None:
            raise UnsupportedDevice('AF-S source claims an unobserved cancellation')

    def _cancel(self, reason):
        self.request.update(state='canceled', canceled_tick=self.board.virtual_ticks, reason=reason)
        self.board.record('AF-S external source0 edge canceled', **self.request,
                          effects='No IRQ, replacement edge, DMA or result')

    def tick(self, board):
        if board is not self.board:
            raise UnsupportedDevice('AF-S source tick belongs to a different board')
        if type(self.request) is dict and 'preview_return' in self.request:
            from . import afs_preview_return_clock
            afs_preview_return_clock.tick(self, board)
            if board.failure is not None:
                return
        if type(self.request) is dict and 'capture_initialization' in self.request:
            from . import afs_capture_clock
            afs_capture_clock.tick(self, board)
            if board.failure is not None:
                return
        if type(self.request) is dict and 'still_initialization' in self.request:
            from . import afs_still_initialization_clock
            afs_still_initialization_clock.tick(self, board)
            if board.failure is not None:
                return
        if type(self.request) is dict and 'af_stream' in self.request:
            from . import afs_contrast_clock
            afs_contrast_clock.tick(self, board)
            if board.failure is not None:
                return
        if self.request is None:
            return
        # Closed inputs own no timer or output. Validate their retained
        # identity when an arm, epoch/history read or snapshot consumes it,
        # instead of walking immutable history on every idle scheduler tick.
        if type(self.request) is dict and self.request.get('state') in ('acknowledged', 'canceled'):
            return
        try:
            record = self.request
            if type(record) is not dict or record.get('state') not in ('waiting', 'asserted'):
                raise UnsupportedDevice('AF-S active source request is not a known plain phase')
            current = self._device('InputProcessingControl')
            if record['state'] == 'waiting':
                gates = (self._measurement_gates(current) if self.request['profile'] in
                         MEASUREMENT_PROFILES else self._gates(current))
                if not gates:
                    self._cancel('Input engine/source gates changed before the edge')
                    return
                if (type(record.get('due_tick')) is not int or type(record.get('requested_tick')) is not int
                        or record.get('delay_ticks') != self.ticks
                        or record['due_tick'] != record['requested_tick'] + self.ticks):
                    raise UnsupportedDevice('AF-S source delay lost its known integer clock')
                if board.virtual_ticks < record['due_tick']:
                    return
            else:
                cursor = record.get('write_cursor')
                if type(cursor) is not int or not 0 <= cursor <= len(current.writes):
                    raise UnsupportedDevice('AF-S source ACK cursor escaped its actual input writes')
                if not any(item['address'] == hex(current.BASE + 0x388) and item['value'] == '0x1'
                           for item in current.writes[cursor:]):
                    record['write_cursor'] = len(current.writes)
                    return
            # The source has no output before its due edge. Full immutable
            # history validation belongs to assertion/ACK consumption and
            # public epoch, history, snapshot and admission consumers.
            source = self._validate()
            if record['state'] == 'waiting':
                if not self._route(source):
                    self._cancel('Unique exact source0 route unavailable at the due edge')
                    return
                if record['profile'] in MEASUREMENT_PROFILES:
                    self._measurement_contract(pixels=record['profile'] in MEASUREMENT_PIXEL_PROFILES,
                                               continuing=record['profile'] == MEASUREMENT_NEXT_FRAME_PROFILE)
                else:
                    self._producer_anchor(live=True)
                record.update(state='asserted', asserted_tick=board.virtual_ticks,
                              write_cursor=len(source.writes))
                board.queue_irq(0, IRQ)
                board.record('AF-S external source0 edge asserted', **record,
                             effects='Architectural IRQ27 only; stock firmware owns subscribers/ACK; no payload')
            elif record['state'] == 'asserted':
                for item in source.writes[record['write_cursor']:]:
                    if item['address'] != hex(source.BASE + 0x388) or item['value'] != '0x1':
                        continue
                    if type(item['cpu']) is not int or item['cpu'] != 0:
                        raise UnsupportedDevice('AF-S source acknowledgment came from a different owner')
                    record.update(state='acknowledged', acknowledged_tick=board.virtual_ticks)
                    board.record('AF-S external source0 edge acknowledged', **record,
                                 limits='Actual owner selector1 write observed at device tick; controller EOI remains kernel-owned')
                    break
                record['write_cursor'] = len(source.writes)
        except UnsupportedDevice as error:
            board.failure = dict(message=str(error), cpu=0, device='AfsFrameSource', operation='source0-edge')

    def next_event_tick(self, now):
        values = []
        if type(self.request) is dict and 'preview_return' in self.request:
            from . import afs_preview_return_clock
            tick = afs_preview_return_clock.due(self)
            if tick is not None:
                values.append(tick)
        if type(self.request) is dict and 'capture_initialization' in self.request:
            from . import afs_capture_clock
            tick = afs_capture_clock.due(self)
            if tick is not None:
                values.append(tick)
        if type(self.request) is dict and 'still_initialization' in self.request:
            from . import afs_still_initialization_clock
            tick = afs_still_initialization_clock.due(self)
            if tick is not None:
                values.append(tick)
        if self.request is not None and self.request['state'] == 'waiting':
            values.append(self.request['due_tick'])
        if type(self.request) is dict and 'af_stream' in self.request:
            from . import afs_contrast_clock
            tick = afs_contrast_clock.due(self)
            if tick is not None:
                values.append(tick)
        return min(values) if values else None

    def source_epoch(self):
        self._validate()
        if self.request is None or self.request['state'] not in ('asserted', 'acknowledged'):
            return None
        return {key: self.request[key] for key in ('profile', 'generation', 'ordinal', 'attempt',
                    'frame', 'cpu', 'source', 'physical_irq', 'asserted_tick')}

    def snapshot(self):
        self._validate()
        if self.request is not None and 'af_stream' in self.request:
            af_requests(self.board)
        return dict(profile=self.profile, ticks=self.ticks, request=deepcopy(self.request),
                    limits='Two shooting, one measurement-init and two post-enable source0 inputs; no sensor pixels, AF result, cadence or physical timing')
