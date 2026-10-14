"""FFF11000 stock input-adjacent control bank; engine unverified.

The recovered driver writes scalar, selectors and four eight-word tables.
These are configuration latches only. Two status words are explicit absent
event inputs; the exact D0000 acknowledgment-looking writes are recorded
separately. An independent opt-in models only the observed control3->2
request's delayed bit0 acknowledgment. Its physical meaning and latency are
unmeasured; it is not an image result, reset, memory transfer or IRQ.
"""
import hashlib
import struct

from .hardware import UnsupportedDevice


CONTROL_PROFILE = 'stock-control3-bit0-clear-ack-unmeasured'
CONTROL_PROFILES = (CONTROL_PROFILE,)


class InputAuxiliaryControl:
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASE = 0xfff11000
    # Full-word stores in92A04C/92A0E8/92A184/92A220. Their contents are
    # opaque configuration, not source/destination addresses or pixel data.
    TABLE_OFFSETS = frozenset(range(0x08, 0x48, 4)) | frozenset(range(0x50, 0x90, 4))
    MASKS = {**{offset: 0xffffffff for offset in TABLE_OFFSETS},
             0x04: 0x31bf33ff, 0x48: 0xffffffff, 0x4c: 0xffffffff,
             0x90: 0xffffffff, 0x94: 0xffffffff,
             0x98: 0x77777777, 0x9c: 0x00007777,
             0xa8: 0x000d000d, 0xac: 0x00003fff,
             0xb0: 0x77777777, 0xb4: 0x00777777}
    READABLE = frozenset((0x00, 0x98, 0x9c, 0xa0, 0xa4, 0xa8, 0xac))

    def __init__(self, board, control_profile=None, control_ticks=2):
        if control_profile is not None and control_profile not in CONTROL_PROFILES:
            raise ValueError('Unknown auxiliary control profile')
        if isinstance(control_ticks, bool) or not isinstance(control_ticks, int) or control_ticks <= 0:
            raise ValueError('Auxiliary control delay must be a positive integer')
        self.board = board
        self.control_profile = control_profile
        self.control_ticks = control_ticks
        self.busy = False
        self.pending_control = None
        self.last_control = None
        self.control_requests = 0
        self.control_completions = 0
        self.registers = {offset: 0 for offset in (0x00, *self.MASKS)}
        self.event_status = {0xa0: 0, 0xa4: 0}
        self.acknowledgments = {0xa0: 0, 0xa4: 0}
        self.writes = []
        self.last_rejected = None
        board.record('Input auxiliary control fixture', base=hex(self.BASE),
            provenance='929F38..92A68F driver;1208404 immutable-table configuration;120843C scalar;120848C guest control',
            control_profile=control_profile, control_ticks=control_ticks,
            limits='Unmeasured zero configuration/absent event status; optional exact3->2 delayed bit0 ack has unknown physical meaning; no images/DMA/IRQ')

    def contains(self, address, size):
        return self.BASE <= address < self.BASE + 0x100

    def _reject(self, operation, core, address, size, value=None,
                reason='Unknown input auxiliary operation'):
        self.last_rejected = dict(operation=operation, cpu=core, address=hex(address), size=size,
            value=None if value is None else hex(value), reason=reason)
        self.board.record('Input auxiliary request rejected', **self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    def _offset(self, operation, core, address, size, value=None):
        offset = address - self.BASE
        if size != 4 or offset not in (0x00, 0xa0, 0xa4) and offset not in self.MASKS:
            self._reject(operation, core, address, size, value)
        return offset

    def read(self, core, address, size):
        offset = self._offset('read', core, address, size)
        if offset not in self.READABLE:
            self._reject('read', core, address, size, reason='No proven readable auxiliary configuration field')
        if offset in self.event_status:
            if self.event_status[offset] != 0:
                self._reject('read', core, address, size, reason='Unmodeled auxiliary event status')
            return 0
        return self.registers[offset]

    def write(self, core, address, size, value):
        offset = self._offset('write', core, address, size, value)
        if not isinstance(value, int) or not 0 <= value <= 0xffffffff:
            self._reject('write', core, address, size, value, reason='Auxiliary word exceeds32 bits')
        if self.busy:
            self._reject('write', core, address, size, value,
                reason='Auxiliary configuration mutation during pending control acknowledgment')
        request = None
        if offset in self.event_status:
            if value != 0xd0000:
                self._reject('write', core, address, size, value,
                    reason='Only exact stock auxiliary acknowledgment-looking strobe is supported')
            old = self.event_status[offset]
            self.acknowledgments[offset] += 1
            # This chosen absent input remains zero. Hardware event clearing
            # polarity/meaning cannot be proved from this single stock write.
        else:
            old = self.registers[offset]
            if offset == 0x00:
                if value not in (0, 1, 2, 3, 0x10, 0x11, 0x13):
                    self._reject('write', core, address, size, value, reason='Unknown auxiliary control command')
                if old & 1 and not value & 1:
                    if self.control_profile != CONTROL_PROFILE:
                        self._reject('write', core, address, size, value,
                            reason='Auxiliary clear-bit0/poll-bit0 response is not modeled')
                    if old != 3 or value != 2 or any(self.event_status.values()):
                        self._reject('write', core, address, size, value,
                            reason='Only exact auxiliary control3->2 with absent events is modeled')
                    request = dict(cpu=core, profile=self.control_profile, request=hex(value),
                        old=hex(old), requested_tick=self.board.virtual_ticks,
                        due_tick=self.board.virtual_ticks+self.control_ticks,
                        config_sha256=self._configuration_hash(), state='waiting')
                # Positive bit1/bit0 is a guest-written control only. It has
                # no modeled engine start, completion or output operation.
            elif value & ~self.MASKS[offset]:
                self._reject('write', core, address, size, value, reason='Unknown auxiliary configuration bits')
            elif offset == 0x04 and value & 0x31bf0000 and not value & 0x200:
                self._reject('write', core, address, size, value,
                    reason='Auxiliary extended fields require stock bit9 selector')
            self.registers[offset] = value
        event = dict(cpu=core, address=hex(address), value=hex(value), old=hex(old))
        self.writes.append(event)
        self.board.record('Input auxiliary configuration write', **event)
        if request is not None:
            self.busy = True
            self.pending_control = request
            self.last_control = dict(request)
            self.control_requests += 1
            self.board.record('Input auxiliary control acknowledgment requested (unmeasured)',
                **request, inference='Only control3->2 then delayed bit0 set; physical meaning/latency unknown, no image/DMA/IRQ')

    def _configuration_hash(self):
        return hashlib.sha256(b''.join(struct.pack('<II', offset, value)
            for offset, value in sorted(self.registers.items()) if offset != 0)).hexdigest()

    def tick(self, board):
        if not self.busy:
            return
        request = self.pending_control
        if (request is None or self.registers[0] != 2
                or self._configuration_hash() != request['config_sha256']
                or any(self.event_status.values())):
            self._reject('tick', None, self.BASE, 4,
                reason='Auxiliary pending control/configuration changed outside exact request')
        if board.virtual_ticks < request['due_tick']:
            return
        # The explicitly selected external handshake alters only bit0 of the
        # retained exact2 word. No processing engine or data result is implied.
        self.registers[0] |= 1
        self.busy = False
        self.pending_control = None
        self.control_completions += 1
        self.last_control = dict(request, state='acknowledged',
            completed_tick=board.virtual_ticks, acknowledged=hex(self.registers[0]))
        board.record('Input auxiliary control acknowledgment (unmeasured)', **self.last_control,
            inference='Delayed bit0 reassertion only; neither reset nor image completion is established')

    def next_event_tick(self, now):
        return self.pending_control['due_tick'] if self.pending_control is not None else None

    def snapshot(self):
        return dict(name=type(self).__name__, base=hex(self.BASE),
            registers={hex(offset):hex(value) for offset,value in self.registers.items()},
            event_status={hex(offset):hex(value) for offset,value in self.event_status.items()},
            acknowledgments={hex(offset):count for offset,count in self.acknowledgments.items()},
            control_profile=self.control_profile, control_ticks=self.control_ticks,
            busy=self.busy, pending_control=self.pending_control, last_control=self.last_control,
            control_requests=self.control_requests, control_completions=self.control_completions,
            writes=self.writes, last_rejected=self.last_rejected,
            limits=['Configuration reset words and absent event inputs are explicit unmeasured zero fixtures.',
                    'Exact D0000 writes are recorded separately from readable absent event status; no clearing polarity or event arrival is inferred.',
                    'Positive control stores retain guest bits only; no pixel output, processing, RAM transfer, IRQ or callback is generated.',
                    'Default profile rejects clear-bit0; separate opt-in accepts only exact3->2 with absent events and delays bit0 reassertion by unmeasured ticks.',
                    'Pending acknowledgment rejects all guest stores/configuration mutation and refuses checkpoint capture through busy; completed state restores normally.',
                    'Unknown widths, registers, bits, other control transitions and physical status meanings reject.',
                    'Four eight-word configuration tables remain opaque; physical engine purpose and behavior are unknown.'])
