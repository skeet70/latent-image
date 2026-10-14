"""Retained FFF19000 image-processing configuration, without processing.

The exact 56-descriptor/96-word family is copied by 939CD8/939D20 and
10CD8B4. Masks are constructed software fields from the stock packers,
assuming zero reserved shadow bits; physical implemented bits are unknown.
Control flags are retained desired configuration, not a working ISP.
"""
import hashlib
import struct

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
