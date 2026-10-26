"""Retained image configuration and an opt-in declared UI CPU conversion.

The exact 56-descriptor/96-word family is copied by 939CD8/939D20 and
10CD8B4. Masks are constructed software fields from the stock packers,
assuming zero reserved shadow bits; physical implemented bits are unknown.
Control flags are retained desired configuration, not a working ISP.

FFF47000 is absent by default. Its declared profile implements the stock
YYCbCr-to-ABGR CPU fallback contract, not measured silicon behavior.
"""
import hashlib
import struct
from dataclasses import dataclass, fields, replace
from enum import Enum, IntEnum

from .hardware import UnsupportedDevice


class ImageParameterControl:
    BASE = 0xfff19000
    MASKS = {
        0x000:0x0111113f, 0x004:0xd1511110, 0x008:0x00003131,
        0x100:0x00010001, 0x104:0x00ff007f, 0x108:0xffff1fff,
        0x10c:0x3fffffff, 0x110:0x01ff01ff, 0x114:0x00ff01ff,
        0x118:0x3fff3fff, **{o:0xffff3fff for o in range(0x11c,0x12c,4)},
        0x12c:0x00007fff, 0x200:0x7fff7fff, 0x204:0x00007fff,
        0x208:0x000f0f0f, 0x20c:0xffffffff, 0x210:0xffff0707,
        0x300:0x3f3f3f3f, **{o:0x1fff1fff for o in range(0x304,0x330,4)},
        0x400:0x1fff1fff, 0x404:0x00001fff,
        0x408:0x1fff1fff, 0x40c:0x00001fff,
        0x410:0x1fff1fff, 0x414:0x00001fff,
        0x500:0x77777777, 0x504:0x00000007,
        **{o:0x3fff3fff for o in range(0x508,0x544,4)},
        0x600:0x000f0f0f, 0x604:0x1fff1fff, 0x608:0x1fff1fff,
        0x60c:0xffffffff, **{o:0x03ff00ff for o in range(0x610,0x624,4)},
        0x624:0x03ff03ff, 0x628:0x000007ff, 0x62c:0x1fff1fff,
        0x630:0x03ff03ff, 0x634:0x000007ff, 0x638:0x1fff1fff,
        0x63c:0x001f001f, 0x640:0xffffffff,
        **{o:0x03ff07ff for o in range(0x644,0x658,4)},
        0x700:0x3f3f3f3f,
        **{o:0x03ff03ff for o in range(0x800,0x818,4)},
        0x818:0x00ffffff, 0x81c:0x3f3f3f3f, 0x820:0x0000ffff,
        0x824:0xffffffff, 0x828:0x0000ffff, 0x82c:0x000000ff,
        0x830:0x1f1f1f1f, 0x834:0x00001f1f, 0x838:0x003f00ff,
        0x900:0x00ffffff, 0x904:0x0f0f00ff, 0x908:0x00ffffff,
    }

    def __init__(self, board):
        self.board = board
        self.registers = {offset:0 for offset in self.MASKS}
        self.writes = []
        self.reads = 0
        self.last_rejected = None
        board.record('Image parameter zero configuration fixture', base=hex(self.BASE),
            words=len(self.MASKS),
            evidence='939C24 packers;939CD8/939D20 canonical56 descriptors;10CD8B4 selected copies',
            limits='Initial zeros and physical implemented bits unmeasured; retained desired configuration only, no processing, acquisition, DMA, IRQ, status or pixels')

    def contains(self, address, size):
        return self.BASE <= address < self.BASE+0x1000

    def _reject(self, operation, core, address, size, value=None,
                reason='Unknown image parameter register or width'):
        self.last_rejected = dict(operation=operation, cpu=core, address=hex(address),
            size=size, value=hex(value) if isinstance(value,int) else repr(value), reason=reason)
        self.board.record('Image parameter request rejected', **self.last_rejected)
        raise UnsupportedDevice(f'{reason}: {address:#x}/{size}')

    def _valid(self, offset, value):
        if isinstance(value,bool) or not isinstance(value,int) or not 0 <= value <= 0xffffffff:
            return False
        if value & ~self.MASKS[offset]:
            return False
        if offset == 0:
            return (value & 0x3f) <= 0x2d
        if offset == 8:
            return ((value >> 12) & 3) <= 2
        if offset == 0x104:
            return (value & 0x7f) <= 0x51
        if offset == 0x110:
            return (value & 0x1ff) <= 0x100 and ((value >> 16) & 0x1ff) <= 0x100
        if offset == 0x114:
            return (value & 0x1ff) <= 0x100
        if offset == 0x300:
            return all(((value >> bit) & 0x3f) <= 0x20 for bit in (0,8,16))
        if offset == 0x900:
            return (value & 0xff) <= 0x80 and ((value >> 8) & 0xff) <= 0x80
        return True

    def _offset(self, operation, core, address, size, value=None):
        offset = address-self.BASE
        if size != 4 or offset not in self.MASKS:
            self._reject(operation, core, address, size, value)
        return offset

    def read(self, core, address, size):
        offset = self._offset('read',core,address,size)
        value = self.registers[offset]
        if not self._valid(offset,value):
            self._reject('read',core,address,size,value,
                reason='Image parameter fixture has unknown reserved bits/domain')
        self.reads += 1
        return value

    def write(self, core, address, size, value):
        offset = self._offset('write',core,address,size,value)
        if not self._valid(offset,value):
            self._reject('write',core,address,size,value,
                reason='Image parameter word outside constructed mask/domain')
        old = self.registers[offset]
        self.registers[offset] = value
        event = dict(cpu=core,address=hex(address),old=hex(old),value=hex(value))
        self.writes.append(event)
        self.board.record('Image parameter configuration retained',**event,
            effects='Configuration/readback only; physical submodule activation and processing are unresolved')

    def configuration_hash(self):
        return hashlib.sha256(b''.join(struct.pack('<II',o,self.registers[o])
            for o in sorted(self.MASKS))).hexdigest()

    def control_fields(self):
        a,b,c = (self.registers[o] for o in (0,4,8))
        return dict(mode=a&0x3f,
            word0_flags={str(bit):bool(a&(1<<bit)) for bit in (8,12,16,20,24)},
            word4_flags={str(bit):bool(b&(1<<bit)) for bit in (4,8,12,16,20,22,24,28)},
            word4_enum30=(b>>30)&3,
            word8_flags={str(bit):bool(c&(1<<bit)) for bit in (0,8)},
            word8_enum4=(c>>4)&3,word8_enum12=(c>>12)&3,
            meaning='Desired stock fields retained; unit names and actual activation/processing effects unresolved')

    def snapshot(self):
        return dict(name=type(self).__name__,base=hex(self.BASE),
            registers={hex(self.BASE+o):hex(v) for o,v in self.registers.items()},
            configuration_sha256=self.configuration_hash(),control_fields=self.control_fields(),
            reads=self.reads,writes=self.writes,last_rejected=self.last_rejected,
            limits=['Only96 canonical word destinations support constructed mask/domain retention.',
                'Initial zero configuration and physical implemented bits are unmeasured.',
                'Reserved shadow bits must be zero; stock packers preserve them rather than proving hardware behavior.',
                'Control flags can affect physical processing; this model records desired fields without processing/output effects.',
                'Unknown status, gaps, start/output operations, widths, bits and domains reject.',
                'No acquisition, DMA, IRQ, metadata, pixels, callbacks, queues or guest results are supplied.'])


class UIConversionProfile(str, Enum):
    DECLARED_UI_CPU_FALLBACK = 'declared-ui-cpu-fallback'

    def __str__(self):
        return self.value

    @classmethod
    def parse(cls, value):
        if value is None or type(value) is cls:
            return value
        if type(value) is not str:
            raise ValueError('UI conversion requires an explicit named profile')
        return cls(value)


class UIConversionCommand(IntEnum):
    IDLE = 0
    START = 3


class UIConversionPhase(str, Enum):
    IDLE = 'idle'
    PROGRAMMING = 'programming'
    ACTIVE = 'active'
    COMPLETED_LATCHED = 'completed-latched'
    ACKNOWLEDGED = 'acknowledged'


class UIConversionMask(IntEnum):
    DISABLED = 0
    COMPLETION = 0x1301


class UIConversionRegister(IntEnum):
    CONTROL = 0x00
    COMMAND = 0x04
    SELECTOR = 0x08
    SOURCE = 0x10
    SOURCE_STRIDE = 0x14
    SOURCE_GEOMETRY = 0x18
    DESTINATION = 0x20
    DESTINATION_STRIDE = 0x24
    DESTINATION_GEOMETRY = 0x28
    WINDOW = 0x2c
    AUXILIARY = 0x70
    STATUS = 0x74
    ACKNOWLEDGE = 0x78
    MASK = 0x7c
    ALPHA = 0x80


class UIConversionSelector(IntEnum):
    SOFTWARE_BLOCK_32 = 1


class UIConversionZero(IntEnum):
    ZERO = 0


class UIConversionAlpha(IntEnum):
    OPAQUE = 255


class UIConversionCompletion(IntEnum):
    SUCCESS = 1


class UIOriginCore(IntEnum):
    CPU0 = 0
    CPU1 = 1


@dataclass(frozen=True)
class UIPhysicalAddress:
    value: int

    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 1 << 32 or self.value % 16:
            raise ValueError('UI physical address must be an aligned 32-bit offset')


@dataclass(frozen=True)
class UIPixelStride:
    value: int

    def __post_init__(self):
        if type(self.value) is not int or not 0 < self.value < 1 << 15 or self.value % 16:
            raise ValueError('UI pixel stride must be positive, 16-aligned and 15-bit')


@dataclass(frozen=True)
class UIPixelGeometry:
    width: int
    height: int

    def __post_init__(self):
        if (type(self.width) is not int or type(self.height) is not int
                or not 0 < self.width < 1 << 13 or self.width % 16
                or not 0 < self.height < 1 << 13 or self.height % 8):
            raise ValueError('UI geometry requires 13-bit positive 16/8-aligned dimensions')

    @classmethod
    def from_word(cls, word):
        return cls(word & 0xffff, word >> 16)


@dataclass(frozen=True)
class UIVirtualTick:
    value: int

    def __post_init__(self):
        if type(self.value) is not int or not 0 <= self.value < 1 << 64:
            raise ValueError('UI deadline must be a bounded virtual tick')


@dataclass(frozen=True)
class UIConversionProgramming:
    selector: UIConversionSelector | None = None
    source: UIPhysicalAddress | None = None
    source_stride: UIPixelStride | None = None
    source_geometry: UIPixelGeometry | None = None
    destination: UIPhysicalAddress | None = None
    destination_stride: UIPixelStride | None = None
    destination_geometry: UIPixelGeometry | None = None
    window: UIConversionZero | None = None
    auxiliary: UIConversionZero | None = None
    mask: UIConversionMask | None = None
    alpha: UIConversionAlpha | None = None

    def validate(self, *, complete=False):
        expected = (UIConversionSelector, UIPhysicalAddress, UIPixelStride, UIPixelGeometry,
                    UIPhysicalAddress, UIPixelStride, UIPixelGeometry, UIConversionZero,
                    UIConversionZero, UIConversionMask, UIConversionAlpha)
        for descriptor, cls in zip(fields(self), expected):
            value = getattr(self, descriptor.name)
            if value is None:
                if complete:
                    raise ValueError('UI conversion programming generation is incomplete')
            elif type(value) is not cls:
                raise ValueError('UI conversion programming field type differs')
            elif hasattr(value, '__post_init__'):
                value.__post_init__()
        if complete and (self.source_geometry != self.destination_geometry
                or self.destination_stride.value != self.source_geometry.width
                or self.source_stride.value < self.source_geometry.width):
            raise ValueError('UI conversion requires identity geometry and bounded row strides')


class DeclaredUIConversion:
    """Single-command, one-tick CPU fallback; ACTIVE jobs cannot be captured."""

    BASE = 0xfff47000
    IRQ = 67
    MAX_OUTPUT_BYTES = 64 << 20
    quiet_ticks_defer = True
    PROGRAMMING_WORDS = {
        UIConversionRegister.SELECTOR: ('selector', UIConversionSelector),
        UIConversionRegister.SOURCE: ('source', UIPhysicalAddress),
        UIConversionRegister.SOURCE_STRIDE: ('source_stride', UIPixelStride),
        UIConversionRegister.SOURCE_GEOMETRY: ('source_geometry', UIPixelGeometry.from_word),
        UIConversionRegister.DESTINATION: ('destination', UIPhysicalAddress),
        UIConversionRegister.DESTINATION_STRIDE: ('destination_stride', UIPixelStride),
        UIConversionRegister.DESTINATION_GEOMETRY: ('destination_geometry', UIPixelGeometry.from_word),
        UIConversionRegister.WINDOW: ('window', UIConversionZero),
        UIConversionRegister.AUXILIARY: ('auxiliary', UIConversionZero),
        UIConversionRegister.MASK: ('mask', UIConversionMask),
        UIConversionRegister.ALPHA: ('alpha', UIConversionAlpha),
    }

    def __init__(self, board, *, profile, scene_profile=None):
        self.board = board
        self.profile = UIConversionProfile.parse(profile)
        if self.profile is None:
            raise ValueError('Declared UI conversion requires explicit selection')
        self.scene_profile, self.scene_state = scene_profile, None
        self.phase = UIConversionPhase.IDLE
        self.programming = UIConversionProgramming()
        self.reset_seen = False
        self.deadline = None
        self.origin = None
        self.route = None
        self.failed = False
        self.plan = None

    @property
    def busy(self):
        from x70_native._native import scene_scaler_busy
        return self.phase is UIConversionPhase.ACTIVE or scene_scaler_busy(self)

    def contains(self, address, size):
        return self.BASE <= address < self.BASE + 0x1000

    def _reject(self, reason):
        raise UnsupportedDevice('Declared UI conversion: ' + reason)

    def _register(self, address, size):
        if type(size) is not int or size != 4 or type(address) is not int:
            self._reject('only exact aligned 32-bit words are supported')
        try:
            return UIConversionRegister(address - self.BASE)
        except ValueError:
            self._reject('unobserved register')

    def _prepare(self, programming):
        programming.validate(complete=True)
        geometry = programming.source_geometry
        if 4 * geometry.width * geometry.height > self.MAX_OUTPUT_BYTES:
            raise ValueError('UI output exceeds the declared 64 MiB limit')
        return self.board.prepare_declared_ui_conversion(
            programming.source.value, programming.destination.value,
            geometry.width, geometry.height, programming.source_stride.value)

    def read(self, core, address, size):
        from x70_native._native import scene_scaler_read
        value = scene_scaler_read(self, core, address, size)
        if value is not None:
            return value
        register = self._register(address, size)
        if (register is not UIConversionRegister.STATUS
                or self.phase is not UIConversionPhase.COMPLETED_LATCHED):
            self._reject('status is readable only after complete output publication')
        return UIConversionCompletion.SUCCESS.value

    def write(self, core, address, size, value):
        from x70_native._native import scene_scaler_write
        if scene_scaler_write(self, core, address, size, value):
            return
        register = self._register(address, size)
        if type(value) is not int or not 0 <= value < 1 << 32:
            self._reject('word value is not an unsigned 32-bit integer')
        if self.phase is UIConversionPhase.ACTIVE:
            self._reject('active programming cannot change')
        if register is UIConversionRegister.CONTROL:
            try:
                command = UIConversionCommand(value)
            except ValueError:
                self._reject('unobserved control word')
            if command is not UIConversionCommand.IDLE:
                self._reject('control requires idle zero')
            if (self.phase not in (UIConversionPhase.IDLE, UIConversionPhase.ACKNOWLEDGED)
                    or self.phase is UIConversionPhase.ACKNOWLEDGED
                    and self.programming.mask is not UIConversionMask.DISABLED):
                self._reject('reset requires idle or acknowledged mask-zero cleanup')
            self.phase = UIConversionPhase.IDLE
            self.programming = UIConversionProgramming()
            self.reset_seen = True
            self.deadline = self.origin = self.route = None
            self.failed = False
            self.plan = None
            return
        if register is UIConversionRegister.COMMAND:
            if value != UIConversionCommand.START or self.phase is not UIConversionPhase.PROGRAMMING:
                self._reject('start requires one fresh complete programming generation')
            try:
                if self.programming.mask is not UIConversionMask.COMPLETION:
                    raise ValueError('start requires the whole completion mask')
                if type(core) is not int or core not in (0, 1):
                    raise ValueError('originating core differs')
                origin = UIOriginCore(core)
                deadline = UIVirtualTick(self.board.virtual_ticks + 1)
                route = self.board.interrupts.declared_ui_irq_target()
                plan = self._prepare(self.programming)
            except Exception as error:
                self._reject(str(error))
            self.origin, self.route, self.deadline = origin, route, deadline
            self.plan = plan
            self.phase = UIConversionPhase.ACTIVE
            return
        if register is UIConversionRegister.ACKNOWLEDGE:
            if (value != UIConversionCompletion.SUCCESS
                    or self.phase is not UIConversionPhase.COMPLETED_LATCHED):
                self._reject('acknowledgement requires the singleton completed latch')
            self.phase = UIConversionPhase.ACKNOWLEDGED
            return
        if self.phase is UIConversionPhase.ACKNOWLEDGED:
            if register is not UIConversionRegister.MASK or value != UIConversionMask.DISABLED:
                self._reject('acknowledged cleanup requires mask zero')
            self.programming = replace(self.programming, mask=UIConversionMask.DISABLED)
            return
        if self.phase is UIConversionPhase.COMPLETED_LATCHED:
            self._reject('completed programming requires singleton acknowledgement')
        if register not in self.PROGRAMMING_WORDS:
            self._reject('unobserved programming word')
        if (self.phase is UIConversionPhase.IDLE and register is UIConversionRegister.MASK
                and value == UIConversionMask.DISABLED):
            return
        if not self.reset_seen:
            self._reject('programming requires an idle-zero generation')
        name, parse = self.PROGRAMMING_WORDS[register]
        try:
            parsed = parse(value)
        except ValueError as error:
            self._reject(str(error))
        self.programming = replace(self.programming, **{name: parsed})
        self.phase = UIConversionPhase.PROGRAMMING

    def next_event_tick(self, now):
        from x70_native._native import scene_scaler_next
        if self.scene_state is not None:
            return scene_scaler_next(self, now)
        return max(now, self.deadline.value) if self.busy and not self.failed else None

    def tick(self, board):
        from x70_native._native import scene_scaler_tick
        if scene_scaler_tick(self, board):
            return
        if not self.busy:
            return
        if self.failed:
            self._reject('active model failure is frozen')
        if board.virtual_ticks < self.deadline.value:
            return
        try:
            if board.interrupts.declared_ui_irq_target() is not self.route:
                self._reject('IRQ67 route changed after arm')
            board.apply_declared_ui_conversion(self.plan, cpu_id=self.origin.value)
            self.plan = None
            self.phase = UIConversionPhase.COMPLETED_LATCHED
            board.queue_irq(self.route.value, self.IRQ)
        except Exception as error:
            self.phase = UIConversionPhase.ACTIVE
            self.failed = True
            self.plan = None
            self._reject(str(error))
        self.deadline = None

    def validate_capture_state(self, state):
        from .interrupts import DeclaredUIIRQTarget
        from x70_native._native import scene_scaler_validate
        if set(state) != set(vars(self)) or state.get('board') is not self.board:
            raise ValueError('UI conversion device field schema differs')
        scene_scaler_validate(self, state)
        if (state['profile'] is not UIConversionProfile.DECLARED_UI_CPU_FALLBACK
                or type(state['phase']) is not UIConversionPhase
                or type(state['programming']) is not UIConversionProgramming
                or type(state['reset_seen']) is not bool or type(state['failed']) is not bool):
            raise ValueError('UI conversion device state types differ')
        phase, programming = state['phase'], state['programming']
        if phase is UIConversionPhase.ACTIVE or state['failed'] or state['deadline'] is not None:
            raise ValueError('UI conversion ACTIVE capture is unsupported')
        if state['plan'] is not None:
            raise ValueError('UI conversion capture cannot retain a native plan')
        complete = phase in (UIConversionPhase.COMPLETED_LATCHED, UIConversionPhase.ACKNOWLEDGED)
        programming.validate(complete=complete)
        if phase is UIConversionPhase.IDLE:
            if programming != UIConversionProgramming():
                raise ValueError('Idle UI conversion retains programming')
        elif not state['reset_seen']:
            raise ValueError('UI conversion programming lacks a reset generation')
        if phase is UIConversionPhase.PROGRAMMING and programming == UIConversionProgramming():
            raise ValueError('UI programming generation has no configured words')
        if complete:
            if (type(state['origin']) is not UIOriginCore
                    or type(state['route']) is not DeclaredUIIRQTarget
                    or phase is UIConversionPhase.COMPLETED_LATCHED
                    and programming.mask is not UIConversionMask.COMPLETION):
                raise ValueError('UI completed metadata differs')
            self._prepare(programming)
        elif state['origin'] is not None or state['route'] is not None:
            raise ValueError('Inactive UI conversion retains active metadata')

    def snapshot(self):
        from x70_native._native import scene_scaler_snapshot
        scene = scene_scaler_snapshot(self)
        if scene is not None:
            return scene
        return dict(name=type(self).__name__, base=hex(self.BASE), phase=self.phase.value,
            busy=self.busy, failed=self.failed,
            deadline=None if self.deadline is None else self.deadline.value,
            originating_core=None if self.origin is None else self.origin.value,
            irq_target=None if self.route is None else self.route.value,
            programming={f.name: (None if (v := getattr(self.programming, f.name)) is None
                else v.value if isinstance(v, IntEnum) else dict(vars(v)))
                for f in fields(self.programming)},
            provenance=dict(profile='xm1-150-declared-ui-yycbcr-to-abgr-cpu-fallback-unmeasured',
                selection=self.profile.value,
                contract='Stock CPU fallback decimal-integer truncation, YYCbCr to opaque ABGR',
                completion='Live source at first completion boundary after a one-tick deadline; output before latch1 and one IRQ67 request',
                limits=['Exact observed aligned32-bit words and command3 only',
                    'Identity geometry; width16/height8/stride16 alignment; bounded disjoint DRAM',
                    '64 MiB output limit; ACTIVE capture and active model-failure retry refused',
                    'Arithmetic, timing, status, IRQ electrical behavior and physical Rename fidelity are unmeasured']))
