"""Strict stock producer contracts and an explicit recorded-input experiment.

933258 clears FFF28000 then waits for bit28 SET. The optional external
acknowledgment and its delay are unmeasured. 92CC5C's two already-idle DMA
stops use explicit zero fixtures. 10CC06C's proven geometry/configuration
fields retain guest words only. 10CB618 maskD4 retains its bounded coordinate
list/geometry/LUT configuration, and 10CB5B0 maskD4 records desired global
unit modes without processing. 10CAEA0's channel0 destination/configuration
and arm sequence retains an armed request awaiting input. It produces no
DMA payload, completion/status transition, IRQ, metadata or pixels by default.
92D5F0/92DB08 and92DC54/92DD30 retain packed parameters160..190 only;
their initial zero is explicitly unmeasured and supplies no readiness.
92E574/92E630 likewise retain mode308 and ten-bit parameter310 only.
The separately selected foreign-mode recorded input supplies bounded bytes,
one completion IRQ, paired acknowledgment and guest release; it is not an ISP.
A completed release/shutdown may retain one separately owned fresh descriptor,
seven arm latches and two desired-enable words, awaiting new input. The first
job's history persists and its recorded payload cannot be replayed by this arm.
That fresh enabled request may retain one15C-anchored, six-word geometry update;
only its current configuration hashes change, with no processing/completion.
After that update it may retain at most three ordered nine-word destination
rotations. The separate late-display input profile may explicitly select four
retained groups; it still supplies no additional data result. These are desired tuples only: historical arm/data ownership and
count524 persist, while hardware bank-latching timing remains unmeasured.
After a complete rotation, one ordered GLOBAL4/8/C image configuration may
retain three desired words. A separate prior/current ledger preserves the
geometry and earlier rotation hashes; later rotations use the updated words.
This does not change the historical arm, data result or desired global modes.
After it, a bounded general ledger retains known desired parameter/list/LUT
stores and single-family global control fields. Older hash views undo stores
inserted after their rotation; future rotations use current configuration.
Before that image triplet, one complete10CB9B4 parameter family may retain
fourteen ordered desired stores. Its separate ledger preserves the earlier
geometry/rotation history and supplies no processing, result or interrupt.
The same pre-image window may then retain one10CC110 mode pair and one
10CBB44(mask1F0) geometry family, preserving the original fourteen stores.
A bounded subsequent phase retains already audited passive configuration setters
until the first image triplet seals it. No global control, destination, count,
status, data or interrupt access is admitted by that phase.
"""
import hashlib
import struct
import ctypes
import json
from dataclasses import asdict

import weakref

from . import strict_audit
from .hardware import UnsupportedDevice
from .histogram_devices import HistogramMixin,HISTOGRAM_PROFILES
from .producer_shutdown import FreshProducerShutdownMixin
from .producer_preview import PreviewProducerStopMixin
from . import histogram_protocol as hp
from . import preview_statistics
from . import afs_statistics_frame
from . import producer_measurement
from . import producer_stream
from . import producer_afs_focus
from . import producer_capture_metstats
from . import producer_still_capture


STOP_PROFILE = 'stock-idle-stop-handshakes-unmeasured'
STOP_PROFILES = (STOP_PROFILE,)
DATA_PROFILE = 'captured-rec28-output-under-startup50-experiment'
DATA_PROFILES = (DATA_PROFILE, *producer_stream.ProducerStreamProfile)


# Device -> {kind: (register-word key, hash)}; outside device state, so
# checkpoints and fingerprints are unchanged.
_hash_memo = weakref.WeakKeyDictionary()
_mask_orders = {}


def _pairs_sha256(registers, addresses):
    """sha256 of each (address, word) as two little-endian u32 words, in order."""
    pairs = [0] * (2 * len(addresses))
    pairs[0::2] = addresses
    pairs[1::2] = [registers[address] for address in addresses]
    return hashlib.sha256(struct.pack(f'<{len(pairs)}I', *pairs)).hexdigest()


def _sorted_masks(masks):
    """Sorted addresses of a constant class mask table, computed once per table."""
    order = _mask_orders.get(id(masks))
    if order is None or order[0] is not masks:
        order = _mask_orders[id(masks)] = (masks, tuple(sorted(masks)))
    return order[1]


class _ConfigurationSnapshot:
    """Typed packed words local to one validation; never retained on a device.

    Historical overlays patch copies of the same address/WORD encoding used
    by the original hashes. New operations build new snapshots, so direct
    register and ledger mutations remain observable rather than trusted.
    """
    def __init__(self, owner):
        addresses = sorted(owner.ALL_CONFIG_MASKS)
        full = bytearray(8 * len(addresses))
        self.full_offsets = {}
        offsets = {}
        for index, address in enumerate(addresses):
            value = owner.registers[address]
            if (not owner._config_valid(address, value)
                    and not (address == owner.BASE + 0x524 and value == 0
                             and type(value) is int and owner.dma_setup_step == 0)):
                raise UnsupportedDevice('Producer retained configuration has unknown word type/bits/domain')
            offset = index * 8
            struct.pack_into('<II', full, offset, address, value)
            offsets[address] = offset
            self.full_offsets[hex(address)] = offset + 4
        self.full = bytes(full)
        legacy = bytearray(8 * len(owner.CONFIG_MASKS))
        self.legacy_offsets = {}
        for index, address in enumerate(sorted(owner.CONFIG_MASKS)):
            offset = index * 8
            source = offsets[address]
            legacy[offset:offset + 8] = full[source:source + 8]
            self.legacy_offsets[hex(address)] = offset + 4
        self.legacy = bytes(legacy)

    def hashes(self, overlay=None):
        if overlay:
            full, legacy = bytearray(self.full), bytearray(self.legacy)
            for address, value in overlay.items():
                offset = self.full_offsets.get(address)
                if offset is not None:
                    struct.pack_into('<I', full, offset, value)
                offset = self.legacy_offsets.get(address)
                if offset is not None:
                    struct.pack_into('<I', legacy, offset, value)
        else:
            full, legacy = self.full, self.legacy
        return hashlib.sha256(legacy).hexdigest(), hashlib.sha256(full).hexdigest()


def _configuration_snapshot(owner):
    """The device's current _ConfigurationSnapshot, reused while its words are unchanged.

    A snapshot is a pure function of the configuration words and the DMA setup
    step (the 0x524 validity rule), so a snapshot built from equal inputs is
    equal; the current words are still read on every call. Its hashes are
    memoized per overlay. X70_EMULATOR_AUDIT=1 builds a new snapshot each time.
    """
    if strict_audit.ENABLED:
        return _ConfigurationSnapshot(owner)
    registers = owner.registers
    key = (tuple([registers[address] for address in _sorted_masks(owner.ALL_CONFIG_MASKS)]),
           owner.dma_setup_step)
    memo = _hash_memo.get(owner)
    if memo is None:
        memo = _hash_memo[owner] = {}
    hit = memo.get('snapshot')
    if hit is not None and hit[0] == key:
        return hit[1]
    snapshot = _MemoizedSnapshot(owner)
    memo['snapshot'] = (key, snapshot)
    return snapshot


class _MemoizedSnapshot(_ConfigurationSnapshot):
    """A snapshot whose hashes (immutable inputs) are computed once per overlay."""
    def __init__(self, owner):
        super().__init__(owner)
        self._hashes = {}

    def hashes(self, overlay=None):
        key = tuple(sorted(overlay.items())) if overlay else ()
        result = self._hashes.get(key)
        if result is None:
            result = self._hashes[key] = super().hashes(overlay)
        return result


class ProducerControl(PreviewProducerStopMixin,FreshProducerShutdownMixin,HistogramMixin):
    # Ticks at quiet quantum boundaries only advance elapsed-time bookkeeping
    # (emulator.board.Board.flush_deferred_ticks).
    quiet_ticks_defer = True
    BASE = 0xfff18000
    GLOBAL = 0xfff28000
    STOP_CLEAR_MASK = 0x1f07333f
    GLOBAL_DESIRED_MASK = 0x0007330c
    GLOBAL_DESIRED_FIELDS = (0x0000000c,0x00000300,0x00003000,0x00070000)
    IDLE_REGISTERS = (BASE, BASE+0x880, BASE+0x884, BASE+0x890,
                      BASE+0x980, BASE+0x984, BASE+0x990)
    DESTINATIONS = tuple(range(BASE+0x500,BASE+0x524,4))
    DMA_SETUP = ((BASE+0x88c,0), (BASE+0x8f4,1),
                 (BASE+0x8a0,0x80003000), (BASE+0x8f8,0),
                 (BASE+0x8c4,2), (BASE+0x8f8,0x10000000),
                 (BASE+0x884,1))
    DMA_REGISTERS = frozenset(a for a,_ in DMA_SETUP)
    RELEASED_SHUTDOWN = (('write',BASE+0x884,0),('read',BASE+0x890,0),
        ('write',BASE+0x984,0),('read',BASE+0x990,0),
        ('read',BASE+0x880,0),('write',BASE+0x880,0),
        ('read',BASE+0x980,0),('write',BASE+0x980,0),
        ('read',BASE,0),('write',BASE,0))
    ACK_REGISTERS = frozenset((BASE+0x88c,BASE+0x8f4,BASE+0x98c,BASE+0x9f4))
    PRODUCER_SELECT_MASK = 0x33173317
    PRODUCER_DISABLED_MASK = 0x33170000
    REARM_DESCRIPTOR_ORDER = tuple(0xfff18000+o for o in
        (0x508,0x500,0x510,0x514,0x518,0x51c,0x520,0x50c,0x504,0x524))
    REARM_ENABLE = ((BASE+4,PRODUCER_SELECT_MASK),(BASE+8,0))
    REARM_GEOMETRY_ORDER = tuple(0xfff18000+o for o in
        (0x100,0x104,0x108,0x300,0x304,0x30c))
    #11F6008 ->10CC14C(mask3F) ->10CA67C does not write count524.
    #Three groups bound this retention experiment, not measured input cadence.
    REARM_ROTATION_ORDER = tuple(0xfff18000+o for o in
        (0x508,0x500,0x510,0x514,0x518,0x51c,0x520,0x50c,0x504))
    REARM_ROTATION_LIMIT = 3
    IMAGE_GLOBAL_CONFIG_ORDER = (GLOBAL+4,GLOBAL+8,GLOBAL+0xc)
    PRE_IMAGE_PARAMETER_ORDER = tuple(0xfff18000+o for o in
        (0x15c,0x160,0x178,0x184,0x16c,0x164,0x170,0x17c,0x188,
         0x168,0x174,0x180,0x18c,0x190))
    PRE_IMAGE_FOLLOWUP_ORDERS = (
        ('mode',tuple(0xfff18000+o for o in (0x308,0x310))),
        ('geometry1f0',tuple(0xfff18000+o for o in (
            0x200,0x204,0x258,*range(0x208,0x258,4),0x100,0x104,0x108,
            *range(0x10c,0x15c,4),0x300,0x304,0x30c))))
    REARM_KNOWN_CONFIG_LIMIT = 8192
    CONFIG_MASKS = {GLOBAL+4:0x1fff1fff, GLOBAL+8:0x1fff0000,
                    GLOBAL+0xc:0x00ff007f,
                    BASE+0xc:0x333f, BASE+0x600:3, BASE+0x418:1,
                    #10CBB44's mask7F0 family: packed parameter words and
                    # two ten-entry geometry arrays.92CA08/92C94C perform
                    # real software readback conversions using15C.
                    BASE+0x100:0x1ffff1ff, BASE+0x104:0x1ffff1ff,
                    BASE+0x108:0x01ff01ff, BASE+0x15c:0x1f07073f,
                    **{0xfff18000+o:0x1fff1fff for o in range(0x10c,0x15c,4)},
                    BASE+0x200:0x1ffff1ff, BASE+0x204:0x1ffff1ff,
                    BASE+0x258:0x01ff01ff,
                    **{0xfff18000+0x208+i*8:0x3fff1fff for i in range(10)},
                    **{0xfff18000+0x20c+i*8:0x1fff1fff for i in range(10)},
                    BASE+0x300:0x1ffff1ff, BASE+0x304:0x1ffff1ff,
                    BASE+0x30c:0x01ff01ff,
                    BASE+0x400:0x3fff1fff, BASE+0x404:0x1fff1fff,
                    BASE+0x408:0x3ffff1ff, BASE+0x40c:0x1fff1fff,
                    BASE+0x410:0x3fff1fff, BASE+0x414:0x1ffff1ff,
                    BASE+0x604:0x1fff1fff, BASE+0x608:0x1fff1fff,
                    #10CA67C copies arbitrary32-bit destination words;
                    # validity/length checks belong to a future data job.
                    **{0xfff18000+o:0xffffffff for o in range(0x500,0x524,4)},
                    BASE+0x524:0x3ff,
                    #10CB618 maskD4 only; no getter-only58/5C output.
                    GLOBAL+0x1c:1,
                    GLOBAL+0x40:0x1fff1fff, GLOBAL+0x44:0x1fff1fff,
                    GLOBAL+0x48:0xfffff, GLOBAL+0x4c:0x3fff,
                    GLOBAL+0x50:0x1fff1fff, GLOBAL+0x54:0x1fff1fff,
                    GLOBAL+0x74:0x00070001,
                    GLOBAL+0x78:0x1fff1fff, GLOBAL+0x7c:0x1fff1fff,
                    GLOBAL+0x80:0x07ff07ff, GLOBAL+0x84:0x3f3f1fff,
                    **{0xfff28000+o:0x3fff3fff for o in range(0x88,0xf8,4)},
                    #933DC0 clipping-list encoding.93464C's separate
                    # unmasked1100..19FC LUT window remains unresolved.
                    **{0xfff28000+o:0x9fffdfff for o in range(0x100,0x1100,4)},
                    **{0xfff28000+o:0x3fffffff for o in range(0x1c00,0x3000,4)},
                    **{0xfff28000+o:0xfffff for o in range(0x3000,0x3400,4)}}
    #92D5F0/92DB08 and92DC54/92DD30 pack/unpack full retained
    # parameter words. These masks describe stock software construction;
    # untouched zeros are an unmeasured fixture, not ready/status values.
    PARAMETER_CONFIG_MASKS = {BASE+0x160:0x013f0fff, BASE+0x178:0x013f0fff,
        BASE+0x184:0x013f0fff, BASE+0x16c:0x00000fff,
        **{0xfff18000+o:0x0fff0fff for o in
           (0x164,0x168,0x170,0x174,0x17c,0x180,0x188,0x18c)},
        BASE+0x190:0x003f3f3f}
    #92E574 accepts mode0..5 and an unsigned halfword below400;
    #92E630 reads the same packed fields. No hardware action is inferred.
    MODE_CONFIG_MASKS = {BASE+0x308:0x7, BASE+0x310:0x3ff}
    #93464C assembles raw LE WORDs from four bytes: nine64-word
    #write-only lookup windows, with no processing or completion effect.
    RAW_LUT_CONFIG_MASKS = {0xfff28000+o:0xffffffff for o in range(0x1100,0x1a00,4)}
    #934384 Boolean and93444C's packed parameters accompany the raw LUT
    #configuration; kept outside the first fixture's legacy hash domain.
    EXTRA_GLOBAL_CONFIG_MASKS = {GLOBAL+0x60:1,GLOBAL+0x64:0x1fff1fff,
        GLOBAL+0x68:0x003f003f,GLOBAL+0x6c:0x07ff07ff,GLOBAL+0x70:0x3fff}
    #CONFIG_MASKS and_config_hash retain the original pinned b59... domain.
    #Transaction integrity covers both domains through the retained hash.
    ALL_CONFIG_MASKS = {**CONFIG_MASKS, **PARAMETER_CONFIG_MASKS, **MODE_CONFIG_MASKS,
                        **RAW_LUT_CONFIG_MASKS,**EXTRA_GLOBAL_CONFIG_MASKS}
    KNOWN_DESIRED_CONFIG = (frozenset(ALL_CONFIG_MASKS)|{GLOBAL})-frozenset((
        *DESTINATIONS,BASE+0x524))
    PRE_IMAGE_DESIRED_CONFIG = KNOWN_DESIRED_CONFIG-frozenset((GLOBAL,*IMAGE_GLOBAL_CONFIG_ORDER))
    WRITE_ONLY_CONFIG = frozenset((*RAW_LUT_CONFIG_MASKS,*range(0xfff29c00,0xfff2b400,4)))
    READABLE = frozenset((BASE, BASE+4, BASE+8, BASE+0x880, BASE+0x884, BASE+0x890,
                          BASE+0x980, BASE+0x990, GLOBAL,
                          BASE+0x88c,BASE+0x8f4,
                          *ALL_CONFIG_MASKS)) - WRITE_ONLY_CONFIG
    WRITABLE = frozenset((BASE, BASE+4, BASE+8, BASE+0x880, BASE+0x884,
                          BASE+0x980, BASE+0x984, GLOBAL,
                          *DMA_REGISTERS,*ALL_CONFIG_MASKS))

    def __init__(self, board, stop_profile=None, stop_ticks=2, *,
                 data_profile=None, data_fixture=None, data_ticks=400000, ack_ticks=2,
                 histogram_profile=None,histogram_data_ticks=400000,rotation_limit=3):
        if type(rotation_limit) is not int or rotation_limit not in (3,4,5):
            raise ValueError('Producer rotation limit must be the explicit 3, 4 or 5 group scope')
        if rotation_limit==5:
            from .input_processing_devices import InputProcessingControl,CARD_STAGING_EVENT_PROFILE
            inputs=[d for d in getattr(board,'devices',()) if isinstance(d,InputProcessingControl)]
            if len(inputs)!=1 or inputs[0].event_profile!=CARD_STAGING_EVENT_PROFILE:
                raise ValueError('Fifth producer rotation requires the explicit card-staging input profile')
        self.rotation_limit=rotation_limit
        if stop_profile is not None and stop_profile not in STOP_PROFILES:
            raise ValueError('Unknown producer stop profile')
        if isinstance(stop_ticks, bool) or not isinstance(stop_ticks, int) or stop_ticks < 1:
            raise ValueError('Producer stop delay must be a positive integer')
        stream_profile = (producer_stream.ProducerStreamProfile.XM1
                          if data_profile == producer_stream.ProducerStreamProfile.XM1 else None)
        if stream_profile is not None:
            if data_fixture is not None:
                raise ValueError('Continuous statistics scenes do not accept recorded statistics')
            data_profile = None
        if data_profile is not None and data_profile not in DATA_PROFILES:
            raise ValueError('Unknown producer data profile')
        if (data_profile is None) != (data_fixture is None):
            raise ValueError('Producer data profile and fixture must be selected together')
        for delay in (data_ticks,ack_ticks):
            if isinstance(delay,bool) or not isinstance(delay,int) or delay < 1:
                raise ValueError('Producer data and acknowledgment delays must be positive integers')
        self.data_profile,self.data_ticks,self.ack_ticks=data_profile,data_ticks,ack_ticks
        self.data_fixture=None
        if data_profile is not None:
            from .recorded_statistics import RecordedStatisticsFixture
            self.data_fixture=RecordedStatisticsFixture.load(data_fixture)
        self.pending_data=self.last_data=self.pending_ack=None
        self.data_jobs=self.data_transfers=self.data_bytes=self.data_irqs=0
        self.ack_requests={0:0,1:0}
        self.ack_completions={0:0,1:0}
        self.ack_channel=0
        self.ack_step='status'
        self.data_release_step=0
        self.data_idle_observed=False
        self.data_disable_secondary=False
        self.released_shutdown=None
        self.rearm=None
        self.fresh_shutdown=None
        self.post_shutdown_control=None
        self.preview_idle_stop=None
        self.preview_configuration=None
        self.board = board
        self.stop_profile, self.stop_ticks = stop_profile, stop_ticks
        self.registers = {address: 0 for address in (*self.IDLE_REGISTERS, self.GLOBAL,
            self.BASE+4,self.BASE+8,*self.DMA_REGISTERS,*self.ALL_CONFIG_MASKS)}
        self._init_histogram(histogram_profile,histogram_data_ticks)
        if data_profile is not None:
            self.registers.update({a:0 for a in self.ACK_REGISTERS if a not in self.registers})
        self.busy = False
        self.pending_stop = None
        self.last_stop = None
        self.stop_requests = self.stop_acknowledgments = 0
        self.idle_channel_stops = {0: 0, 1: 0}
        self.desired_control_writes = []
        self.destination_writes = set()
        self.dma_setup_step = 0
        self.dma_arm = None
        self.producer_enable_writes = []
        self.writes = []
        self.last_rejected = None
        board.record('Producer stop/idle fixture', base=hex(self.BASE), global_control=hex(self.GLOBAL),
            stop_profile=stop_profile, stop_ticks=stop_ticks,
            evidence='933258 global clear/poll bit28;92CC5C zero-stop/idle polls;10CC06C geometry/config;10CBB44 mask7F0 parameters;10CB618/10CB5B0 maskD4 desired configuration;10CAEA0/92F584 channel0 arm;92D5F0/92DB08 and92DC54/92DD30 packed parameter retention160..190;92E574/92E630 mode308/parameter310',
            limits='Zero initial controls/configuration/already-idle statuses and optional delayed bit28 ack are unmeasured; retained configuration and armed-awaiting-input only, no processing/data/IRQ/metadata/pixels')
        if stream_profile is not None:
            producer_stream.enable(self, data_delay_ticks=data_ticks, profile=stream_profile)

    stream=None  # Plain state of the opt-in continuous statistics stream.

    def enable_stream(self,**options):
        return producer_stream.enable(self,**options)

    def contains(self, address, size):
        return (self.BASE <= address < self.BASE+0x1000
                or self.GLOBAL <= address < self.GLOBAL+0x4000)

    def _reject(self, operation, core, address, size, value=None,
                reason='Unsupported producer operation'):
        self.last_rejected = dict(operation=operation, cpu=core, address=hex(address), size=size,
            value=None if value is None else hex(value) if isinstance(value,int) else repr(value), reason=reason)
        self.board.record('Producer request rejected', **self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    def _check(self, operation, core, address, size, value=None):
        if size == 4 and self.stream is not None and producer_stream.is_extra_word(self, address):
            return
        if size != 4 or not self._allowed(operation, address):
            self._reject(operation, core, address, size, value)

    def _allowed(self, operation, address):
        """Membership in the union of the register sets the profiles enable."""
        if address in (self.READABLE if operation == 'read' else self.WRITABLE):
            return True
        if self.histogram_profile is not None and address in self.HISTOGRAM_WORDS:
            return True
        if address not in self.ACK_REGISTERS:
            return False
        rearm = self.rearm if type(self.rearm) is dict else {}
        return (self.stream is not None or self.data_profile is not None or self.histogram_profile is not None
                or any(type(rearm.get(name)) is dict and rearm[name].get('data') is not None
                       for name in ('afs_focus', 'capture_metstats', 'still_capture')))

    def _idle(self):
        return all(self.registers[address] == 0 for address in self.IDLE_REGISTERS)

    def _awaiting_input_configuration(self):
        return (self.dma_arm is not None and self.dma_setup_step == len(self.DMA_SETUP)
                and self.data_profile is None and self.histogram_profile is None
                and self.data_jobs == 0 and not self.dma_busy
                and self.pending_data is None and self.pending_ack is None and self.rearm is None)

    def _config_valid(self, address, value):
        if type(value) is not int or not 0<=value<=0xffffffff or value & ~self.ALL_CONFIG_MASKS[address]:
            return False
        if address == self.GLOBAL+0xc:
            return (value & 7) <= 5 and ((value >> 3) & 15) <= 11
        if address == self.BASE+0x600:
            return value <= 2
        if address == self.BASE+0x308:
            return value <= 5
        if address == self.BASE+0x524:
            return bool(value&0x200) and bool(value&0x100)==bool(value&0xff)
        return True

    def _dma_route(self):
        interrupts = getattr(self.board,'interrupts',None)
        targets = [] if interrupts is None else [index for index,bank in enumerate(interrupts.banks)
            if bank.enabled&(1<<0x2e)]
        if len(targets) != 1:
            raise UnsupportedDevice('Producer arm requires one firmware-enabled IRQ2E route')
        return targets[0]

    def _dma_write(self,core,address,size,value):
        if self.dma_arm is not None:
            if self.data_profile is not None and self.last_data is not None and self.last_data['state']=='consumed-once':
                expected=((self.BASE+0x884,0),(self.BASE+0x8f8,0))
                if (self.data_release_step>=len(expected) or (address,value)!=expected[self.data_release_step]
                        or self.busy or self.pending_data is not None or self.pending_ack is not None
                        or self.ack_completions!={0:1,1:1}
                        or self.registers[self.BASE+4]!=self.PRODUCER_DISABLED_MASK
                        or not self.data_disable_secondary
                        or self.registers[address]!=(1 if self.data_release_step==0 else 0x10000000)
                        or self._config_hash()!=self.last_data['configuration_sha256']
                        or self._retained_config_hash()!=self.last_data['retained_configuration_sha256']
                        or self.data_release_step==1 and not self.data_idle_observed):
                    self._reject('write',core,address,size,value,
                        reason='Only the acknowledged one-shot disable/84-clear/890-idle/F8-clear release is modeled')
                self.data_release_step+=1
                if self.data_release_step==2:
                    self.dma_arm['state']='released-after-recorded-input'
                    self.last_data=dict(self.last_data,state='released-after-recorded-input',
                        released_tick=self.board.virtual_ticks)
                    self.board.record('Recorded statistics DMA released by guest',**self.last_data)
                return
            self._reject('write',core,address,size,value,
                reason='Armed producer acknowledgment/stop needs a functional data/quiescence contract')
        expected = self.DMA_SETUP[self.dma_setup_step]
        if (address,value) != expected:
            self._reject('write',core,address,size,value,
                reason='Unsupported producer channel0 initialization order/value')
        if self.dma_setup_step == 0 and (not self._idle()
                or self.destination_writes != set(self.DESTINATIONS)
                or not self._config_valid(self.BASE+0x524,self.registers[self.BASE+0x524])):
            self._reject('write',core,address,size,value,
                reason='Producer channel0 arm requires nine retained destinations and packed count')
        route = None
        if self.dma_setup_step >= 5:
            try:route = self._dma_route()
            except UnsupportedDevice as error:
                self._reject('write',core,address,size,value,reason=str(error))
        retained_hash=self._retained_config_hash()
        self.dma_setup_step += 1
        if self.dma_setup_step == len(self.DMA_SETUP):
            self.dma_arm = dict(channel=0,route=route,irq=hex(0x2e),armed_tick=self.board.virtual_ticks,
                state='armed-awaiting-input',configuration_sha256=self._config_hash(),
                retained_configuration_sha256=retained_hash,
                destinations={hex(a):hex(self.registers[a]) for a in self.DESTINATIONS},
                packed_count=hex(self.registers[self.BASE+0x524]),
                initialization_ack_request='0x1 retained at FFF188F4; no hardware clear supplied',
                effects='No input/transfer/data/status transition/completion/IRQ/metadata/pixels')
            self.board.record('Producer channel0 armed awaiting input',**self.dma_arm)

    def _producer_enable_write(self,core,address,size,value):
        old = self.registers[address]
        if self.dma_arm is None:
            self._reject('write',core,address,size,value,
                reason='Producer desired DMA enable requires the complete channel0 arm')
        consumed=(self.data_profile is not None and self.last_data is not None
                  and self.last_data['state']=='consumed-once' and self.ack_completions=={0:1,1:1})
        if consumed and self._retained_config_hash()!=self.last_data['retained_configuration_sha256']:
            self._reject('write',core,address,size,value,
                reason='Consumed statistics retained configuration changed before disable/release')
        if consumed and address==self.BASE+4:
            valid=old==self.PRODUCER_SELECT_MASK and value==self.PRODUCER_DISABLED_MASK
        elif consumed:
            valid=self.registers[self.BASE+4]==self.PRODUCER_DISABLED_MASK and old==value==0
        elif address == self.BASE+4:
            valid = old == 0 and value == self.PRODUCER_SELECT_MASK
        else:
            valid = (self.registers[self.BASE+4] == self.PRODUCER_SELECT_MASK and old == value == 0)
        if not valid:
            self._reject('write',core,address,size,value,
                reason='Only the actual maskFF desired producer enable and retained secondary word are modeled')
        if address==self.BASE+8 and self.data_profile is not None and not consumed:
            self._request_data(core)
        if consumed and address==self.BASE+8:
            self.data_disable_secondary=True
        self.producer_enable_writes.append(dict(cpu=core,address=hex(address),old=hex(old),value=hex(value),
            effects='Desired enable/select fields only; no processing or DMA payload/completion'))

    def _memo_hash(self, kind, addresses, extra, compute):
        """Reuse a hash while its register words (and extra inputs) are unchanged.

        The hashes are pure functions of these words. The memo lives outside
        device state; X70_EMULATOR_AUDIT=1 recomputes every time.
        """
        registers = self.registers
        key = (tuple([registers[address] for address in addresses]), extra)
        if strict_audit.ENABLED:
            return compute()
        memo = _hash_memo.get(self)
        if memo is None:
            memo = _hash_memo[self] = {}
        hit = memo.get(kind)
        if hit is not None and hit[0] == key:
            return hit[1]
        value = compute()
        memo[kind] = (key, value)
        return value

    def _config_hash(self):
        """Original configuration domain pinned by the recorded-input fixture."""
        return self._memo_hash('legacy', _sorted_masks(self.CONFIG_MASKS), None,
                               self._compute_config_hash)

    def _compute_config_hash(self):
        return _pairs_sha256(self.registers, _sorted_masks(self.CONFIG_MASKS))

    def _retained_config_hash(self):
        """Hash every typed retained parameter; reject invalid stored words."""
        return self._memo_hash('retained', _sorted_masks(self.ALL_CONFIG_MASKS), self.dma_setup_step,
                               self._compute_retained_config_hash)

    def _compute_retained_config_hash(self):
        registers, masks = self.registers, self.ALL_CONFIG_MASKS
        addresses = _sorted_masks(masks)
        words = [registers[address] for address in addresses]
        # _config_valid for every word: the mask test in bulk, then the few
        # addresses with value rules (and the idle 0x524 exemption) one by one.
        valid = (all(type(word) is int and 0 <= word <= 0xffffffff for word in words)
                 and not any(word & ~masks[address] for address, word in zip(addresses, words))
                 and all(self._config_valid(address, registers[address])
                         or (address == self.BASE+0x524 and registers[address] == 0
                             and self.dma_setup_step == 0)
                         for address in (self.GLOBAL+0xc, self.BASE+0x600, self.BASE+0x308, self.BASE+0x524)
                         if address in masks))
        if not valid:
            raise UnsupportedDevice('Producer retained configuration has unknown word type/bits/domain')
        return _pairs_sha256(registers, addresses)

    def _data_plan(self):
        """Validate the complete declared foreign-input projection before writes."""
        fixture=self.data_fixture
        if (fixture is None or self.dma_arm is None or self.data_transfers
                or self._config_hash()!=fixture.expected_config_sha256
                or self.dma_arm['configuration_sha256']!=fixture.expected_config_sha256
                or self._retained_config_hash()!=self.dma_arm['retained_configuration_sha256']
                or any(type(self.registers[a]) is not int or self.registers[a]!=0
                       for a in (*self.PARAMETER_CONFIG_MASKS,*self.MODE_CONFIG_MASKS,
                                 *self.RAW_LUT_CONFIG_MASKS,*self.EXTRA_GLOBAL_CONFIG_MASKS))
                or self.registers[self.GLOBAL]!=0x10030104
                or self.registers[self.BASE+4]!=self.PRODUCER_SELECT_MASK
                or self.registers[self.BASE+8]!=0
                or self.registers[self.BASE+0x884]!=1
                or self.registers[self.BASE+0x8f8]!=0x10000000):
            raise UnsupportedDevice('Recorded statistics require the exact startup configuration and first unconsumed arm')
        route=self._dma_route()
        if route!=self.dma_arm['route']:
            raise UnsupportedDevice('Recorded statistics IRQ route changed after arm')
        base=self.registers[self.BASE+0x500]
        if base!=0x5fecaa60 or any(self.registers[a]!=base+offset
                for a,offset in fixture.register_offsets.items()):
            raise UnsupportedDevice('Recorded statistics require the exact first-bank destination layout')
        ram_bytes=ctypes.sizeof(self.board.ram)
        plan=[];ranges=[]
        for register,offset,data in fixture.spans:
            address=self.registers[register]+offset
            size=len(data)
            physical=address-0x40000000
            if (not size or address&3 or size&3 or not 0x40000000<=address<0x80000000
                    or address+size>0x80000000 or physical<0 or physical+size>ram_bytes):
                raise UnsupportedDevice('Recorded statistics destination lies outside aligned shared DRAM')
            if any(physical<end and begin<physical+size for begin,end in ranges):
                raise UnsupportedDevice('Recorded statistics destination spans overlap')
            ranges.append((physical,physical+size))
            plan.append((address,data))
        if len(plan)!=22 or sum(len(data) for _,data in plan)!=0x1460:
            raise UnsupportedDevice('Recorded statistics projection span contract changed')
        return route,tuple(plan)

    def _request_data(self,core):
        if self.pending_data is not None or self.last_data is not None:
            self._reject('write',core,self.BASE+8,4,0,reason='Recorded statistics are a one-shot input; rearm is unsupported')
        try:route,plan=self._data_plan()
        except UnsupportedDevice as error:
            self._reject('write',core,self.BASE+8,4,0,reason=str(error))
        self.pending_data=dict(profile=self.data_profile,state='pending-recorded-input',
            requested_tick=self.board.virtual_ticks,due_tick=self.board.virtual_ticks+self.data_ticks,
            configuration_sha256=self._config_hash(),fixture_sha256=self.data_fixture.sha256,
            retained_configuration_sha256=self._retained_config_hash(),
            route=route,spans=[dict(address=hex(a),bytes=len(data),sha256=hashlib.sha256(data).hexdigest())
                               for a,data in plan],total_bytes=sum(len(data) for _,data in plan),
            source_mode='0x28',guest_mode='0x50',mode_compatibility='deliberately mismatched consumer experiment')
        self.last_data=dict(self.pending_data)
        self.data_jobs+=1
        self.busy=True
        self.dma_arm['state']='pending-recorded-input'
        self.dma_arm['effects']='Explicit one-shot foreign recorded statistics pending; no sensor/ISP computation or display pixels'
        self.board.record('Recorded statistics input requested',**self.pending_data)

    def _ack_write(self,core,address,size,value):
        if self.last_data is None or self.last_data['state']!='completed-awaiting-ack' or self.pending_data is not None:
            self._reject('write',core,address,size,value,reason='Statistics acknowledgment requires delivered recorded bytes')
        if self._retained_config_hash()!=self.last_data['retained_configuration_sha256']:
            self._reject('write',core,address,size,value,
                reason='Statistics retained configuration changed before acknowledgment')
        expected_address=self.BASE+0x88c+self.ack_channel*0x100+(0x68 if self.ack_step=='request' else 0)
        expected_value=1 if self.ack_step=='request' else 0
        if self.ack_channel>1 or self.pending_ack is not None or address!=expected_address or value!=expected_value:
            self._reject('write',core,address,size,value,reason='Statistics paired-channel acknowledgment order/value is unsupported')
        old=self.registers[address]
        self.registers[address]=value
        event=dict(cpu=core,address=hex(address),value=hex(value),old=hex(old))
        self.writes.append(event)
        self.board.record('Producer statistics acknowledgment requested',**event)
        if self.ack_step=='status':
            self.ack_step='request'
        else:
            self.pending_ack=dict(channel=self.ack_channel,address=address,requested_tick=self.board.virtual_ticks,
                due_tick=self.board.virtual_ticks+self.ack_ticks,configuration_sha256=self._config_hash(),
                retained_configuration_sha256=self._retained_config_hash())
            self.ack_requests[self.ack_channel]+=1
            self.busy=True

    def _ack_read(self,core,address,size):
        if self.last_data is None or self.last_data['state'] not in ('completed-awaiting-ack','consumed-once','released-after-recorded-input'):
            self._reject('read',core,address,size,reason='Statistics acknowledgment read requires delivered recorded bytes')
        if (self.last_data['state']!='released-after-recorded-input'
                and self._retained_config_hash()!=self.last_data['retained_configuration_sha256']):
            self._reject('read',core,address,size,
                reason='Statistics retained configuration changed before acknowledgment/release')
        channel=(address-self.BASE-0x88c)//0x100
        if (channel>self.ack_channel or channel not in (0,1)
                or channel==self.ack_channel and self.pending_ack is None):
            self._reject('read',core,address,size,reason='Statistics channel acknowledgment read precedes its paired request')
        return self.registers[address]

    def _tick_data(self,board):
        if self.pending_data is not None:
            request=self.pending_data
            if self._retained_config_hash()!=request['retained_configuration_sha256']:
                self._reject('tick',None,self.BASE+8,4,
                    reason='Pending statistics retained configuration changed')
            if board.virtual_ticks<request['due_tick']:return
            try:route,plan=self._data_plan()
            except UnsupportedDevice as error:
                self._reject('tick',None,self.BASE+8,4,reason=str(error))
            if (route!=request['route'] or self._config_hash()!=request['configuration_sha256']
                    or self._retained_config_hash()!=request['retained_configuration_sha256']):
                self._reject('tick',None,self.BASE+8,4,reason='Recorded statistics request changed before delivery')
            # All spans and route are preflighted together. No UI, metadata,
            # callback slot or task state is supplied; stock code owns those.
            for address,data in plan:board.write_memory(address,data)
            self.data_transfers+=1
            self.data_bytes+=sum(len(data) for _,data in plan)
            self.pending_data=None
            self.busy=False
            self.last_data=dict(request,state='completed-awaiting-ack',completed_tick=board.virtual_ticks)
            self.dma_arm['state']='completed-awaiting-ack'
            self.dma_arm['effects']='Recorded foreign-mode statistics delivered before oneIRQ2E; no sensor/ISP computation or display pixels'
            board.record('Recorded statistics bytes delivered before IRQ',**self.last_data,
                effects='Recorded foreign-mode statistics only; no sensor/ISP computation or display pixels')
            board.queue_irq(route,0x2e)
            self.data_irqs+=1
        if self.pending_ack is not None:
            request=self.pending_ack
            if (self._config_hash()!=request['configuration_sha256']
                    or self._retained_config_hash()!=request['retained_configuration_sha256']
                    or self.registers[request['address']]!=1):
                self._reject('tick',None,request['address'],4,reason='Pending statistics acknowledgment changed')
            if board.virtual_ticks<request['due_tick']:return
            self.registers[request['address']]=0
            self.ack_completions[request['channel']]+=1
            self.pending_ack=None
            self.busy=False
            self.ack_channel+=1
            self.ack_step='status'
            board.record('Producer statistics channel acknowledged',channel=request['channel'],
                completed_tick=board.virtual_ticks,latency='explicit unmeasured virtual delay')
            if self.ack_channel==2:
                self.last_data=dict(self.last_data,state='consumed-once',acknowledged_tick=board.virtual_ticks)
                self.dma_arm['state']='consumed-once'

    def _global_valid(self, value):
        return (isinstance(value,int) and not isinstance(value,bool)
            and 0 <= value <= 0xffffffff
            and not value & ~(self.GLOBAL_DESIRED_MASK|0x10000000)
            and ((value>>2)&3) <= 2 and ((value>>8)&3) <= 2)

    def _data_released(self):
        return (self.data_profile is not None and self.last_data is not None
                and self.last_data['state']=='released-after-recorded-input'
                and self.data_release_step==2 and self.data_idle_observed
                and self.pending_data is None and self.pending_ack is None
                and self.ack_completions=={0:1,1:1}
                and self.registers[self.BASE+0x884]==self.registers[self.BASE+0x8f8]==0)

    def _released_stop_idle(self, *, allow_pending_stop=False):
        """An already-released one-shot may clear retained desired modes.

        This validates stored virtual idle/disable state, not physical ISP
        quiescence. It neither resets release history nor permits a new job.
        """
        zeros=(*self.IDLE_REGISTERS,self.BASE+8,self.BASE+0x884,self.BASE+0x8f8,
               self.BASE+0x88c,self.BASE+0x98c,self.BASE+0x8f4,self.BASE+0x9f4)
        stop_idle=(self.busy is True and self.pending_stop is not None if allow_pending_stop
                   else not self.busy and self.pending_stop is None)
        return (self._data_released() and self.dma_arm is not None
            and self.dma_arm['state']=='released-after-recorded-input'
            and type(self.data_release_step) is int and self.data_release_step==2
            and self.data_idle_observed is True and stop_idle and self.data_disable_secondary is True
            and type(self.registers[self.BASE+4]) is int
            and self.registers[self.BASE+4]==self.PRODUCER_DISABLED_MASK
            and self.ack_requests==self.ack_completions=={0:1,1:1}
            and all(type(counts[channel]) is int for counts in (self.ack_requests,self.ack_completions)
                    for channel in (0,1))
            and all(type(self.registers[a]) is int and self.registers[a]==0 for a in zeros))

    def desired_control(self):
        value = self.registers[self.GLOBAL]
        return dict(raw=hex(value),mode2=(value>>2)&3,mode8=(value>>8)&3,
            mode16=(value>>16)&3,configuration18=bool(value&(1<<18)),
            configuration12=bool(value&(1<<12)),configuration13=bool(value&(1<<13)),
            stop_ack28=bool(value&(1<<28)),
            effects='Stock desired unit modes retained; processing/data/completion not modeled; bit28 is only the prior declared stop fixture')

    def _released_shutdown_access(self, operation, core, address, size, value=None):
        """Retain the complete second92CC5C stop after a released one-shot.

        Only an already-zero transaction is modeled. Its synchronous partial
        step/hash/history participate in checkpoints, without resetting the
        previous arm or enabling another recorded-data transfer.
        """
        request=self.released_shutdown
        beginning=request is None
        if not beginning:
            step=request.get('step') if isinstance(request,dict) else None
            accesses=request.get('accesses') if isinstance(request,dict) else None
            if (type(step) is not int or not 1<=step<=len(self.RELEASED_SHUTDOWN)
                    or request.get('state')!=('completed' if step==len(self.RELEASED_SHUTDOWN) else 'in-progress')
                    or not isinstance(accesses,list) or len(accesses)!=step
                    or any(not isinstance(event,dict) or type(event.get('step')) is not int
                        or event['step']!=index
                        or (event.get('operation'),event.get('address'),event.get('value'))
                            !=(kind,hex(target),hex(word))
                        for index,(event,(kind,target,word)) in
                            enumerate(zip(accesses,self.RELEASED_SHUTDOWN[:step])))):
                self._reject(operation,core,address,size,value,
                    reason='Partial released shutdown state/step/history is inconsistent')
            if step==len(self.RELEASED_SHUTDOWN):return False
        if beginning and (operation,address,value)!=self.RELEASED_SHUTDOWN[0]:return False
        if beginning and not self._data_released():return False
        if (not self._released_stop_idle() or type(self.registers[self.GLOBAL]) is not int
                or self.registers[self.GLOBAL]!=0x10000000 or self.last_stop is None
                or self.last_stop.get('released_one_shot') is not True
                or self.last_stop['state']!='acknowledged'):
            self._reject(operation,core,address,size,value,
                reason='Repeated shutdown requires an acknowledged global stop and disabled, idle, released one-shot')
        step=0 if beginning else request['step']
        expected=self.RELEASED_SHUTDOWN[step]
        actual=self.registers[address] if operation=='read' else value
        if ((operation,address,actual)!=expected or type(actual) is not int
                or not beginning and (self._config_hash()!=request['configuration_sha256']
                    or self._retained_config_hash()!=request['retained_configuration_sha256'])):
            self._reject(operation,core,address,size,value,
                reason='Repeated released shutdown order/value or retained configuration changed')
        if beginning:
            request=dict(state='in-progress',step=0,started_tick=self.board.virtual_ticks,
                configuration_sha256=self._config_hash(),
                retained_configuration_sha256=self._retained_config_hash(),accesses=[])
            self.released_shutdown=request
        event=dict(operation=operation,cpu=core,address=hex(address),value=hex(actual),
            tick=self.board.virtual_ticks,step=step)
        if operation=='write':
            old=self.registers[address]
            self.registers[address]=value
            self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
            if address in (self.BASE+0x884,self.BASE+0x984):
                self.idle_channel_stops[(address-self.BASE-0x884)//0x100]+=1
        request['accesses'].append(event)
        request['step']+=1
        if request['step']==len(self.RELEASED_SHUTDOWN):
            request.update(state='completed',completed_tick=self.board.virtual_ticks)
        self.board.record('Released producer repeated shutdown access',**event,
            state=request['state'],effects='Retained zero controls/status only; previous one-shot ownership is unchanged')
        return True

    def _rearm_history_hash(self):
        """Fingerprint the first job; a new arm never replaces its history."""
        counts=(self.data_jobs,self.data_transfers,self.data_bytes,self.data_irqs,
                self.dma_setup_step,self.data_release_step,self.ack_channel)
        if (any(type(v) is not int for v in counts)
                or counts!=(1,1,0x1460,1,len(self.DMA_SETUP),2,2)
                or self.ack_requests!={0:1,1:1} or self.ack_completions!={0:1,1:1}
                or any(type(v) is not int for d in (self.ack_requests,self.ack_completions)
                       for v in d.values())
                or self.ack_step!='status' or self.data_idle_observed is not True
                or self.data_disable_secondary is not True
                or not isinstance(self.dma_arm,dict)
                or self.dma_arm.get('state')!='released-after-recorded-input'
                or not isinstance(self.last_data,dict)
                or self.last_data.get('state')!='released-after-recorded-input'
                or not isinstance(self.released_shutdown,dict)
                or self.released_shutdown.get('state')!='completed'
                or type(self.released_shutdown.get('step')) is not int
                or self.released_shutdown['step']!=len(self.RELEASED_SHUTDOWN)
                or not isinstance(self.last_stop,dict)
                or self.last_stop.get('state')!='acknowledged'
                or self.last_stop.get('released_one_shot') is not True):
            raise UnsupportedDevice('Fresh producer arm requires the completed original one-shot release/shutdown history')
        history=dict(dma_setup_step=self.dma_setup_step,dma_arm=self.dma_arm,last_data=self.last_data,
            data_jobs=self.data_jobs,data_transfers=self.data_transfers,data_bytes=self.data_bytes,
            data_irqs=self.data_irqs,ack_requests=self.ack_requests,ack_completions=self.ack_completions,
            ack_channel=self.ack_channel,ack_step=self.ack_step,data_release_step=self.data_release_step,
            data_idle_observed=self.data_idle_observed,data_disable_secondary=self.data_disable_secondary,
            released_shutdown=self.released_shutdown,last_stop=self.last_stop)
        try:return hashlib.sha256(json.dumps(history,sort_keys=True,separators=(',',':'),
            allow_nan=False).encode()).hexdigest()
        except (TypeError,ValueError) as error:
            raise UnsupportedDevice('Original producer history is not a plain checkpointable record') from error

    @staticmethod
    def _rearm_state(descriptor_step,setup_step,enable_step):
        if descriptor_step<10:return 'descriptor-rebuild'
        if setup_step==0:return 'descriptor-ready'
        if setup_step<7:return 'initializing'
        if enable_step==0:return 'armed-awaiting-fresh-input'
        if enable_step==1:return 'desired-enable-retained'
        return 'enabled-awaiting-fresh-input'

    def _validate_rearm_access(self):
        """Per-access ownership check: the full `_validate_rearm` audit runs
        with X70_EMULATOR_AUDIT=1, and always while streaming (its stream
        branch reseals hashes). Otherwise state changes only through this
        device's handlers, which validate their own transitions."""
        if strict_audit.ENABLED or self.stream is not None:
            self._validate_rearm()

    def _validate_rearm(self):
        """Validate current ownership before every access, without any effects."""
        if self.stream is not None:
            # The opt-in stream owns the producer registers; the finite
            # ledgers below are frozen at enable and not audited meanwhile.
            producer_stream.refresh_hashes(self)
            return
        if type(self.rearm) is dict and 'still_capture' in self.rearm:
            producer_still_capture.validate(self)
            return
        if type(self.rearm) is dict and 'capture_metstats' in self.rearm:
            producer_capture_metstats.validate(self)
            return
        if type(self.rearm) is dict and 'afs_focus' in self.rearm:
            producer_afs_focus.validate(self)
            return
        if type(self.rearm) is dict and 'measurement' in self.rearm:
            producer_measurement.validate(self)
            return
        r=self.rearm
        if not isinstance(r,dict):
            raise UnsupportedDevice('Fresh producer arm record is not a dictionary')
        afs_statistics_frame.validate(self)
        steps=tuple(r.get(k) for k in ('descriptor_step','setup_step','enable_step'))
        if (any(type(s) is not int for s in steps) or not 1<=steps[0]<=10
                or not 0<=steps[1]<=7 or not 0<=steps[2]<=2
                or steps[1] and steps[0]!=10 or steps[2] and steps[1]!=7
                or r.get('state')!=self._rearm_state(*steps)):
            raise UnsupportedDevice('Fresh producer descriptor/setup/enable step or state is inconsistent')
        if (self.busy is not False or self.pending_stop is not None
                or self.pending_data is not None or self.pending_ack is not None
                or r.get('original_history_sha256')!=self._rearm_history_hash()):
            raise UnsupportedDevice('Fresh producer current configuration, pending state or first-job history changed')
        snapshot = _configuration_snapshot(self)
        configuration_sha256, retained_configuration_sha256 = snapshot.hashes()
        if (r.get('configuration_sha256')!=configuration_sha256
                or r.get('retained_configuration_sha256')!=retained_configuration_sha256
                or type(r.get('global_control')) is not int
                or type(self.registers[self.GLOBAL]) is not int
                or self.registers[self.GLOBAL]!=self._post_shutdown_global_word(r['global_control'])
                or not self._global_valid(r['global_control'])):
            raise UnsupportedDevice('Fresh producer current configuration, pending state or first-job history changed')
        sequences=(('descriptor',self.REARM_DESCRIPTOR_ORDER),
                   ('setup',tuple(a for a,_ in self.DMA_SETUP)),
                   ('enable',tuple(a for a,_ in self.REARM_ENABLE)))
        for (name,addresses),step in zip(sequences,steps):
            events=r.get(name+'_accesses')
            if not isinstance(events,list) or len(events)!=step:
                raise UnsupportedDevice('Fresh producer ordered access count is inconsistent')
            for index,event in enumerate(events):
                word=event.get('word') if isinstance(event,dict) else None
                if (type(word) is not int or not 0<=word<=0xffffffff
                        or type(event.get('step')) is not int or event['step']!=index
                        or event.get('operation')!='write' or event.get('address')!=hex(addresses[index])
                        or event.get('value')!=hex(word)
                        or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                        or type(event.get('tick')) is not int or event['tick']<0):
                    raise UnsupportedDevice('Fresh producer ordered access record has unknown type/order/value')
                if name=='descriptor':
                    if (not self._config_valid(addresses[index],word)
                            or addresses[index] not in self.REARM_ROTATION_ORDER
                                and self.registers[addresses[index]]!=word):
                        raise UnsupportedDevice('Fresh producer destination/count no longer matches its fresh store')
                elif word!=(self.DMA_SETUP if name=='setup' else self.REARM_ENABLE)[index][1]:
                    raise UnsupportedDevice('Fresh producer initialization/enable record differs from stock transaction')
        expected={self.BASE+0x88c:0,self.BASE+0x8f4:0,self.BASE+0x8a0:0x80003000,
                  self.BASE+0x8f8:0,self.BASE+0x8c4:2,self.BASE+0x884:0}
        expected.update(self.DMA_SETUP[:steps[1]])
        expected.update({a:0 for a in (*self.IDLE_REGISTERS,self.BASE+8,
                                     self.BASE+0x98c,self.BASE+0x9f4) if a!=self.BASE+0x884})
        expected[self.BASE+4]=self.PRODUCER_SELECT_MASK if steps[2] else self.PRODUCER_DISABLED_MASK
        self._histogram_expected(expected)
        self._fresh_shutdown_project(expected)
        afs_statistics_frame.project(self,expected)
        if any(type(self.registers.get(a)) is not int or self.registers[a]!=v for a,v in expected.items()):
            raise UnsupportedDevice('Fresh producer controls/status no longer match its ordered arm stage')
        if steps[1]>=6:
            if type(r.get('route')) is not int or not self._fresh_shutdown_route_matches(r['route']):
                raise UnsupportedDevice('Fresh producer IRQ2E route changed after firmware registration')
        elif r.get('route') is not None:
            raise UnsupportedDevice('Fresh producer route precedes firmware IRQ registration')
        self._validate_rearm_pre_image_parameters(snapshot)
        self._validate_rearm_pre_image_followups(snapshot)
        self._validate_rearm_pre_image_configuration(snapshot)
        self._validate_rearm_known_configuration(snapshot)
        self._validate_rearm_image_global(snapshot)
        self._validate_rearm_rotations(snapshot)
        self._validate_rearm_geometry(snapshot)

    def _image_global_prior(self, rotation_ordinal=None):
        """Reconstruct the globals that preceded the one later triplet."""
        record=self.rearm['image_global_configuration']
        if record is None or (rotation_ordinal is not None
                and rotation_ordinal>record['after_rotation_count']):
            return {}
        return record['prior_words']

    def _known_configuration_prior(self, rotation_ordinal=None):
        record=self.rearm['known_configuration']
        prior={}
        if record is not None:
            for event in record['accesses']:
                if rotation_ordinal is None or event['after_rotation_count']>=rotation_ordinal:
                    prior.setdefault(event['address'],event['old_word'])
        return prior

    def _pre_image_parameter_prior(self, rotation_ordinal=None):
        record=self.rearm['pre_image_parameters']
        phase=self._pre_image_configuration_prior(rotation_ordinal)
        if record is None or (rotation_ordinal is not None
                and rotation_ordinal>record['after_rotation_count']):return phase
        return {**phase,**self._pre_image_followup_prior(),**record['prior_words']}

    def _pre_image_followup_prior(self, first=0):
        record=self.rearm['pre_image_parameters']
        if record is None:return {}
        prior={}
        for group in record['followups'][first:]:
            if not isinstance(group,dict) or not isinstance(group.get('prior_words'),dict):
                raise UnsupportedDevice('Pre-image followup prior-word history is malformed')
            for address,word in group['prior_words'].items():prior.setdefault(address,word)
        return prior

    def _pre_image_configuration_prior(self, rotation_ordinal=None):
        record=self.rearm['pre_image_configuration'];prior={}
        if record is not None:
            for event in record['accesses']:
                if rotation_ordinal is None or event['after_rotation_count']>=rotation_ordinal:
                    prior.setdefault(event['address'],event['old_word'])
        return prior

    def _validate_rearm_pre_image_configuration(self, snapshot=None):
        """Bind the passive post-family phase to its complete earlier archive."""
        r=self.rearm
        if ('pre_image_configuration' not in r
                or type(r.get('pre_image_configuration_started')) is not bool):
            raise UnsupportedDevice('Pre-image configuration marker is missing or untyped')
        record=r['pre_image_configuration']
        if record is None:
            if r['pre_image_configuration_started']:
                raise UnsupportedDevice('Pre-image configuration history was removed')
            return
        parent=r['pre_image_parameters'];image=r['image_global_configuration']
        events=record.get('accesses') if isinstance(record,dict) else None
        prior=record.get('prior_words') if isinstance(record,dict) else None
        if (r['pre_image_configuration_started'] is not True
                or not isinstance(parent,dict) or parent.get('state')!='completed'
                or len(parent['followups'])!=len(self.PRE_IMAGE_FOLLOWUP_ORDERS)
                or any(g.get('state')!='completed' for g in parent['followups'])
                or r['state']!='enabled-awaiting-fresh-input'
                or not isinstance(events,list) or not 1<=len(events)<=self.REARM_KNOWN_CONFIG_LIMIT
                or not isinstance(prior,dict) or type(record.get('stores')) is not int
                or record['stores']!=len(events)
                or record.get('state')!=('retaining-desired-configuration' if image is None else 'sealed')
                or record.get('sealed_tick')!=(None if image is None else image['started_tick'])):
            raise UnsupportedDevice('Pre-image configuration phase/archive/count or seal changed')
        count=parent['after_rotation_count'];previous_tick=parent['followups'][-1]['completed_tick']
        effective={};counts=[];owner=parent['accesses'][0]['cpu']
        for index,event in enumerate(events):
            a=event.get('address') if isinstance(event,dict) else None
            address=int(a,16) if isinstance(a,str) else -1
            old=event.get('old_word') if isinstance(event,dict) else None
            word=event.get('word') if isinstance(event,dict) else None
            inserted=event.get('after_rotation_count') if isinstance(event,dict) else None
            if (address not in self.PRE_IMAGE_DESIRED_CONFIG or a!=hex(address)
                    or not self._known_pair_valid(address,old,word)
                    or event.get('operation')!='write' or event.get('value')!=hex(word) or event.get('old')!=hex(old)
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('cpu')) is not int or event['cpu']!=owner
                    or type(inserted) is not int or not count<=inserted<=len(r['rotations'])
                    or counts and inserted<counts[-1]
                    or inserted and r['rotations'][inserted-1]['state']!='completed'
                    or type(event.get('tick')) is not int or not max(previous_tick,
                        r['rotations'][inserted-1]['completed_tick'] if inserted
                        else r['geometry']['completed_tick'])<=event['tick']<=0xffffffffffffffff
                    or old!=effective.get(a,prior.get(a))):
                raise UnsupportedDevice('Pre-image configuration masked store/owner/prior chain changed')
            effective[a]=word;counts.append(inserted);previous_tick=event['tick']
        historical=self._known_configuration_prior()
        if (set(prior)!=set(effective) or prior!=self._pre_image_configuration_prior()
                or any(historical.get(a,self.registers[int(a,16)])!=word for a,word in effective.items())
                or record.get('started_tick')!=events[0]['tick'] or type(record.get('started_tick')) is not int
                or record.get('latest_tick')!=previous_tick or type(record.get('latest_tick')) is not int
                or record.get('ledger_sha256')!=self._known_ledger_hash(events)
                or image is not None and (image['started_tick']<previous_tick or image['after_rotation_count']<counts[-1])):
            raise UnsupportedDevice('Pre-image configuration current words/time or ledger hash changed')
        initial={**historical,**self._image_global_prior(),**prior}
        initial.update({hex(a):event['word'] for a,event in zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
        for rotation in r['rotations'][:counts[0]]:
            initial.update({event['address']:event['word'] for event in rotation['accesses']})
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for key,digest in zip(('initial_configuration_sha256','initial_retained_configuration_sha256'),snapshot.hashes(initial)):
            if record.get(key)!=digest:
                raise UnsupportedDevice('Pre-image configuration initial hash or earlier archive changed')

    def _validate_rearm_pre_image_parameters(self, snapshot=None):
        """Keep one whole10CB9B4 transaction before the image triplet.

        The seeded-card boot programs this family after geometry, before the
        first rotation. Its zero-count chronology is anchored to geometry.
        """
        r=self.rearm
        if 'pre_image_parameters' not in r or type(r.get('pre_image_parameters_started')) is not bool:
            raise UnsupportedDevice('Pre-image parameter history marker is missing or untyped')
        record=r['pre_image_parameters']
        if record is None:
            if r['pre_image_parameters_started']:
                raise UnsupportedDevice('Pre-image parameter history was removed')
            return
        step=record.get('step') if isinstance(record,dict) else None
        count=record.get('after_rotation_count') if isinstance(record,dict) else None
        prior=record.get('prior_words') if isinstance(record,dict) else None
        events=record.get('accesses') if isinstance(record,dict) else None
        rotations=r.get('rotations');image=r.get('image_global_configuration')
        if (r['pre_image_parameters_started'] is not True
                or r['state']!='enabled-awaiting-fresh-input'
                or not isinstance(r.get('geometry'),dict) or r['geometry'].get('state')!='completed'
                or type(step) is not int or not 1<=step<=len(self.PRE_IMAGE_PARAMETER_ORDER)
                or record.get('state')!=('completed' if step==len(self.PRE_IMAGE_PARAMETER_ORDER) else 'in-progress')
                or type(count) is not int or not 0<=count<=self._rotation_limit()
                or not isinstance(rotations,list) or count>len(rotations)
                or any(a.get('state')!='completed' for a in rotations[:count])
                or step<len(self.PRE_IMAGE_PARAMETER_ORDER) and (len(rotations)!=count or image is not None)
                or not isinstance(events,list) or len(events)!=step
                or not isinstance(prior,dict) or set(prior)!={hex(a) for a in self.PRE_IMAGE_PARAMETER_ORDER}
                or any(not self._config_valid(a,prior[hex(a)]) for a in self.PRE_IMAGE_PARAMETER_ORDER)
                or type(record.get('started_tick')) is not int
                or type(r['geometry'].get('completed_tick')) is not int
                or record['started_tick']<(rotations[count-1]['completed_tick'] if count
                    else r['geometry']['completed_tick'])
                or image is not None and (image['after_rotation_count']<count
                    or image['started_tick']<record.get('completed_tick',0))):
            raise UnsupportedDevice('Pre-image parameter state/order/rotation or prior words changed')
        effective=dict(prior);previous_tick=record['started_tick'];owner=events[0].get('cpu')
        for index,event in enumerate(events):
            address=self.PRE_IMAGE_PARAMETER_ORDER[index]
            word=event.get('word') if isinstance(event,dict) else None
            old=prior[hex(address)]
            if (not self._config_valid(address,word)
                    or event.get('operation')!='write' or event.get('address')!=hex(address)
                    or event.get('value')!=hex(word) or event.get('old')!=hex(old)
                    or type(event.get('old_word')) is not int or event['old_word']!=old
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('after_rotation_count')) is not int or event['after_rotation_count']!=count
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1) or event['cpu']!=owner
                    or type(event.get('tick')) is not int or not previous_tick<=event['tick']<=0xffffffffffffffff
                    or index==0 and event['tick']!=record['started_tick']):
                raise UnsupportedDevice('Pre-image parameter store ledger differs from the fourteen stock stores')
            effective[hex(address)]=word;previous_tick=event['tick']
        historical={**self._known_configuration_prior(),**self._pre_image_configuration_prior()}
        if (any(historical.get(hex(a),self.registers[a])!=effective[hex(a)]
                for a in self.PRE_IMAGE_PARAMETER_ORDER)
                or record.get('ledger_sha256')!=self._known_ledger_hash(events)
                or step==len(self.PRE_IMAGE_PARAMETER_ORDER) and (type(record.get('completed_tick')) is not int
                    or record['completed_tick']!=previous_tick)
                or step<len(self.PRE_IMAGE_PARAMETER_ORDER) and record.get('completed_tick') is not None):
            raise UnsupportedDevice('Pre-image parameter current words, hash or completion changed')
        initial={**historical,**prior,**self._pre_image_followup_prior(),**self._image_global_prior()}
        initial.update({hex(a):event['word'] for a,event in
            zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
        for rotation in rotations[:count]:
            initial.update({event['address']:event['word'] for event in rotation['accesses']})
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for key,digest in zip(('initial_configuration_sha256','initial_retained_configuration_sha256'),
                              snapshot.hashes(initial)):
            if record.get(key)!=digest:
                raise UnsupportedDevice('Pre-image parameter initial configuration or prior descriptor changed')

    def _validate_rearm_pre_image_followups(self, snapshot=None):
        """Validate the optional complete mode/1F0 families in the same window."""
        r=self.rearm;parent=r['pre_image_parameters']
        if parent is None:return
        groups=parent.get('followups');marker=parent.get('followups_started')
        if (not isinstance(groups,list) or not 0<=len(groups)<=len(self.PRE_IMAGE_FOLLOWUP_ORDERS)
                or type(marker) is not bool or marker!=(len(groups)>0)):
            raise UnsupportedDevice('Pre-image followup family history/marker changed')
        if not groups:return
        count=parent['after_rotation_count'];owner=parent['accesses'][0]['cpu']
        previous_tick=parent.get('completed_tick');image=r['image_global_configuration']
        if parent['state']!='completed' or type(previous_tick) is not int:
            raise UnsupportedDevice('Pre-image followups require the complete fourteen-store parameter family')
        historical={**self._known_configuration_prior(),**self._pre_image_configuration_prior()};effective={}
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for ordinal,group in enumerate(groups):
            name,order=self.PRE_IMAGE_FOLLOWUP_ORDERS[ordinal]
            step=group.get('step') if isinstance(group,dict) else None
            events=group.get('accesses') if isinstance(group,dict) else None
            prior=group.get('prior_words') if isinstance(group,dict) else None
            if (group.get('family')!=name or type(step) is not int or not 1<=step<=len(order)
                    or group.get('state')!=('completed' if step==len(order) else 'in-progress')
                    or step<len(order) and (ordinal!=len(groups)-1 or image is not None or len(r['rotations'])!=count)
                    or not isinstance(events,list) or len(events)!=step
                    or not isinstance(prior,dict) or set(prior)!={hex(a) for a in order}
                    or any(not self._config_valid(a,prior[hex(a)]) for a in order)
                    or type(group.get('started_tick')) is not int or group['started_tick']<previous_tick):
                raise UnsupportedDevice('Pre-image followup family/step/ordered prior words changed')
            initial={**historical,**self._pre_image_followup_prior(ordinal),**self._image_global_prior()}
            initial.update({hex(a):event['word'] for a,event in
                zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
            for rotation in r['rotations'][:count]:
                initial.update({event['address']:event['word'] for event in rotation['accesses']})
            for key,digest in zip(('initial_configuration_sha256','initial_retained_configuration_sha256'),
                                  snapshot.hashes(initial)):
                if group.get(key)!=digest:
                    raise UnsupportedDevice('Pre-image followup initial configuration or preceding family changed')
            effective.update(prior);previous_tick=group['started_tick']
            for index,event in enumerate(events):
                address=order[index];word=event.get('word') if isinstance(event,dict) else None
                old=prior[hex(address)]
                if (not self._config_valid(address,word)
                        or event.get('operation')!='write' or event.get('address')!=hex(address)
                        or event.get('value')!=hex(word) or event.get('old')!=hex(old)
                        or type(event.get('old_word')) is not int or event['old_word']!=old
                        or type(event.get('step')) is not int or event['step']!=index
                        or type(event.get('after_rotation_count')) is not int or event['after_rotation_count']!=count
                        or type(event.get('cpu')) is not int or event['cpu']!=owner
                        or type(event.get('tick')) is not int or not previous_tick<=event['tick']<=0xffffffffffffffff
                        or index==0 and event['tick']!=group['started_tick']):
                    raise UnsupportedDevice('Pre-image followup masked store/owner/order/time changed')
                effective[hex(address)]=word;previous_tick=event['tick']
            if (group.get('ledger_sha256')!=self._known_ledger_hash(events)
                    or step==len(order) and (type(group.get('completed_tick')) is not int
                        or group['completed_tick']!=previous_tick)
                    or step<len(order) and group.get('completed_tick') is not None):
                raise UnsupportedDevice('Pre-image followup ledger hash/completion changed')
        if (any(historical.get(a,self.registers[int(a,16)])!=word for a,word in effective.items())
                or image is not None and (image['after_rotation_count']<count or image['started_tick']<previous_tick)):
            raise UnsupportedDevice('Pre-image followup current words or later image chronology changed')

    def _known_pair_valid(self,address,old,word):
        if address!=self.GLOBAL:
            return self._config_valid(address,old) and self._config_valid(address,word)
        delta=old^word
        return (self._global_valid(old) and self._global_valid(word) and not delta&0x10000000
                and any(delta&~field==0 for field in self.GLOBAL_DESIRED_FIELDS))

    @staticmethod
    def _known_ledger_hash(events):
        return hashlib.sha256(b''.join(struct.pack('<6IQ',event['step'],event['cpu'],
            int(event['address'],16),event['old_word'],event['word'],
            event['after_rotation_count'],event['tick']) for event in events)).hexdigest()

    def _validate_rearm_known_configuration(self, snapshot=None):
        """Bind known desired stores to insertion history, without effects."""
        r=self.rearm
        if 'known_configuration' not in r or type(r.get('known_configuration_started')) is not bool:
            raise UnsupportedDevice('Known producer configuration marker is missing or untyped')
        record=r['known_configuration']
        if record is None:
            if r['known_configuration_started']:
                raise UnsupportedDevice('Known producer configuration history was removed')
            return
        events=record.get('accesses') if isinstance(record,dict) else None
        prior=record.get('prior_words') if isinstance(record,dict) else None
        image=r.get('image_global_configuration')
        if (r['known_configuration_started'] is not True
                or r['state']!='enabled-awaiting-fresh-input'
                or not isinstance(image,dict) or image.get('state')!='completed'
                or not isinstance(events,list) or not 1<=len(events)<=self.REARM_KNOWN_CONFIG_LIMIT
                or record.get('state')!='retaining-desired-configuration'
                or type(record.get('stores')) is not int or record['stores']!=len(events)
                or not isinstance(prior,dict)):
            raise UnsupportedDevice('Known producer configuration history/state/count is inconsistent')
        effective={};counts=[];previous_tick=image['completed_tick']
        for index,event in enumerate(events):
            a=event.get('address') if isinstance(event,dict) else None
            address=int(a,16) if isinstance(a,str) else -1
            count=event.get('after_rotation_count') if isinstance(event,dict) else None
            word=event.get('word') if isinstance(event,dict) else None
            old=event.get('old_word') if isinstance(event,dict) else None
            if (address not in self.KNOWN_DESIRED_CONFIG or a!=hex(address)
                    or type(word) is not int or type(old) is not int or not self._known_pair_valid(address,old,word)
                    or type(count) is not int or not 1<=count<=len(r['rotations'])
                    or counts and count<counts[-1] or r['rotations'][count-1]['state']!='completed'
                    or event.get('operation')!='write' or event.get('value')!=hex(word) or event.get('old')!=hex(old)
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                    or type(event.get('tick')) is not int or not 0<=event['tick']<=0xffffffffffffffff
                    or event['tick']<max(previous_tick,r['rotations'][count-1]['completed_tick'])
                    or old!=effective.get(a,prior.get(a))):
                raise UnsupportedDevice('Known producer configuration ordered store or prior chain changed')
            effective[a]=word;counts.append(count);previous_tick=event['tick']
        if (set(prior)!=set(effective) or prior!=self._known_configuration_prior()
                or any(type(self.registers[int(a,16)]) is not int
                    or self.registers[int(a,16)]!=(self._post_shutdown_global_word(word)
                        if int(a,16)==self.GLOBAL else word)
                    for a,word in effective.items())
                or record.get('started_tick')!=events[0]['tick']
                or type(record.get('started_tick')) is not int
                or record.get('latest_tick')!=previous_tick or type(record.get('latest_tick')) is not int
                or record.get('ledger_sha256')!=self._known_ledger_hash(events)):
            raise UnsupportedDevice('Known producer configuration current words, markers or ledger hash changed')
        initial=dict(prior)
        initial.update({hex(a):event['word'] for a,event in
            zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
        for rotation in r['rotations'][:counts[0]]:
            initial.update({event['address']:event['word'] for event in rotation['accesses']})
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for key,digest in zip(('initial_configuration_sha256',
                              'initial_retained_configuration_sha256'), snapshot.hashes(initial)):
            if record.get(key)!=digest:
                raise UnsupportedDevice('Known producer configuration initial hash or insertion descriptor changed')

    def _validate_rearm_image_global(self, snapshot=None):
        """Bind an ordered3-word update without rewriting earlier history."""
        r=self.rearm
        if ('image_global_configuration' not in r
                or type(r.get('image_global_started')) is not bool):
            raise UnsupportedDevice('Image global configuration marker is missing or untyped')
        record=r['image_global_configuration']
        if record is None:
            if r['image_global_started']:
                raise UnsupportedDevice('Image global configuration history was removed')
            return
        step=record.get('step') if isinstance(record,dict) else None
        count=record.get('after_rotation_count') if isinstance(record,dict) else None
        prior=record.get('prior_words') if isinstance(record,dict) else None
        events=record.get('accesses') if isinstance(record,dict) else None
        rotations=r.get('rotations');g=r.get('geometry')
        if (r['image_global_started'] is not True
                or r['state']!='enabled-awaiting-fresh-input'
                or not isinstance(g,dict) or g.get('state')!='completed'
                or type(step) is not int or not 1<=step<=3
                or record.get('state')!=('completed' if step==3 else 'in-progress')
                or type(count) is not int or not 1<=count<=self._rotation_limit()
                or not isinstance(rotations,list) or count>len(rotations)
                or any(not isinstance(a,dict) or a.get('state')!='completed'
                    or a.get('step')!=len(self.REARM_ROTATION_ORDER) for a in rotations[:count])
                or step<3 and len(rotations)!=count
                or not isinstance(events,list) or len(events)!=step
                or not isinstance(prior,dict) or set(prior)!={hex(a) for a in self.IMAGE_GLOBAL_CONFIG_ORDER}
                or any(not self._config_valid(a,prior[hex(a)]) for a in self.IMAGE_GLOBAL_CONFIG_ORDER)
                or type(record.get('started_tick')) is not int
                or type(rotations[count-1].get('completed_tick')) is not int
                or record['started_tick']<rotations[count-1]['completed_tick']):
            raise UnsupportedDevice('Image global configuration state/step/prior rotation or words are inconsistent')
        effective=dict(prior)
        event_tick=record['started_tick']
        for index,event in enumerate(events):
            address=self.IMAGE_GLOBAL_CONFIG_ORDER[index]
            word=event.get('word') if isinstance(event,dict) else None
            if (not self._config_valid(address,word)
                    or event.get('operation')!='write' or event.get('address')!=hex(address)
                    or event.get('value')!=hex(word) or event.get('old')!=hex(prior[hex(address)])
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                    or type(event.get('tick')) is not int or event['tick']<event_tick
                    or index==0 and event['tick']!=record['started_tick']):
                raise UnsupportedDevice('Image global configuration store ledger differs from the stock triplet')
            effective[hex(address)]=word
            event_tick=event['tick']
        historical=self._known_configuration_prior()
        if (any(type(historical.get(hex(a),self.registers[a])) is not int
                or historical.get(hex(a),self.registers[a])!=effective[hex(a)]
                for a in self.IMAGE_GLOBAL_CONFIG_ORDER)
                or step==3 and (type(record.get('completed_tick')) is not int
                    or record['completed_tick']!=event_tick)
                or step<3 and record.get('completed_tick') is not None):
            raise UnsupportedDevice('Image global configuration current words or completion changed')
        #Reconstruct the descriptor at insertion, even after a later rotation.
        initial={**self._known_configuration_prior(),**prior}
        initial.update({hex(a):event['word'] for a,event in
            zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
        for rotation in rotations[:count]:
            initial.update({event['address']:event['word'] for event in rotation['accesses']})
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for key,digest in zip(('initial_configuration_sha256',
                              'initial_retained_configuration_sha256'), snapshot.hashes(initial)):
            if record.get(key)!=digest:
                raise UnsupportedDevice('Image global configuration initial hash or prior descriptor changed')

    def startup_scope(self):
        """Pure selected input identities; this query grants no new work."""
        from . import afs_startup_scope
        inputs=[device for device in self.board.devices
                if type(device).__name__=='InputProcessingControl' and device.board is self.board]
        if len(inputs)!=1:
            raise UnsupportedDevice('AF-S producer scope requires one same-board input controller')
        return afs_startup_scope.scope(inputs[0].event_profile)

    def _rotation_limit(self):
        if type(self.rotation_limit) is not int or self.rotation_limit not in (3,4,5):
            raise UnsupportedDevice('Producer desired rotation scope was changed or has unknown type')
        if self.rotation_limit==4:
            from .input_processing_devices import InputProcessingControl, REC_DISPLAY_EVENT_PROFILES
            inputs=[d for d in getattr(self.board,'devices',()) if isinstance(d,InputProcessingControl)]
            if len(inputs)!=1 or inputs[0].event_profile not in REC_DISPLAY_EVENT_PROFILES:
                raise UnsupportedDevice('Fourth producer rotation requires exactly one separately selected late-display input profile')
        if self.rotation_limit==5:
            from .input_processing_devices import InputProcessingControl,CARD_STAGING_EVENT_PROFILE
            inputs=[d for d in getattr(self.board,'devices',()) if isinstance(d,InputProcessingControl)]
            if len(inputs)!=1 or inputs[0].event_profile!=CARD_STAGING_EVENT_PROFILE:
                raise UnsupportedDevice('Fifth producer rotation requires exactly one card-staging input profile')
        if self.rotation_limit in (4,5):
            source=self._afs_rotation_epoch()
            if source is not None:
                return source['frame']
        return self.rotation_limit

    def _afs_rotation_epoch(self, frame=None):
        """Each added bank change belongs to one explicitly asserted input."""
        sources=[d for d in getattr(self.board,'devices',())
                 if type(d).__name__=='AfsFrameSource']
        if not sources or len(sources)==1 and sources[0].request is None:
            return None
        from .afs_frame_source import requests,epoch_for
        if frame is not None:
            return epoch_for(self.board,frame)
        for request in reversed(requests(self.board)):
            if request['state'] in ('asserted','acknowledged'):
                from .afs_frame_source import MEASUREMENT_PROFILE, MEASUREMENT_FRAME_PROFILE, MEASUREMENT_NEXT_FRAME_PROFILE
                if (request['generation']==4 and
                        (request['profile'],request['frame']) in
                        ((MEASUREMENT_PROFILE,7),(MEASUREMENT_FRAME_PROFILE,8),(MEASUREMENT_NEXT_FRAME_PROFILE,9))):
                    continue
                if request['generation']!=3 or request['frame'] not in (5,6):
                    raise UnsupportedDevice('AF-S rotation epoch exceeds the two-frame experiment')
                return epoch_for(self.board,request['frame'])
        return None

    def _validate_afs_rotation_word(self, ordinal, step, core, tick, word):
        if self.rotation_limit not in (4,5) or ordinal not in (5,6):
            return
        # Before explicit AF admission, the card profile retains its original
        # fifth startup tuple law. Once admitted, every additional tuple must
        # belong to the actual independent AF edge, just as in ordinary REC.
        if self.rotation_limit==5 and not any(
                type(device).__name__=='AfsFrameSource' and device.request is not None
                for device in self.board.devices):
            return
        source=self._afs_rotation_epoch(ordinal)
        # Unchanged11F6008 alternates B/A after the initial B/A/B/A prefix.
        base=0x5fecd410 if ordinal==5 else 0x5fecaa60
        offsets=(0x850,0,0x1990,0x1d90,0x2190,0x21b0,0x23b0,0x1850,0x800)
        if (source is None or source['generation']!=3 or source['frame']!=ordinal
                or core!=source['cpu'] or tick<source['asserted_tick']
                or word!=base+offsets[step]):
            raise UnsupportedDevice('AF-S rotation requires its source epoch and exact stock alternating bank tuple')

    def _validate_rearm_rotations(self, snapshot=None):
        """Bind desired destination changes to their historical arm tuple.

        No transfer owns these new addresses yet. Each complete or partial
        group is retained in plain checkpoint state, with count524 unchanged.
        """
        r=self.rearm
        count=r.get('rotation_count');records=r.get('rotations')
        if (type(count) is not int or not 0<=count<=self._rotation_limit()
                or not isinstance(records,list) or len(records)!=count
                or type(r.get('rotation_started')) is not bool
                or r['rotation_started']!=(count>0)):
            raise UnsupportedDevice('Live producer rotation count/history marker is inconsistent')
        effective={address:event['word'] for address,event in
            zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])
            if address in self.REARM_ROTATION_ORDER}
        if records:
            g=r.get('geometry')
            if (r['state']!='enabled-awaiting-fresh-input'
                    or not isinstance(g,dict) or g.get('state')!='completed'
                    or type(g.get('step')) is not int or g['step']!=len(self.REARM_GEOMETRY_ORDER)
                    or type(g.get('completed_tick')) is not int or g['completed_tick']<0
                    or len(effective)!=len(self.REARM_ROTATION_ORDER)):
                raise UnsupportedDevice('Live producer rotation requires the fully enabled, geometry-configured fresh arm')
            previous_tick=g['completed_tick']
            snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
            for ordinal,record in enumerate(records,1):
                step=record.get('step') if isinstance(record,dict) else None
                prior=record.get('prior_words') if isinstance(record,dict) else None
                events=record.get('accesses') if isinstance(record,dict) else None
                if (type(step) is not int or not 1<=step<=len(self.REARM_ROTATION_ORDER)
                        or type(record.get('ordinal')) is not int or record['ordinal']!=ordinal
                        or record.get('state')!=('completed' if step==len(self.REARM_ROTATION_ORDER) else 'in-progress')
                        or step<len(self.REARM_ROTATION_ORDER) and ordinal!=count
                        or not isinstance(events,list) or len(events)!=step
                        or not isinstance(prior,dict) or set(prior)!={hex(a) for a in self.REARM_ROTATION_ORDER}
                        or any(not self._config_valid(a,prior[hex(a)]) for a in self.REARM_ROTATION_ORDER)
                        or any(prior[hex(a)]!=effective[a] for a in self.REARM_ROTATION_ORDER)
                        or type(record.get('started_tick')) is not int
                        or record['started_tick']<previous_tick):
                    raise UnsupportedDevice('Live producer rotation ordinal/step/state/prior tuple is inconsistent')
                initial={**self._known_configuration_prior(ordinal),**self._pre_image_parameter_prior(ordinal),
                         **prior,**self._image_global_prior(ordinal)}
                for key,digest in zip(('initial_configuration_sha256',
                                      'initial_retained_configuration_sha256'), snapshot.hashes(initial)):
                    if record.get(key)!=digest:
                        raise UnsupportedDevice('Live producer rotation initial configuration hash changed')
                event_tick=record['started_tick']
                for index,event in enumerate(events):
                    address=self.REARM_ROTATION_ORDER[index]
                    word=event.get('word') if isinstance(event,dict) else None
                    if (not self._config_valid(address,word)
                            or event.get('operation')!='write' or event.get('address')!=hex(address)
                            or event.get('value')!=hex(word) or event.get('old')!=hex(prior[hex(address)])
                            or type(event.get('step')) is not int or event['step']!=index
                            or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                            or type(event.get('tick')) is not int or event['tick']<event_tick
                            or index==0 and event['tick']!=record['started_tick']):
                        raise UnsupportedDevice('Live producer rotation store ledger differs from its nine-word ordered tuple')
                    self._validate_afs_rotation_word(ordinal,index,event['cpu'],event['tick'],word)
                    effective[address]=word
                    event_tick=event['tick']
                if (step==len(self.REARM_ROTATION_ORDER)
                        and (type(record.get('completed_tick')) is not int
                             or record['completed_tick']!=events[-1]['tick'])
                        or step<len(self.REARM_ROTATION_ORDER) and record.get('completed_tick') is not None):
                    raise UnsupportedDevice('Live producer rotation completion precedes its complete nine stores')
                previous_tick=event_tick
        stream=self.stream
        if stream is not None and (stream['rotations'] or stream['rotation_step']):
            # Opt-in stream: stock 11F6008 rotations are live and unbounded;
            # the finite ledger above is frozen at stream enable.
            return
        if any(type(self.registers[a]) is not int or self.registers[a]!=word
               for a,word in effective.items()):
            raise UnsupportedDevice('Live producer desired destinations differ from the original arm and rotation prefixes')

    def _validate_rearm_geometry(self, snapshot=None):
        """Bind a single post-arm update to its retained source and six stores."""
        r=self.rearm
        if 'geometry' not in r or type(r.get('geometry_started')) is not bool:
            raise UnsupportedDevice('Fresh producer geometry marker is missing or has unknown type')
        g=r['geometry']
        if g is None:
            if r['geometry_started']:
                raise UnsupportedDevice('Fresh producer geometry history was removed after its retained read')
            return
        if (not isinstance(g,dict) or r['geometry_started'] is not True
                or r['state']!='enabled-awaiting-fresh-input'):
            raise UnsupportedDevice('Post-arm geometry requires the fully enabled fresh request')
        step=g.get('step');events=g.get('accesses');prior=g.get('prior_words')
        if (type(step) is not int or not 0<=step<=len(self.REARM_GEOMETRY_ORDER)
                or g.get('state')!=('completed' if step==len(self.REARM_GEOMETRY_ORDER) else 'in-progress')
                or not isinstance(events,list) or len(events)!=step
                or not isinstance(prior,dict) or set(prior)!={hex(a) for a in self.REARM_GEOMETRY_ORDER}
                or any(not self._config_valid(a,prior[hex(a)]) for a in self.REARM_GEOMETRY_ORDER)):
            raise UnsupportedDevice('Post-arm geometry step/state/prior-word history is inconsistent')
        read=g.get('read')
        word=read.get('word') if isinstance(read,dict) else None
        if (type(word) is not int or not self._config_valid(self.BASE+0x15c,word)
                or read.get('operation')!='read' or read.get('address')!=hex(self.BASE+0x15c)
                or read.get('value')!=hex(word) or type(read.get('cpu')) is not int
                or read['cpu'] not in (0,1) or type(read.get('tick')) is not int or read['tick']<0
                or type(read.get('step')) is not int or read['step']!=0
                or type(g.get('started_tick')) is not int
                or read['tick']!=g.get('started_tick')
                or {**self._known_configuration_prior(),**self._pre_image_parameter_prior()}.get(hex(self.BASE+0x15c),
                    self.registers[self.BASE+0x15c])!=word):
            raise UnsupportedDevice('Post-arm geometry retained15C read no longer matches its source')
        for index,event in enumerate(events):
            address=self.REARM_GEOMETRY_ORDER[index]
            word=event.get('word') if isinstance(event,dict) else None
            if (type(word) is not int or not self._config_valid(address,word)
                    or event.get('operation')!='write' or event.get('address')!=hex(address)
                    or event.get('value')!=hex(word) or event.get('old')!=hex(prior[hex(address)])
                    or type(event.get('step')) is not int or event['step']!=index
                    or type(event.get('cpu')) is not int or event['cpu'] not in (0,1)
                    or type(event.get('tick')) is not int or event['tick']<read['tick']
                    or {**self._known_configuration_prior(),**self._pre_image_parameter_prior()}.get(hex(address),self.registers[address])!=word):
                raise UnsupportedDevice('Post-arm geometry store ledger differs from the stock ordered words')
        if any({**self._known_configuration_prior(),**self._pre_image_parameter_prior()}.get(hex(a),self.registers[a])!=prior[hex(a)]
                for a in self.REARM_GEOMETRY_ORDER[step:]):
            raise UnsupportedDevice('Post-arm geometry unwritten words changed outside its transaction')
        #Later desired destination rotations do not rewrite the arm that
        #preceded this geometry update. Restore that historical descriptor
        #as well as the six prior geometry words for its initial hash.
        initial={**self._known_configuration_prior(),**self._pre_image_parameter_prior(),**prior}
        initial.update({hex(a):event['word'] for a,event in
            zip(self.REARM_DESCRIPTOR_ORDER,r['descriptor_accesses'])})
        initial.update(self._image_global_prior())
        snapshot = _configuration_snapshot(self) if snapshot is None else snapshot
        for key,digest in zip(('initial_configuration_sha256',
                              'initial_retained_configuration_sha256'), snapshot.hashes(initial)):
            if g.get(key)!=digest:
                raise UnsupportedDevice('Post-arm geometry initial configuration hash changed')
        if (step==len(self.REARM_GEOMETRY_ORDER)
                and (type(g.get('completed_tick')) is not int or g['completed_tick']!=events[-1]['tick'])
                or step<len(self.REARM_GEOMETRY_ORDER) and g.get('completed_tick') is not None):
            raise UnsupportedDevice('Post-arm geometry completion record precedes its complete six stores')

    def _rearm_geometry_access(self,operation,core,address,size,value=None):
        """Retain one real15C read and six masked stores while awaiting input.

        The query and setter both read15C. Other retained reads remain possible;
        guest RAM tokens and caller PCs are not hardware admissibility gates.
        """
        r=self.rearm
        if r['state']!='enabled-awaiting-fresh-input':return False
        g=r['geometry']
        if operation=='read':
            if address!=self.BASE+0x15c:return False
            if g is None:
                word=self.registers[address]
                read=dict(operation='read',cpu=core,address=hex(address),value=hex(word),word=word,
                    tick=self.board.virtual_ticks,step=0)
                r['geometry']=dict(state='in-progress',step=0,read=read,accesses=[],
                    prior_words={hex(a):self.registers[a] for a in self.REARM_GEOMETRY_ORDER},
                    initial_configuration_sha256=r['configuration_sha256'],
                    initial_retained_configuration_sha256=r['retained_configuration_sha256'],
                    started_tick=self.board.virtual_ticks,completed_tick=None)
                r['geometry_started']=True
                self.board.record('Fresh producer post-arm geometry retained read',**read,
                    effects='Retained15C only; no readiness/processing/data/status/IRQ')
            return True
        if g is None:return False
        step=g['step']
        if (step>=len(self.REARM_GEOMETRY_ORDER) or address!=self.REARM_GEOMETRY_ORDER[step]
                or not self._config_valid(address,value)):
            raise UnsupportedDevice('Post-arm geometry requires one exact six-store masked transaction after15C read')
        old=self.registers[address]
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),tick=self.board.virtual_ticks,step=step)
        self.registers[address]=value
        g['accesses'].append(event)
        g['step']+=1
        if g['step']==len(self.REARM_GEOMETRY_ORDER):
            g.update(state='completed',completed_tick=self.board.virtual_ticks)
        r['configuration_sha256']=self._config_hash()
        r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer post-arm geometry retained store',**event,state=g['state'],
            effects='Current configuration only; awaiting new input, no recorded replay/data/status/IRQ/metadata/pixels')
        return True

    def _rearm_rotation_access(self,operation,core,address,size,value=None):
        """Retain a bounded mask3F destination tuple, with no data job.

        The driver writes unmasked words one at a time. Exact order and the
        selected three/four-group bound are emulator scope; in-flight bank latching and
        destination validity/transfer lengths are not inferred.
        """
        r=self.rearm
        g=r['geometry']
        if (r['state']!='enabled-awaiting-fresh-input'
                or g is None or g['state']!='completed' or operation=='read'):
            return False
        records=r['rotations']
        beginning=not records or records[-1]['state']=='completed'
        if beginning and address not in self.REARM_ROTATION_ORDER:return False
        if beginning:
            if r['rotation_count']>=self._rotation_limit():
                raise UnsupportedDevice('Live producer destination rotation budget is exhausted')
            step=0
        else:step=records[-1]['step']
        if (address!=self.REARM_ROTATION_ORDER[step] or not self._config_valid(address,value)):
            raise UnsupportedDevice('Live producer destinations require exactly nine ordered full-word stores; count524 is unchanged')
        previous_tick=(records[-1]['completed_tick'] if records else g['completed_tick']) if beginning else records[-1]['accesses'][-1]['tick']
        image_global=r['image_global_configuration']
        if beginning and image_global is not None:
            previous_tick=max(previous_tick,image_global['completed_tick'])
        known=r['known_configuration']
        if beginning and known is not None:
            previous_tick=max(previous_tick,known['latest_tick'])
        if (type(core) is not int or core not in (0,1)
                or type(self.board.virtual_ticks) is not int or self.board.virtual_ticks<previous_tick):
            raise UnsupportedDevice('Live producer destination store CPU/tick is not a typed, ordered hardware access')
        self._validate_afs_rotation_word(r['rotation_count']+1 if beginning else records[-1]['ordinal'],
                                         step,core,self.board.virtual_ticks,value)
        if beginning:
            record=dict(ordinal=r['rotation_count']+1,state='in-progress',step=0,accesses=[],
                prior_words={hex(a):self.registers[a] for a in self.REARM_ROTATION_ORDER},
                initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'],
                started_tick=self.board.virtual_ticks,completed_tick=None)
            records.append(record)
            r['rotation_count']+=1
            r['rotation_started']=True
        else:record=records[-1]
        old=self.registers[address]
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),tick=self.board.virtual_ticks,step=step)
        self.registers[address]=value
        record['accesses'].append(event)
        record['step']+=1
        if record['step']==len(self.REARM_ROTATION_ORDER):
            record.update(state='completed',completed_tick=self.board.virtual_ticks)
        r['configuration_sha256']=self._config_hash()
        r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer live destination retained store',**event,
            ordinal=record['ordinal'],state=record['state'],
            effects='Desired tuple only; historical arm/count/data ownership unchanged, no data/status/ACK/IRQ/metadata/pixels')
        return True

    def _rearm_pre_image_configuration_access(self,operation,core,address,size,value=None):
        if operation=='read' or address not in self.PRE_IMAGE_DESIRED_CONFIG:return False
        r=self.rearm;parent=r['pre_image_parameters']
        if (r['image_global_configuration'] is not None or parent is None or parent['state']!='completed'
                or len(parent['followups'])!=len(self.PRE_IMAGE_FOLLOWUP_ORDERS)
                or any(g['state']!='completed' for g in parent['followups'])):return False
        record=r['pre_image_configuration'];old=self.registers[address]
        if (r['rotations'] and r['rotations'][-1]['state']!='completed'
                or not self._known_pair_valid(address,old,value)
                or type(core) is not int or core!=parent['accesses'][0]['cpu']
                or type(self.board.virtual_ticks) is not int or not 0<=self.board.virtual_ticks<=0xffffffffffffffff
                or self.board.virtual_ticks<(record['latest_tick'] if record is not None
                    else parent['followups'][-1]['completed_tick'])
                or record is not None and record['stores']>=self.REARM_KNOWN_CONFIG_LIMIT):
            raise UnsupportedDevice('Pre-image configuration mask/owner/rotation/time or store budget is unsupported')
        if record is None:
            record=dict(state='retaining-desired-configuration',stores=0,accesses=[],prior_words={},
                started_tick=self.board.virtual_ticks,latest_tick=self.board.virtual_ticks,sealed_tick=None,
                initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'])
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),old_word=old,tick=self.board.virtual_ticks,step=record['stores'],
            after_rotation_count=r['rotation_count'])
        self.registers[address]=value;record['prior_words'].setdefault(hex(address),old)
        record['accesses'].append(event);record['stores']+=1;record['latest_tick']=self.board.virtual_ticks
        record['ledger_sha256']=self._known_ledger_hash(record['accesses'])
        r['pre_image_configuration']=record;r['pre_image_configuration_started']=True
        r['configuration_sha256']=self._config_hash();r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer pre-image configuration retained',**event,
            effects='Passive desired words only; earlier archives, job,data,status,ACK,IRQ and pixels unchanged')
        return True

    def _rearm_pre_image_followups_access(self,operation,core,address,size,value=None):
        if operation=='read':return False
        r=self.rearm;parent=r['pre_image_parameters']
        if parent is None or parent['state']!='completed':return False
        groups=parent['followups'];beginning=not groups or groups[-1]['state']=='completed'
        ordinal=len(groups) if beginning else len(groups)-1
        if ordinal>=len(self.PRE_IMAGE_FOLLOWUP_ORDERS):return False
        name,order=self.PRE_IMAGE_FOLLOWUP_ORDERS[ordinal]
        if beginning and address!=order[0]:return False
        if (r['image_global_configuration'] is not None or r['rotation_count']!=parent['after_rotation_count']
                or r['rotations'] and r['rotations'][-1]['state']!='completed'):
            if beginning:return False
            raise UnsupportedDevice('Pre-image followup cannot interleave image configuration or destination rotation')
        step=0 if beginning else groups[-1]['step']
        previous_tick=(parent['completed_tick'] if not groups else groups[-1]['accesses'][-1]['tick'])
        if (address!=order[step] or not self._config_valid(address,value)
                or type(core) is not int or core!=parent['accesses'][0]['cpu']
                or type(self.board.virtual_ticks) is not int or not previous_tick<=self.board.virtual_ticks<=0xffffffffffffffff):
            raise UnsupportedDevice('Pre-image followup requires its complete ordered masked stock family')
        if beginning:
            group=dict(family=name,state='in-progress',step=0,accesses=[],
                prior_words={hex(a):self.registers[a] for a in order},
                started_tick=self.board.virtual_ticks,completed_tick=None,
                initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'])
        else:group=groups[-1]
        old=self.registers[address]
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,old=hex(old),
            old_word=old,tick=self.board.virtual_ticks,step=step,after_rotation_count=parent['after_rotation_count'])
        self.registers[address]=value;group['accesses'].append(event);group['step']+=1
        group['ledger_sha256']=self._known_ledger_hash(group['accesses'])
        if group['step']==len(order):group.update(state='completed',completed_tick=self.board.virtual_ticks)
        if beginning:groups.append(group)
        parent['followups_started']=True
        r['configuration_sha256']=self._config_hash();r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer pre-image followup retained',**event,family=name,state=group['state'],
            effects='Desired configuration only; fourteen-store archive, historical arm/data,status,ACK,IRQ and pixels unchanged')
        return True

    def _rearm_pre_image_parameters_access(self,operation,core,address,size,value=None):
        """Retain one earlier packed-parameter family, without any job.

        No rotation is necessary for the complete stock geometry-first path;
        any existing rotation must still be complete and precede this family.
        """
        if operation=='read':return False
        r=self.rearm;record=r['pre_image_parameters'];beginning=record is None
        if beginning and address!=self.PRE_IMAGE_PARAMETER_ORDER[0]:return False
        if beginning and r['image_global_configuration'] is not None:return False
        if not beginning and record['state']=='completed':return False
        if (r['state']!='enabled-awaiting-fresh-input' or r['image_global_configuration'] is not None
                or not isinstance(r['geometry'],dict) or r['geometry']['state']!='completed'
                or r['rotations'] and r['rotations'][-1]['state']!='completed'):
            raise UnsupportedDevice('Pre-image parameters require completed geometry/rotation before image configuration')
        step=0 if beginning else record['step']
        if (address!=self.PRE_IMAGE_PARAMETER_ORDER[step] or not self._config_valid(address,value)
                or type(core) is not int or core not in (0,1)
                or not beginning and core!=record['accesses'][0]['cpu']
                or type(self.board.virtual_ticks) is not int or not 0<=self.board.virtual_ticks<=0xffffffffffffffff
                or self.board.virtual_ticks<((r['rotations'][-1]['completed_tick'] if r['rotations']
                    else r['geometry']['completed_tick']) if beginning
                    else record['accesses'][-1]['tick'])):
            raise UnsupportedDevice('Pre-image parameters require exactly fourteen ordered masked stores from one owner')
        if beginning:
            record=dict(state='in-progress',step=0,accesses=[],
                followups=[],followups_started=False,
                prior_words={hex(a):self.registers[a] for a in self.PRE_IMAGE_PARAMETER_ORDER},
                after_rotation_count=r['rotation_count'],started_tick=self.board.virtual_ticks,completed_tick=None,
                initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'])
        old=self.registers[address]
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,old=hex(old),
            old_word=old,tick=self.board.virtual_ticks,step=step,after_rotation_count=r['rotation_count'])
        self.registers[address]=value;record['accesses'].append(event);record['step']+=1
        record['ledger_sha256']=self._known_ledger_hash(record['accesses'])
        if record['step']==len(self.PRE_IMAGE_PARAMETER_ORDER):
            record.update(state='completed',completed_tick=self.board.virtual_ticks)
        r['pre_image_parameters']=record;r['pre_image_parameters_started']=True
        r['configuration_sha256']=self._config_hash();r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer pre-image parameters retained',**event,state=record['state'],
            effects='Desired configuration only; historical arm/data, status,ACK,IRQ and pixels unchanged')
        return True

    def _rearm_image_global_access(self,operation,core,address,size,value=None):
        """Retain10CC06C(mask4)'s complete933828/933930 triplet only."""
        if operation=='read':return False
        r=self.rearm;record=r['image_global_configuration']
        beginning=record is None
        if beginning and address not in self.IMAGE_GLOBAL_CONFIG_ORDER:return False
        if not beginning and record['state']=='completed':return False
        if (r['state']!='enabled-awaiting-fresh-input'
                or not isinstance(r['geometry'],dict) or r['geometry']['state']!='completed'
                or not r['rotations'] or r['rotations'][-1]['state']!='completed'):
            raise UnsupportedDevice('Image global configuration requires a fully enabled geometry and completed destination rotation')
        step=0 if beginning else record['step']
        if (address!=self.IMAGE_GLOBAL_CONFIG_ORDER[step] or not self._config_valid(address,value)
                or type(core) is not int or core not in (0,1)
                or type(self.board.virtual_ticks) is not int
                or self.board.virtual_ticks<(r['rotations'][-1]['completed_tick'] if beginning
                    else record['accesses'][-1]['tick'])):
            raise UnsupportedDevice('Image global configuration requires exactly ordered masked GLOBAL4/8/C stores')
        if beginning:
            record=dict(state='in-progress',step=0,accesses=[],
                prior_words={hex(a):self.registers[a] for a in self.IMAGE_GLOBAL_CONFIG_ORDER},
                after_rotation_count=r['rotation_count'],started_tick=self.board.virtual_ticks,
                completed_tick=None,initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'])
        old=self.registers[address]
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),tick=self.board.virtual_ticks,step=step)
        self.registers[address]=value
        record['accesses'].append(event);record['step']+=1
        if record['step']==3:record.update(state='completed',completed_tick=self.board.virtual_ticks)
        phase=r['pre_image_configuration']
        if beginning and phase is not None:phase.update(state='sealed',sealed_tick=self.board.virtual_ticks)
        r['image_global_configuration']=record;r['image_global_started']=True
        r['configuration_sha256']=self._config_hash()
        r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer image global configuration retained',**event,
            state=record['state'],after_rotation_count=record['after_rotation_count'],
            effects='Desired3-word configuration only; prior job,globalcontrol,DMA,status,ACK,data,IRQ and pixels unchanged')
        return True

    def _rearm_known_configuration_access(self,operation,core,address,size,value=None):
        """Retain existing, mask-proven fields after the completed triplet."""
        if operation=='read' or address not in self.KNOWN_DESIRED_CONFIG:return False
        r=self.rearm;image=r['image_global_configuration']
        if image is None or image['state']!='completed':return False
        if not r['rotations'] or r['rotations'][-1]['state']!='completed':
            raise UnsupportedDevice('Known producer configuration cannot interleave an incomplete rotation')
        record=r['known_configuration'];old=self.registers[address]
        if (not self._known_pair_valid(address,old,value) or type(core) is not int or core not in (0,1)
                or type(self.board.virtual_ticks) is not int or not 0<=self.board.virtual_ticks<=0xffffffffffffffff
                or self.board.virtual_ticks<max(image['completed_tick'],r['rotations'][-1]['completed_tick'],
                    record['latest_tick'] if record is not None else 0)
                or record is not None and record['stores']>=self.REARM_KNOWN_CONFIG_LIMIT):
            raise UnsupportedDevice('Known producer configuration bits/domain, desired control, time or store budget is unsupported')
        if record is None:
            record=dict(state='retaining-desired-configuration',stores=0,accesses=[],prior_words={},
                started_tick=self.board.virtual_ticks,latest_tick=self.board.virtual_ticks,
                initial_configuration_sha256=r['configuration_sha256'],
                initial_retained_configuration_sha256=r['retained_configuration_sha256'])
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),old_word=old,tick=self.board.virtual_ticks,step=record['stores'],
            after_rotation_count=r['rotation_count'])
        self.registers[address]=value
        record['prior_words'].setdefault(hex(address),old)
        record['accesses'].append(event);record['stores']+=1;record['latest_tick']=self.board.virtual_ticks
        record['ledger_sha256']=self._known_ledger_hash(record['accesses'])
        r['known_configuration']=record;r['known_configuration_started']=True
        if address==self.GLOBAL:r['global_control']=value
        r['configuration_sha256']=self._config_hash();r['retained_configuration_sha256']=self._retained_config_hash()
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer known configuration retained',**event,
            effects='Desired words only; original job, DMA, data, status,ACK,IRQ and pixels unchanged')
        return True

    def _rearm_access(self,operation,core,address,size,value=None):
        """Retain one fresh descriptor/arm after the complete released shutdown.

        This request owns separate, synchronously checkpointable ledgers. It
        cannot reuse the first recorded result or trigger another input job.
        """
        r=self.rearm
        beginning=r is None
        descriptor_addresses=frozenset(self.REARM_DESCRIPTOR_ORDER)
        shutdown_complete=(isinstance(self.released_shutdown,dict)
                           and self.released_shutdown.get('state')=='completed')
        if beginning and (operation!='write' or address not in descriptor_addresses
                          or not shutdown_complete):return False
        try:
            if beginning:
                history=self._rearm_history_hash()
                if (address!=self.REARM_DESCRIPTOR_ORDER[0] or not self._config_valid(address,value)
                        or not self._released_stop_idle() or self.busy is not False
                        or self.pending_data is not None or self.pending_ack is not None
                        or not self._global_valid(self.registers[self.GLOBAL])
                        or any(type(self.registers.get(a)) is not int or self.registers[a]!=v
                            for a,v in ((self.BASE+0x8a0,0x80003000),(self.BASE+0x8c4,2)))):
                    raise UnsupportedDevice('Fresh producer descriptor requires completed shutdown and its first508 store')
                # Validate the entire retained domain before changing any latch.
                legacy,retained=self._config_hash(),self._retained_config_hash()
                r=dict(state='descriptor-rebuild',descriptor_step=0,setup_step=0,enable_step=0,
                    descriptor_accesses=[],setup_accesses=[],enable_accesses=[],route=None,
                    configuration_sha256=legacy,retained_configuration_sha256=retained,
                    original_history_sha256=history,global_control=self.registers[self.GLOBAL],
                    started_tick=self.board.virtual_ticks,armed_tick=None,enabled_tick=None,
                    geometry=None,geometry_started=False,
                    rotations=[],rotation_count=0,rotation_started=False,
                    image_global_configuration=None,image_global_started=False,
                    pre_image_parameters=None,pre_image_parameters_started=False,
                    pre_image_configuration=None,pre_image_configuration_started=False,
                    known_configuration=None,known_configuration_started=False,
                    effects='Fresh guest configuration only; awaiting fresh input, no recorded replay/data/status/IRQ/metadata/pixels')
            else:self._validate_rearm_access()
            if not beginning and self._rearm_pre_image_configuration_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_pre_image_followups_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_pre_image_parameters_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_known_configuration_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_image_global_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_rotation_access(operation,core,address,size,value):return True
            if not beginning and self._rearm_geometry_access(operation,core,address,size,value):return True
            if operation=='read':
                if address in self.ACK_REGISTERS or address in (self.BASE+0x890,self.BASE+0x990):
                    raise UnsupportedDevice('Fresh producer result/acknowledgment/idle status awaits an unmodeled new input')
                return False
            if r['descriptor_step']<len(self.REARM_DESCRIPTOR_ORDER):
                name='descriptor';step=r['descriptor_step']
                expected=self.REARM_DESCRIPTOR_ORDER[step]
                if address!=expected or not self._config_valid(address,value):
                    raise UnsupportedDevice('Fresh producer descriptor requires the exact nine-address/count store order')
            elif r['setup_step']<len(self.DMA_SETUP):
                name='setup';step=r['setup_step']
                if (address,value)!=self.DMA_SETUP[step]:
                    raise UnsupportedDevice('Fresh producer initialization requires the exact seven stock arm writes')
                route=self._dma_route() if step>=5 else None
            elif r['enable_step']<len(self.REARM_ENABLE):
                name='enable';step=r['enable_step']
                if (address,value)!=self.REARM_ENABLE[step]:
                    raise UnsupportedDevice('Fresh producer enable requires33173317 followed by secondary zero')
            else:
                raise UnsupportedDevice('Fresh producer arm awaits a new input; further activation/configuration/results are unresolved')
        except (UnsupportedDevice,KeyError,TypeError,ValueError,OverflowError,struct.error) as error:
            self._reject(operation,core,address,size,value,reason=str(error))
        old=self.registers[address]
        self.registers[address]=value
        event=dict(operation='write',cpu=core,address=hex(address),value=hex(value),word=value,
            old=hex(old),tick=self.board.virtual_ticks,step=step)
        r[name+'_accesses'].append(event)
        r[name+'_step']+=1
        if name=='setup' and step>=5:r['route']=route
        r['configuration_sha256']=self._config_hash()
        r['retained_configuration_sha256']=self._retained_config_hash()
        r['state']=self._rearm_state(r['descriptor_step'],r['setup_step'],r['enable_step'])
        if r['setup_step']==len(self.DMA_SETUP) and r['armed_tick'] is None:
            r['armed_tick']=self.board.virtual_ticks
        if r['enable_step']==len(self.REARM_ENABLE):r['enabled_tick']=self.board.virtual_ticks
        self.rearm=r
        self.writes.append(dict(cpu=core,address=hex(address),value=hex(value),old=hex(old)))
        self.board.record('Fresh producer descriptor/arm retained',**event,state=r['state'],
            effects=r['effects'])
        return True

    def read(self, core, address, size):
        self._check('read', core, address, size)
        if hasattr(self, 'processing_pipeline_reset'):
            from . import processing_pipeline_reset
            return processing_pipeline_reset.access(self,'read',core,address,size)
        if hasattr(self, 'processing_control_configuration'):
            from . import processing_control_configuration
            return processing_control_configuration.access(self,'read',core,address,size)
        if hasattr(self, 'processing_passive_configuration'):
            from . import processing_passive_configuration
            return processing_passive_configuration.access(self,'read',core,address,size)
        if hasattr(self, 'processing_configuration'):
            from . import producer_processing_configuration
            return producer_processing_configuration.access(self,'read',core,address,size)
        if hasattr(self, 'processing_reset'):
            from . import producer_processing_reset
            return producer_processing_reset.access(self,'read',core,address,size)[1]
        if self.stream is not None and producer_stream.access(self,'read',core,address,size):return self.registers[address]
        if producer_still_capture.access(self,'read',core,address,size):return self.registers[address]
        if producer_capture_metstats.access(self,'read',core,address,size):return self.registers[address]
        if producer_afs_focus.access(self,'read',core,address,size):return self.registers[address]
        if producer_measurement.access(self,'read',core,address,size):return self.registers[address]
        if afs_statistics_frame.access(self,'read',core,address,size):return self.registers[address]
        if self._fresh_shutdown_access('read',core,address,size):return self.registers[address]
        if self._histogram_access('read',core,address,size):return self.registers[address]
        if self._released_shutdown_access('read',core,address,size):return self.registers[address]
        if self._rearm_access('read',core,address,size):return self.registers[address]
        if self.data_profile is not None and address in self.ACK_REGISTERS:
            return self._ack_read(core,address,size)
        if (self.data_profile is not None and address==self.BASE+0x890
                and self.last_data is not None and self.last_data['state']=='consumed-once'
                and self.data_release_step==1 and not self.busy and self.registers[address]==0):
            if self._retained_config_hash()!=self.last_data['retained_configuration_sha256']:
                self._reject('read',core,address,size,
                    reason='Statistics retained configuration changed before idle release observation')
            self.data_idle_observed=True
            return 0
        value = self.registers[address]
        if address in self.ALL_CONFIG_MASKS:
            #524 is an explicit zero initial fixture until its setter runs.
            if address == self.BASE+0x524 and value == 0 and self.dma_setup_step == 0:
                return value
            if not self._config_valid(address, value):
                self._reject('read', core, address, size, reason='Unmodeled producer configuration state')
        elif address in (self.BASE+4,self.BASE+8):
            controls=(0,self.PRODUCER_SELECT_MASK)
            if self.data_profile is not None and self.last_data is not None and self.last_data['state'] in ('consumed-once','released-after-recorded-input'):
                controls=(*controls,self.PRODUCER_DISABLED_MASK)
            if (address == self.BASE+4 and value not in controls) or (address == self.BASE+8 and value != 0):
                self._reject('read',core,address,size,reason='Unmodeled producer desired enable state')
        elif address in (self.BASE+0x88c,self.BASE+0x8f4):
            self._reject('read',core,address,size,
                reason='Producer acknowledgment status requires a completed functional data job')
        elif address == self.BASE+0x884:
            if value not in (0,1) or (value == 1 and self.dma_arm is None):
                self._reject('read',core,address,size,reason='Unmodeled producer channel0 arm state')
        elif self.dma_arm is not None and address in (self.BASE+0x890,self.BASE+0x990):
            self._reject('read',core,address,size,
                reason='Armed producer idle status requires a functional quiescence contract')
        elif address == self.GLOBAL:
            if not self._global_valid(value):
                self._reject('read', core, address, size, reason='Unmodeled producer control/status state')
        elif value != 0:
            self._reject('read', core, address, size, reason='Active producer/DMA status is unresolved')
        return value

    def write(self, core, address, size, value):
        self._check('write', core, address, size, value)
        if not isinstance(value, int) or isinstance(value, bool) or not 0 <= value <= 0xffffffff:
            self._reject('write', core, address, size, value, reason='Producer word exceeds32 bits')
        if hasattr(self, 'processing_pipeline_reset'):
            from . import processing_pipeline_reset
            processing_pipeline_reset.access(self,'write',core,address,size,value)
            return
        if hasattr(self, 'processing_control_configuration'):
            from . import processing_control_configuration
            processing_control_configuration.access(self,'write',core,address,size,value)
            return
        if hasattr(self, 'processing_passive_configuration'):
            from . import processing_passive_configuration
            processing_passive_configuration.access(self,'write',core,address,size,value)
            return
        if hasattr(self, 'processing_configuration'):
            from . import producer_processing_configuration
            producer_processing_configuration.access(self,'write',core,address,size,value)
            return
        if hasattr(self, 'processing_reset'):
            from . import producer_processing_reset
            producer_processing_reset.access(self,'write',core,address,size,value)
            return
        if self.stream is not None and producer_stream.access(self,'write',core,address,size,value):return
        if producer_still_capture.access(self,'write',core,address,size,value):return
        if producer_capture_metstats.access(self,'write',core,address,size,value):return
        if producer_afs_focus.access(self,'write',core,address,size,value):return
        if producer_measurement.access(self,'write',core,address,size,value):return
        if afs_statistics_frame.access(self,'write',core,address,size,value):return
        if self._fresh_shutdown_access('write',core,address,size,value):return
        if self._histogram_access('write',core,address,size,value):return
        if self._released_shutdown_access('write',core,address,size,value):return
        if self._rearm_access('write',core,address,size,value):return
        if self.data_profile is not None and self.last_data is not None and address in self.ACK_REGISTERS:
            self._ack_write(core,address,size,value)
            return
        if self.data_profile is not None and address in (self.BASE+0x98c,self.BASE+0x9f4):
            self._reject('write',core,address,size,value,reason='Channel1 statistics acknowledgment requires delivered recorded bytes')
        if self.busy:
            self._reject('write', core, address, size, value,
                reason='Producer mutation during pending stop acknowledgment')
        old = self.registers[address]
        if address in self.DMA_REGISTERS and (address != self.BASE+0x884 or value != 0 or self.dma_setup_step):
            self._dma_write(core,address,size,value)
            request = None
        elif address in (self.BASE+4,self.BASE+8):
            self._producer_enable_write(core,address,size,value)
            request = None
        elif address in self.ALL_CONFIG_MASKS:
            idle_configuration = self._idle() and (not self.dma_setup_step or self._data_released())
            input_configuration = self._awaiting_input_configuration()
            if not self._config_valid(address, value) or not (idle_configuration or input_configuration):
                self._reject('write', core, address, size, value,
                    reason='Unknown producer configuration bits/domain or DMA initialization/arm')
            request = None
            if address in self.DESTINATIONS:self.destination_writes.add(address)
        elif address == self.GLOBAL and value == 0:
            if self.stop_profile != STOP_PROFILE:
                self._reject('write', core, address, size, value,
                    reason='Producer stop bit28 acknowledgment is not modeled')
            if (not self._global_valid(old)
                    or old not in (0,0x10000000) and not self._released_stop_idle()
                    or value != 0 or not self._idle()
                    or self.dma_setup_step and not self._data_released()):
                self._reject('write', core, address, size, value,
                    reason='Only exact global zero stop with already-idle producer/DMA is modeled')
            request = dict(cpu=core, profile=self.stop_profile, old=hex(old), request=hex(value),
                requested_tick=self.board.virtual_ticks,
                due_tick=self.board.virtual_ticks+self.stop_ticks,
                configuration_sha256=self._config_hash(),
                retained_configuration_sha256=self._retained_config_hash(),state='waiting')
            if old not in (0,0x10000000):request['released_one_shot']=True
        elif address == self.GLOBAL:
            delta = old^value
            idle_configuration = self._idle() and (not self.dma_setup_step or self._data_released())
            if (not self._global_valid(old) or not self._global_valid(value)
                    or not (idle_configuration or self._awaiting_input_configuration()) or delta&0x10000000
                    or not any(delta&~field == 0 for field in self.GLOBAL_DESIRED_FIELDS)):
                self._reject('write',core,address,size,value,
                    reason='Only maskD4 single-family desired-control changes with retained stop status are modeled')
            request = None
            self.desired_control_writes.append(dict(cpu=core,old=hex(old),value=hex(value),
                desired_fields='2..3/8..9/16..18 only',
                effects='No processing, transfer, measurement, completion or IRQ follows activation flags'))
        else:
            if old != 0 or value != 0 or not self._idle() or self.dma_setup_step:
                self._reject('write', core, address, size, value,
                    reason='Only already-idle producer/DMA zero stops and bit0 clears are modeled')
            request = None
            if address in (self.BASE+0x884, self.BASE+0x984):
                self.idle_channel_stops[(address-self.BASE-0x884)//0x100] += 1
        self.registers[address] = value
        event = dict(cpu=core, address=hex(address), value=hex(value), old=hex(old))
        self.writes.append(event)
        self.board.record('Producer configuration retained or stop requested', **event)
        if request is not None:
            self.busy = True
            self.pending_stop = request
            self.last_stop = dict(request)
            self.stop_requests += 1
            self.board.record('Producer global stop acknowledgment requested (unmeasured)', **request,
                inference='Only exact cleared zero word then delayed bit28 set; no DMA/data/completion/IRQ')

    def tick(self, board):
        if hasattr(self, 'processing_reset'):
            from . import producer_processing_reset
            producer_processing_reset.tick(self,board)
            return
        if self.stream is not None:producer_stream.tick(self,board)
        if type(self.rearm) is dict and 'still_capture' in self.rearm:
            producer_still_capture.tick(self,board)
            return
        if type(self.rearm) is dict and 'capture_metstats' in self.rearm:
            producer_capture_metstats.tick(self,board)
            return
        if type(self.rearm) is dict and 'afs_focus' in self.rearm:
            producer_afs_focus.tick(self,board)
            return
        if producer_measurement.tick(self,board):return
        try:
            if afs_statistics_frame.tick(self,board):return
        except (UnsupportedDevice,KeyError,TypeError,ValueError,OverflowError,struct.error) as error:
            self._reject('tick',None,self.BASE,4,reason=str(error))
        if preview_statistics.tick(self,board):return
        if self._tick_post_shutdown_reset(board):return
        # Passive latches cannot advance with time. Reconstruct their complete
        # ownership/configuration ledgers at consuming MMIO and active ticks,
        # rather than using every idle scheduler quantum as a host-state audit.
        h=self.histogram
        fresh_closed=(self.stop_profile=='stock-idle-stop-handshakes-unmeasured'
            and type(self.fresh_shutdown) is dict
            and self.fresh_shutdown.get('state')=='completed'
            and type(self.fresh_shutdown.get('step')) is int
            and self.fresh_shutdown['step']==len(self.FRESH_SHUTDOWN_SEQUENCE))
        histogram_idle=(self.histogram_profile is None or h is None
            or self._histogram_timerless_idle() or (
            self.histogram_profile in HISTOGRAM_PROFILES and type(h) is dict
            and h.get('profile')==self.histogram_profile
            and (h.get('state')=='enabled' or h.get('state')=='released' and fresh_closed)
            and type(h.get('samples')) is list and len(h['samples'])==hp.MAX_SAMPLES
            and type(h.get('deliveries')) is list and len(h['deliveries'])==hp.MAX_SAMPLES
            and all(type(d) is dict and d.get('state')=='acknowledged'
                    and type(d.get('ack_channel')) is int and d['ack_channel']==2
                    and type(d.get('ack_step')) is int and d['ack_step']==0
                    and d.get('ack_due_tick') is None and d.get('ack_requested_tick') is None
                    for d in h['deliveries'])))
        if (self.busy is False and self.dma_busy is False and self.pending_data is None
                and self.pending_ack is None and self.pending_stop is None and histogram_idle):
            return
        # Validate the rearm ledgers on a tick only when this tick can act:
        # pending work cannot change before its due tick, and stock briefly
        # masks a whole IRQ bank while unregistering another source, which is
        # not a changed route (see _tick_histogram). Every consuming MMIO
        # access still validates; X70_EMULATOR_AUDIT=1 re-audits every tick.
        validate=self.rearm is not None and (strict_audit.ENABLED or self.stream is not None)
        if self.rearm is not None and not validate:
            try:
                due=self.next_event_tick(board.virtual_ticks)
                validate=due is not None and due<=board.virtual_ticks
            except (UnsupportedDevice,KeyError,TypeError,ValueError,OverflowError):
                validate=True  # malformed pending records: let the audit reject them
        if validate:
            try:self._validate_rearm()
            except (UnsupportedDevice,KeyError,TypeError,ValueError,OverflowError,struct.error) as error:
                self._reject('tick',None,self.BASE,4,reason=str(error))
        if self.data_profile is not None:
            self._tick_data(board)
        self._tick_histogram(board.virtual_ticks)
        if self.pending_stop is None:
            return
        request = self.pending_stop
        if (request is None or type(self.registers[self.GLOBAL]) is not int
                or self.registers[self.GLOBAL] != 0 or not self._idle()
                or self._config_hash() != request['configuration_sha256']
                or self._retained_config_hash()!=request['retained_configuration_sha256']
                or request.get('released_one_shot') and not self._released_stop_idle(allow_pending_stop=True)):
            self._reject('tick', None, self.GLOBAL, 4,
                reason='Producer pending stop or idle fixture changed outside exact request')
        if board.virtual_ticks < request['due_tick']:
            return
        self.registers[self.GLOBAL] = 0x10000000
        self.busy = False
        self.pending_stop = None
        self.stop_acknowledgments += 1
        self.last_stop = dict(request, state='acknowledged', completed_tick=board.virtual_ticks,
            acknowledged=hex(self.registers[self.GLOBAL]))
        board.record('Producer global stop acknowledged (unmeasured)', **self.last_stop,
            inference='Bit28 only; physical latency/status origin unmeasured, no producer input/data/IRQ')

    def next_event_tick(self, now):
        if hasattr(self, 'processing_reset'):
            from . import producer_processing_reset
            return producer_processing_reset.due(self)
        due=self._next_event_tick_strict(now)
        stream_due=producer_stream.due(self) if self.stream is not None else None
        if stream_due is None:return due
        return stream_due if due is None else min(due,stream_due)

    def _next_event_tick_strict(self, now):
        if type(self.rearm) is dict and 'still_capture' in self.rearm:
            return producer_still_capture.due(self)
        if type(self.rearm) is dict and 'capture_metstats' in self.rearm:
            return producer_capture_metstats.due(self)
        if type(self.rearm) is dict and 'afs_focus' in self.rearm:
            return producer_afs_focus.due(self)
        times=[request['due_tick'] for request in (self.pending_stop,self.pending_data,self.pending_ack)
               if request is not None]
        measurement_due=producer_measurement.due(self)
        if measurement_due is not None:times.append(measurement_due)
        reset_due=self._post_shutdown_reset_due()
        if reset_due is not None:times.append(reset_due)
        preview_due=preview_statistics.due(self)
        if preview_due is not None:times.append(preview_due)
        afs_due=afs_statistics_frame.due(self)
        if afs_due is not None:times.append(afs_due)
        histogram_due=self._histogram_next_tick()
        if histogram_due is not None:times.append(histogram_due)
        return min(times) if times else None

    def arm_measurement(self,profile=producer_measurement.PROFILE):
        return producer_measurement.arm(self,profile)

    def arm_afs_focus(self,profile=producer_afs_focus.PROFILE):
        return producer_afs_focus.arm(self,profile)

    def arm_capture_metstats(self,profile=producer_capture_metstats.PROFILE):
        return producer_capture_metstats.arm(self,profile)

    def arm_still_capture(self,profile=producer_still_capture.PROFILE):
        return producer_still_capture.arm(self,profile)

    def arm_processing_reset(self,profile):
        from . import producer_processing_reset
        return producer_processing_reset.arm(self,profile)

    def processing_reset_summary(self):
        from . import producer_processing_reset
        return producer_processing_reset.summary(self)

    def arm_processing_configuration(self,profile,*,caller_address=None):
        from . import producer_processing_configuration
        return producer_processing_configuration.arm(self,profile,caller_address=caller_address)

    def processing_configuration_summary(self):
        from . import producer_processing_configuration
        return producer_processing_configuration.summary(self)

    def still_capture_summary(self):
        return producer_still_capture.summary(self)

    def still_capture_source_anchor(self):
        return producer_still_capture.source_anchor(self)

    def arm_still_statistics(self,profile=None,*,scene,delay_ticks=1024):
        from . import afs_still_numeric
        return afs_still_numeric.arm(self,profile=profile,scene=scene,delay_ticks=delay_ticks)

    def still_statistics_summary(self):
        from . import afs_still_numeric
        return afs_still_numeric.summary(self)

    def still_statistics_anchor(self):
        from . import afs_still_numeric
        return afs_still_numeric.anchor(self)

    def capture_metstats_summary(self):
        return producer_capture_metstats.summary(producer_still_capture.historical_capture_view(self))

    def capture_metstats_source_anchor(self):
        return producer_capture_metstats.source_anchor(self)

    def arm_capture_metstats_data(self,profile=producer_capture_metstats.DATA_PROFILE):
        return producer_capture_metstats.arm_data(self,profile)

    def capture_metstats_data_anchor(self,sample=1):
        return producer_capture_metstats.data_anchor(self,sample)

    def afs_focus_source_anchor(self,expected=None):
        return producer_afs_focus.source_anchor(self,expected)

    def afs_focus_summary(self):
        return producer_afs_focus.summary(self)

    def arm_afs_focus_statistics(self,profile=producer_afs_focus.afs_focus_data.PROFILE):
        if profile != producer_afs_focus.afs_focus_data.PROFILE:
            raise UnsupportedDevice('First AF data API requires the first-pair profile')
        return producer_afs_focus.afs_focus_data.arm(self,profile)

    def arm_next_afs_focus_statistics(self,profile=producer_afs_focus.afs_focus_data.CONTINUATION_PROFILE):
        return producer_afs_focus.afs_focus_data.arm_next(self,profile)

    def afs_focus_statistics_summary(self):
        predecessor=producer_still_capture.historical_capture_view(self)
        return producer_afs_focus.afs_focus_data.summary(producer_capture_metstats.historical_af_view(predecessor))

    def afs_focus_data_anchor(self,pair_id=1):
        return producer_afs_focus.afs_focus_data.anchor(self,pair_id)

    def measurement_source_anchor(self,expected=None):
        return producer_measurement.source_anchor(self,expected)

    def arm_measurement_statistics(self,profile=producer_measurement.afs_measurement_data.PROFILE):
        return producer_measurement.afs_measurement_data.arm(self,profile)

    def arm_next_measurement_statistics(self,profile=producer_measurement.afs_measurement_data.CONTINUATION_PROFILE):
        return producer_measurement.afs_measurement_data.arm_next(self,profile)

    def measurement_data_anchor(self,frame=8):
        return producer_measurement.afs_measurement_data.anchor(self,frame)

    def measurement_retirement_anchor(self):
        return producer_measurement.retirement_anchor(self)

    def rearm_records(self):
        return producer_measurement.records(self)

    def measurement_summary(self):
        owner=self
        if type(self.rearm) is dict and 'afs_focus' in self.rearm:
            producer_afs_focus.validate(self)
            owner=producer_afs_focus._terminal_view(self,self.rearm['afs_focus'])
        record=producer_measurement.validate(owner)
        if record is None:return None
        request=record['request']
        data=record.get('data')
        from .afs_frame_source import epoch_for
        return dict(profile=record['profile'],state='admitted-awaiting-source' if request is None else request['state'],
            frame=7,access_count=len(record['accesses']),access_step=0 if request is None else request['step'],
            armed_tick=record['armed_tick'],enabled_tick=None if request is None else request['enabled_tick'],
            due_tick=producer_measurement.afs_measurement_data.due(record),
            ack_channel=None if data is None else data['ack_channel'],source_epoch=epoch_for(self.board,7),
            data=None if data is None else dict(profile=data['profile'],state=data['state'],
                source_epoch=data['source_epoch'],completed_tick=data['completed_tick'],
                acknowledged_tick=data['acknowledged_tick'],spans=len(data['latch']['plan']),
                geometry={hex(address):word for address,word in data['latch']['geometry'].items()}),
            rotation_step=0 if record.get('rotation') is None else record['rotation']['step'],
            continuation_rotation_step=0 if record.get('continuation_rotation') is None else record['continuation_rotation']['step'],
            continuation_configuration_count=len(record.get('continuation_configuration',[])),
            data_lifetimes=len(record.get('data_history',[]))+(data is not None),
            late_access_count=len(record.get('late_configuration',[])))

    def snapshot(self):
        if any(type(d).__name__ == 'ImageWorkControl' and hasattr(d, 'processing_activation')
               for d in self.board.devices):
            from . import image_processing_activation
            image = image_processing_activation.image_for_producer(self)
            result = type(self).snapshot(image_processing_activation.historical(self))
            result['processing_image_activation'] = image_processing_activation.summary(image)
            return result
        result = dict(name=type(self).__name__, base=hex(self.BASE), global_control=hex(self.GLOBAL),
            stream=(dict(self.stream, frame=asdict(self.stream['frame']))
                    if self.stream is not None and type(self.stream.get('frame')) is producer_stream.StatisticsFrame
                    else self.stream),
            registers={hex(address):hex(value) for address,value in self.registers.items()},
            stop_profile=self.stop_profile, stop_ticks=self.stop_ticks, busy=self.busy,
            pending_stop=self.pending_stop, last_stop=self.last_stop,
            stop_requests=self.stop_requests, stop_acknowledgments=self.stop_acknowledgments,
            desired_control=self.desired_control(),desired_control_writes=self.desired_control_writes,
            configuration_hash_domains=dict(
                legacy=dict(words=len(self.CONFIG_MASKS),sha256=self._config_hash(),
                    scope='CONFIG_MASKS original domain pinned by recorded-input b59 fixture'),
                retained=dict(words=len(self.ALL_CONFIG_MASKS),sha256=self._retained_config_hash(),
                    scope='Legacy fields plus13 packed parameters,2 mode words,576 raw LUT words and5 additional global parameters; full current transaction integrity'),
                raw_lut=dict(words=len(self.RAW_LUT_CONFIG_MASKS),
                    addresses='FFF29100..FFF299FC',mask='0xffffffff',readback='unsupported',
                    scope='93464C nine64-WORD desired lookup windows; no processing effect'),
                parameter_bank=dict(words=len(self.PARAMETER_CONFIG_MASKS),
                    masks={hex(a):hex(mask) for a,mask in self.PARAMETER_CONFIG_MASKS.items()},
                    scope='160..190 full-word passive packed fields; initial zero unmeasured'),
                mode_bank=dict(words=len(self.MODE_CONFIG_MASKS),
                    masks={hex(a):hex(mask) for a,mask in self.MODE_CONFIG_MASKS.items()},
                    scope='308 mode0..5,310 ten-bit value; initial zero unmeasured')),
            destination_writes=[hex(a) for a in sorted(self.destination_writes)],
            dma_setup_step=self.dma_setup_step,dma_arm=self.dma_arm,
            producer_enable_writes=self.producer_enable_writes,
            data_profile=self.data_profile,data_ticks=self.data_ticks,ack_ticks=self.ack_ticks,
            rotation_limit=self.rotation_limit,
            histogram_profile=self.histogram_profile,histogram_data_ticks=self.histogram_data_ticks,
            histogram=self.histogram,dma_busy=self.dma_busy,
            data_fixture=None if self.data_fixture is None else self.data_fixture.as_report(),
            pending_data=self.pending_data,last_data=self.last_data,pending_ack=self.pending_ack,
            data_jobs=self.data_jobs,data_transfers=self.data_transfers,data_bytes=self.data_bytes,data_irqs=self.data_irqs,
            ack_requests=self.ack_requests,ack_completions=self.ack_completions,
            data_release_step=self.data_release_step,data_idle_observed=self.data_idle_observed,
            data_disable_secondary=self.data_disable_secondary,
            released_shutdown=self.released_shutdown,rearm=self.rearm,fresh_shutdown=self.fresh_shutdown,
            post_shutdown_control=self.post_shutdown_control,
            preview_idle_stop=self.preview_idle_stop,
            preview_configuration=self.preview_configuration,
            idle_channel_stops=self.idle_channel_stops, writes=self.writes, last_rejected=self.last_rejected,
            limits=([
                'Continuous statistics latch the guest destination tuple and sampling program on physical virtual-source edges; scene bytes are DMA-written before the selected producer IRQ and guest paired acknowledgment.',
                'X-M1 uses its own IRQ2E handler at67BFDC, U32 contrast/channel grids, ten eight-byte contrast slots, ten four-slot channel records, packed U16 phase grids, totals and row/column arrays.',
                'Analytic scene quadrature, 14-bit population weighting, contrast filters, phase splitting, unresolved row/column semantics, transferred extents, cadence and acknowledgment latency are declared unmeasured laws.',
                'Synthetic statistics establish no captured-frame equivalence, autofocus accuracy, exposure accuracy, sensor/optics calibration or physical timing.'
            ] if self.stream is not None and self.stream['profile'] is producer_stream.ProducerStreamProfile.XM1 else [
                'The selected scene supplies bytes to source-latched destination windows before IRQ2E and the legacy guest paired acknowledgment.',
                'Scene laws, transferred extents, cadence and acknowledgment latency are unmeasured.'
            ] if self.stream is not None else [
                'Initial zero controls and already-idle DMA statuses are explicit unmeasured fixtures.',
                'Optional exact global zero stop delays only bit28 set; physical meaning/latency are unmeasured.',
                'After an acknowledged, disabled, zero-status one-shot release only, that stop may also clear valid retained desired modes. Release/arm/data history is preserved; no new result or physical quiescence is inferred.',
                'Pending stop rejects all guest writes and host fixture mutation; busy refuses checkpoint capture.',
                'Both idle DMA stops retain zero without a transfer, acknowledgment IRQ or output.',
                'Proven10CC06C/10CBB44 mask7F0 and10CB618 maskD4 retain bounded guest geometry/coordinate-list/arrays/LUT configuration.',
                '92D5F0/92DB08 and92DC54/92DD30 retain13 full-word parameters160..190 using stock packed masks. Initial zero is explicitly unmeasured; no readiness, status, processing, DMA, data or IRQ follows these fields.',
                '92E574/92E630 retain308 mode0..5 and310 ten-bit unsigned parameter through full-word accesses only; no processing/data/status/IRQ follows these desired fields.',
                'Legacy CONFIG_MASKS/_config_hash domain stays pinned for the first recorded-input fixture. Full retained hashes include parameters, raw LUTs and extra global fields for current integrity; first recorded input additionally requires all those added fields to retain explicit unmeasured-zero fixtures.',
                '10CB5B0 maskD4 desired unit modes2..3/8..9/16..18 retain prior bit28 only; positive modes supply no processing or success effect.',
                'The six large LUT windows and nine raw93464C windows1100..19FC are write-only because getter/readback semantics are unproven; retained bytes do not imply processing.',
                '10CAEA0/92F584 channel0 retains nine unmasked address words, packed count and exact ordered arm latches; armed-awaiting-input is capturable, not an in-flight transfer.',
                '92CD04 maskFF retains desired enable/select word33173317 and secondary zero without data/completion/IRQ; destination validity/lengths remain a future data-job requirement.',
                'Default FFF188F4 retains the initialization request1; no clear acknowledgment, active-idle status, metadata, payload or producer interrupt is supplied.',
                'Separate explicitly selected captured-rec28/output-under-startup50 experiment supplies one22-span foreign-memory projection beforeIRQ2E and delayed paired-channel acknowledgments; source mode/configuration mismatch and opaque51C span remain declared, not a faithful sensor/ISP model.',
                'After that one-shot is consumed and acknowledged, exact33170000 disable and84-clear/890-idle/F8-clear permit guest DMA release; no additional output is implied.',
                'After its later declared global stop is acknowledged, one complete ten-access92CC5C shutdown may retain already-zero words in exact order. Partial step/hash/history are checkpointed; original arm/setup/data/release records persist, with no new job or repeat allowance.',
                'After that completed shutdown, one fresh descriptor in508/500/510/514/518/51C/520/50C/504/524 order, seven92F584 arm writes and33173317/secondary-zero desired enables may be retained. Separate descriptor/setup/enable ledgers, current legacy/full hashes and first-history fingerprint are checkpointed; original dma_arm/setup/data/release records are untouched. It awaits new input and cannot schedule the old recorded fixture again.',
                'A fresh arm supplies no new result, F4 acknowledgment clear, idle status, data, IRQ, metadata or pixels. Further configuration/activation and repeat arms remain unsupported until their complete stock transactions are established.',
                'The fully enabled fresh request may retain one post-arm geometry transaction: first retained15C read (query/setter reads both remain available), then six full-word stores100/104/108/300/304/30C in stock order with existing packed masks. Prior words/read/store ledgers bind initial and current hashes, preserve first-job history and supply no new processing/readiness/status/data/IRQ/metadata/pixels.',
                f'After that geometry update, at most {self.rotation_limit} live desired destination groups may retain nine unmasked WORDs in508/500/510/514/518/51C/520/50C/504 order. No524 store occurs; count, original arm descriptor, geometry initial hashes and first recorded-job history remain historical. Separate chained prior/current tuple hashes and plain partial ledgers are checkpointable. The group bound/order are emulator scope; silicon address validity, in-flight bank-latching timing and transfer ownership remain unmeasured. No data job, payload/status/ACK/IRQ/metadata/pixels follows these stores.',
                'One later10CC06C(mask4) image configuration may retain exactly GLOBAL4/8/C after completed geometry and a complete rotation. A separate prior/current ledger reconstructs earlier geometry/rotation hashes; later rotations use new globals. Partial stores reject interleaved writes. No globalcontrol, DMA, data result, status, acknowledgment, IRQ or pixels are changed; active-engine latching remains unmeasured.',
                'After the complete global triplet, at most8192 known desired parameter/list/LUT stores and single-family GLOBALmode/configuration updates retain a plain insertion-rotation/prior/current ledger. Later geometry/15C/triplet updates reconstruct historical values from the prior ledger while actual getters read current words. Initial ordered transactions and their hash anchors remain historical; later rotations use current fields. Destinations/count, DMA, ACK and completion registers are excluded. All desiredGLOBALupdates preserve bit28 and other families. No new data, status, acknowledgment, IRQ, metadata or pixels are produced.',
                'Other activation/configuration, active DMA/status/quiescence, getter-only outputs58/5C, completion strobes, IRQs, metadata and pixels remain unsupported.',
                'Unknown addresses, widths, bits and states reject; no function result/task/queue/callback is supplied.']))
        if hasattr(self, 'processing_reset'):
            result['processing_reset'] = self.processing_reset_summary()
        if hasattr(self, 'processing_configuration'):
            result['processing_configuration'] = self.processing_configuration_summary()
        if hasattr(self, 'processing_passive_configuration'):
            result['processing_passive_configuration'] = self.processing_passive_configuration_summary()
        if hasattr(self, 'processing_control_configuration'):
            result['processing_control_configuration'] = self.processing_control_configuration_summary()
        if hasattr(self, 'processing_pipeline_reset'):
            result['processing_pipeline_reset'] = self.processing_pipeline_reset_summary()
        return result

    def arm_processing_passive_configuration(self,profile,*,caller_address=None):
        from . import processing_passive_configuration
        return processing_passive_configuration.arm(self,profile,caller_address=caller_address)

    def processing_passive_configuration_summary(self):
        from . import processing_passive_configuration
        return processing_passive_configuration.summary(self)

    def arm_processing_control_configuration(self,profile,*,caller_address=None):
        from . import processing_control_configuration
        return processing_control_configuration.arm(self,profile,caller_address=caller_address)

    def processing_control_configuration_summary(self):
        from . import processing_control_configuration
        return processing_control_configuration.summary(self)

    def arm_processing_pipeline_reset(self,profile):
        from . import processing_pipeline_reset
        return processing_pipeline_reset.arm(self,profile)

    def processing_pipeline_reset_summary(self):
        from . import processing_pipeline_reset
        return processing_pipeline_reset.summary(self)
