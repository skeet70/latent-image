"""Bounded NAND, memory/peripheral DMA and Fuji LZ engine models.

These implement transactions observed in stock instruction sequences, not a
complete SoC. Unknown commands stop execution. NAND spare/ECC bytes and the
unpopulated flash tail are explicit ideal-device fixtures; DAT does not contain
physical spare bytes. Transfer timing uses deterministic virtual ticks, not
measured hardware cycles. See research/emulator-loader/README.md for evidence.
Legacy X70 caller addresses below scope the original model contracts. X-M1
profiles supply target flash bytes and an explicit bounded audio-TX sink.
"""

from pathlib import Path
import hashlib
import json
from dataclasses import dataclass

from .hardware import UnsupportedDevice
from .nand_image import export_image, restore_image
from x70_native._native import DeclaredMovrszPlan, MovrszProfile

DAT_SHA256 = "7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6"
MOVRSZ_PROFILE = 'declared-packed-yycbcr-pyramid-unmeasured'
MOVRSZ_SCENE_PROFILE = 'xm1-150-declared-movrsz-scene-identity-unmeasured'
MOVRSZ_PROFILES = {MOVRSZ_PROFILE: MovrszProfile.Pyramid,
                   MOVRSZ_SCENE_PROFILE: MovrszProfile.SceneIdentity}


def native_flash_from_dat(path):
    """Return every native flash byte in the supplied, hash-checked X70 DAT.

    The final 972 bytes matter: stock boot reads metadata at flash 0x32a0000.
    """
    data = Path(path).read_bytes()
    if hashlib.sha256(data).hexdigest() != DAT_SHA256:
        raise ValueError("Native flash model requires the supplied X70 1.10 DAT")
    return data[0x25c:].translate(bytes(255 - n for n in range(256)))


class _Registers:
    def __init__(self, *, read_bytes, write_bytes, schedule, record=None):
        self.read_bytes, self.write_bytes = read_bytes, write_bytes
        self.schedule = schedule
        self.record = record if record is not None else lambda *a, **kw: None
        self.regs = {}
        self.jobs_started = 0
        self.jobs_completed = 0
        self.last_job = None

    def _offset(self, address, size):
        offset = address - self.BASE
        if size != 4 or offset & 3:
            raise UnsupportedDevice(f"{type(self).__name__}: unsupported access {address:#x}/{size}")
        if offset not in self.OFFSETS:
            raise UnsupportedDevice(f"{type(self).__name__}: unknown register {address:#x}")
        return offset

    def read(self, cpu_id, address, size):
        return self.regs.get(self._offset(address, size), 0)

    def snapshot(self):
        return {"device": type(self).__name__,
                "registers": {hex(self.BASE + offset): hex(value)
                              for offset, value in sorted(self.regs.items())},
                "jobs_started": self.jobs_started, "jobs_completed": self.jobs_completed,
                "busy": self.busy, "last_job": self.last_job}


_NATIVE_FEED = False


def _native_feed():
    """The native Fuji LZ decoder, or None where the extension is absent.

    Imported on first use, like the other native helpers, because
    x70_native imports this package.
    """
    global _NATIVE_FEED
    if _NATIVE_FEED is False:
        try:
            from x70_native._native import fuji_lz_feed
        except ImportError:
            fuji_lz_feed = None
        _NATIVE_FEED = fuji_lz_feed
    return _NATIVE_FEED


class FujiLZStream:
    """Method-4 tokens, retaining match history and split tokens across jobs."""
    def __init__(self):
        self.history = bytearray()
        self.pending = bytearray()

    def feed(self, source):
        native = _native_feed()
        if native is not None and type(self.history) is bytearray and type(self.pending) is bytearray:
            # Same token walk in Rust; identical result and state, also on a
            # malformed token (tests/test_fuji_lz_feed.py compares both).
            return native(self.history, self.pending, source)
        return self._feed_python(source)

    def _feed_python(self, source):
        pending, history = self.pending, self.history
        pending.extend(source)
        end = len(pending)
        position, start = 0, len(history)
        size = start
        extend = history.extend
        while position < end:
            token = pending[position]
            if token == 0:
                raise ValueError("Zero-length Fuji literal")
            if token < 0x80:
                stop = position + 1 + token
                if stop > end:
                    break
                extend(pending[position + 1:stop])
                size += token
                position = stop
            else:
                if position + 2 > end:
                    break
                second = pending[position + 1]
                distance = ((token & 0x7f) << 4) | (second >> 4)
                length = second & 15
                if not 0 < distance <= size or not length:
                    raise ValueError("Invalid Fuji LZ match")
                match_start = size - distance
                match = history[match_start:match_start + length]
                if length > distance:
                    # Overlap repeats the original distance-byte seed.
                    match = (match * ((length + distance - 1) // distance))[:length]
                extend(match)
                size += length
                position += 2
        del pending[:position]
        return bytes(history[start:])


class FujiDecompressor(_Registers):
    BASE = 0xfff43000
    MOVRSZ_PROFILE = MOVRSZ_PROFILE
    MOVRSZ_PROFILES = tuple(MOVRSZ_PROFILES)
    # The shared block also implements the stock MOVRSZ mode1 submission.
    # These words are desired configuration, not LZ data/count registers.
    MOVRSZ_CONFIG = {0x08:0x00011113,0x10:0xffffffff,0x14:0x00007ff0,
        0x18:0x1fff1fff,0x20:0xffffffff,0x24:0x00007ff0,0x28:0x1fff1fff,
        0x30:0x000003ff,0x34:0x007f7f7f,0x38:0xffffffff,
        0x3c:0x03ff03ff,0x80:0x31ff31ff}
    MOVRSZ_ORDER = tuple(MOVRSZ_CONFIG)
    OFFSETS = {0x00, 0x04, 0x40, 0x44, 0x48, 0x4c, 0x80, 0x88, 0x8c} | set(MOVRSZ_CONFIG)
    IRQ = 0x45  # Logical100 table0xaeafe0: physical69, priority13, mode1.

    def __init__(self, *, bytes_per_tick=512, raise_irq=None, irq_targets=None,
                 movrsz_profile=None, movrsz_ticks=400000, **callbacks):
        super().__init__(**callbacks)
        if isinstance(bytes_per_tick, bool) or not isinstance(bytes_per_tick, int) or bytes_per_tick <= 0:
            raise ValueError("Decompressor virtual throughput must be a positive integer")
        if (raise_irq is None) != (irq_targets is None):
            raise ValueError("Decompressor IRQ requires both delivery and firmware-enabled routing callbacks")
        if movrsz_profile is not None and movrsz_profile not in MOVRSZ_PROFILES:
            raise ValueError('Unknown MOVRSZ completion profile')
        if type(movrsz_ticks) is not int or movrsz_ticks<1:
            raise ValueError('MOVRSZ delay must be a positive actual integer')
        if movrsz_profile is not None and raise_irq is None:
            raise ValueError('MOVRSZ completion requires firmware-enabled IRQ routing')
        self.raise_irq, self.irq_targets = raise_irq, irq_targets
        self.irqs_raised = 0
        self.completion_acknowledgements = 0
        self.bytes_per_tick = bytes_per_tick
        self.generation = 0
        self.busy = False
        self.stream = FujiLZStream()
        self.next_destination = None
        self.total_input_bytes = 0
        self.total_output_bytes = 0
        self.movrsz_requests = 0
        self.movrsz_request = None
        self.movrsz_profile,self.movrsz_ticks=movrsz_profile,movrsz_ticks
        self.movrsz_jobs_completed=0
        self.movrsz_output_bytes=0
        self.movrsz_irqs_raised=0
        self.movrsz_last_job=None

    def contains(self, address, size):
        return self.BASE <= address and address + size <= self.BASE + 0x100

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return ((self.BASE, self.BASE + 0x100),)

    def snapshot(self):
        return {**super().snapshot(), "history_bytes": len(self.stream.history),
                "pending_token_bytes": len(self.stream.pending),
                "pending_token_prefix": self.stream.pending[:16].hex(),
                "next_destination": None if self.next_destination is None else hex(self.next_destination),
                "total_input_bytes": self.total_input_bytes,
                "total_output_bytes": self.total_output_bytes,
                "physical_irq": hex(self.IRQ), "irqs_raised": self.irqs_raised,
                "completion_acknowledgements": self.completion_acknowledgements,
                "irq_routing": "Exactly one firmware-enabled bank at trigger; no enabled bank retains loader polling" if self.irq_targets is not None else "Legacy explicit polling interface; no IRQ callback configured",
                "irq_limits": ["Physical routing and completion timing are inferred from stock registration/status/acknowledge code, not measured hardware",
                               "Only successful method4 completion emits an IRQ; error-bit interrupt semantics are unmodeled",
                               "A bank enabled only after a completed polling job is not retroactively asserted; routing changes during an IRQ job stop explicitly"],
                "encoded_zero_length": "inferred 0x80000-byte counter wrap; not hardware validated",
                "virtual_bytes_per_tick": self.bytes_per_tick,
                "timing": "deterministic virtual ticks; hardware latency unvalidated",
                "movrsz_requests":self.movrsz_requests,
                "movrsz_request":self.movrsz_request,
                "movrsz_profile":self.movrsz_profile,"movrsz_ticks":self.movrsz_ticks,
                "movrsz_jobs_completed":self.movrsz_jobs_completed,
                "movrsz_output_bytes":self.movrsz_output_bytes,
                "movrsz_irqs_raised":self.movrsz_irqs_raised,
                "movrsz_last_job":self.movrsz_last_job,
                "movrsz_limits":["93278C desired12-WORD configuration plus8C=1/04=1 submission only",
                    "Default mode1 awaits an external result; it produces no raster, status/count, completion, IRQ or callback",
                    "Logical34 and loaderlogical100 share physicalIRQ45 but have separate firmware owners",
                    "Pyramid output uses declared packed YYCbCr format1, U16 luma format0 and two-to-one box averaging",
                    "The scene profile also transfers equal-geometry opaque format2 rows between declared strides; destination padding is zero",
                    "Physical filter, phase, format2 pixel interpretation and latency are unmeasured; MOVRSZ count4C remains unsupported"]}

    def read(self,cpu_id,address,size):
        offset=self._offset(address,size)
        if offset in self.MOVRSZ_CONFIG and offset!=0x80:
            raise UnsupportedDevice("MOVRSZ desired configuration has no proven MMIO getter")
        if self.movrsz_request is not None:
            self._movrsz_valid()
            if offset==0x4c:
                raise UnsupportedDevice("MOVRSZ output/count semantics are not modeled")
        return self.regs.get(offset,0)

    @classmethod
    def _movrsz_word(cls,offset,value,words):
        if type(value) is not int or not 0<=value<=0xffffffff or value & ~cls.MOVRSZ_CONFIG[offset]:
            raise UnsupportedDevice("MOVRSZ unknown typed/masked desired configuration")
        if offset in (0x10,0x20) and value & 15:
            raise UnsupportedDevice("MOVRSZ desired pointer must preserve stock16-byte alignment")
        if offset in (0x18,0x28) and value & 7:
            raise UnsupportedDevice("MOVRSZ desired low dimension must preserve stock8-unit alignment")
        if offset==0x34 and words.get(0x08,0)&0x1000:
            if (value&127)+2*((value>>8&127)+(value>>16&127))!=0x80:
                raise UnsupportedDevice("MOVRSZ selected filter coefficients do not sum to stock0x80")
        if offset==0x38:
            if (value&255)+2*sum(value>>shift&255 for shift in (8,16,24))!=0x100:
                raise UnsupportedDevice("MOVRSZ filter coefficients do not sum to stock0x100")
        if offset==0x3c and any(v>0x200 or v&7 for v in (value&0x3ff,value>>16&0x3ff)):
            raise UnsupportedDevice("MOVRSZ desired phase pair exceeds stock domain/alignment")

    def _movrsz_valid(self):
        request=self.movrsz_request
        if (not isinstance(request,dict) or type(request.get('step')) is not int
                or not 1<=request['step']<=len(self.MOVRSZ_ORDER)
                or request.get('state') not in ('configuring','configured','armed-awaiting-result',
                    'active','completed-awaiting-ack','acknowledged','released')
                or type(request.get('cpu')) is not int or request['cpu'] not in (0,1)
                or type(request.get('interrupt_enable_written')) is not bool
                or not isinstance(request.get('configuration'),dict)
                or list(request['configuration'])!=[hex(o) for o in self.MOVRSZ_ORDER[:request['step']]]):
            raise UnsupportedDevice("MOVRSZ request has an invalid typed configuration prefix")
        words={}
        for offset in self.MOVRSZ_ORDER[:request['step']]:
            word=request['configuration'][hex(offset)]
            self._movrsz_word(offset,word,words)
            if type(self.regs.get(offset)) is not int or self.regs[offset]!=word:
                raise UnsupportedDevice("MOVRSZ retained configuration differs from its request")
            words[offset]=word
        if request['state']=='configuring' and (request['step']==len(self.MOVRSZ_ORDER)
                or request['interrupt_enable_written']):
            raise UnsupportedDevice("MOVRSZ configuration phase was changed")
        if request['state']!='configuring' and request['step']!=len(self.MOVRSZ_ORDER):
            raise UnsupportedDevice("MOVRSZ configured/armed request is incomplete")
        if request['state'] not in ('configuring','configured') and (not request['interrupt_enable_written']
                or self.regs.get(0x04)!=1
                or self.regs.get(0x8c)!=(0xffffffff if request['state']=='released' else 1)):
            raise UnsupportedDevice("MOVRSZ arm lacks its exact enable/trigger")
        if request['state'] in ('active','completed-awaiting-ack','acknowledged','released'):
            if (self.movrsz_profile not in MOVRSZ_PROFILES or request.get('profile')!=self.movrsz_profile
                    or self.busy!=(request['state']=='active')):
                raise UnsupportedDevice('MOVRSZ completion state/profile/busy ownership changed')

    def _movrsz_configure(self,cpu_id,offset,value):
        if type(cpu_id) is not int or cpu_id not in (0,1):
            raise UnsupportedDevice("MOVRSZ configuration owner must be a real CPU index")
        if self.busy or type(self.regs.get(0)) is not int or self.regs[0]!=1:
            raise UnsupportedDevice("MOVRSZ configuration requires reset-ready shared hardware")
        request=self.movrsz_request
        if request is None:
            if offset!=self.MOVRSZ_ORDER[0]:
                raise UnsupportedDevice("MOVRSZ configuration must start at08")
            words={};step=0
        else:
            self._movrsz_valid()
            if request['state']!='configuring' or request['cpu']!=cpu_id:
                raise UnsupportedDevice("MOVRSZ request configuration is closed or has a different owner")
            step=request['step'];words={int(o,16):v for o,v in request['configuration'].items()}
            if offset!=self.MOVRSZ_ORDER[step]:
                raise UnsupportedDevice("MOVRSZ desired configuration is out of stock order")
        self._movrsz_word(offset,value,words)
        if request is None:
            request=dict(cpu=cpu_id,state='configuring',step=0,configuration={},
                interrupt_enable_written=False,physical_irq=hex(self.IRQ),completion_irq_cpu=None,
                source=None,destination=None)
            self.movrsz_request=request
        self.regs[offset]=value;request['configuration'][hex(offset)]=value
        request['step']+=1
        if request['step']==len(self.MOVRSZ_ORDER):request['state']='configured'

    def _movrsz_start(self,cpu_id):
        self._movrsz_valid();request=self.movrsz_request
        if (self.busy or self.regs.get(0)!=1 or request['state']!='configured'
                or not request['interrupt_enable_written'] or self.regs.get(0x8c)!=1
                or request['cpu']!=cpu_id):
            raise UnsupportedDevice("MOVRSZ start requires a complete same-owner request and exact enable")
        route=None
        if self.irq_targets is not None:
            targets=list(self.irq_targets(self.IRQ))
            if len(targets)!=1 or type(targets[0]) is not int or targets[0] not in (0,1):
                raise UnsupportedDevice("MOVRSZ request requires one enabled physicalIRQ45 route")
            route=targets[0]
        plan=None
        if self.movrsz_profile is not None:
            plan=self._movrsz_plan()
            data=bytes(self.read_bytes(plan.source,plan.source_bytes,cpu_id=cpu_id))
            if len(data)!=plan.source_bytes:
                raise UnsupportedDevice('MOVRSZ source read differs from its bounded DRAM span')
        self.regs[0x04]=1
        request.update(state='armed-awaiting-result',completion_irq_cpu=route,
            source=hex(self.regs[0x10]),destination=hex(self.regs[0x20]))
        self.movrsz_requests+=1
        self.record("MOVRSZ desired request armed awaiting result",cpu=cpu_id,
            source=request['source'],destination=request['destination'],physical_irq=hex(self.IRQ),
            limits=('No raster transfer/status/completion/IRQ; pitch/filter meanings unmodeled'
                    if plan is None else 'Explicit unmeasured pyramid profile; result and IRQ are scheduled'))
        if plan is not None:
            self._movrsz_schedule(plan,data,route)

    def _movrsz_plan(self):
        try:
            return DeclaredMovrszPlan(MOVRSZ_PROFILES[self.movrsz_profile],
                tuple(self.regs[offset] for offset in self.MOVRSZ_ORDER))
        except ValueError as error:
            raise UnsupportedDevice(str(error)) from error

    def _movrsz_schedule(self,plan,data,route):
        request=self.movrsz_request
        request.update(state='active',profile=self.movrsz_profile,stage=plan.stage,
            source_bytes=plan.source_bytes,output_bytes=plan.output_bytes,irq_raised=False)
        self.busy=True
        self.movrsz_last_job=dict(request)
        self.record('MOVRSZ approximate resize start',cpu=request['cpu'],stage=plan.stage,
            profile=self.movrsz_profile,source_bytes=plan.source_bytes,output_bytes=plan.output_bytes)
        generation=self.generation
        def complete():
            if generation!=self.generation:
                return
            self._movrsz_valid()
            # An empty target list is a masked enable group (stock 42B00/42EB0 clear
            # and restore all 32 sources); raise_irq latches pending while masked.
            targets=list(self.irq_targets(self.IRQ))
            same_or_masked=(not targets or len(targets)==1
                            and type(targets[0]) is int and targets[0]==route)
            if (self.movrsz_request is not request or request['state']!='active'
                    or self._movrsz_plan()!=plan or not same_or_masked):
                raise UnsupportedDevice('MOVRSZ active descriptor or IRQ route changed')
            output=plan.raster(data)
            if len(output)!=plan.output_bytes:
                raise UnsupportedDevice('MOVRSZ approximate raster differs from its destination span')
            self.write_bytes(plan.destination,output,cpu_id=request['cpu'])
            self.regs[0x88]=1
            request['state']='completed-awaiting-ack'
            self.busy=False
            self.movrsz_jobs_completed+=1
            self.movrsz_output_bytes+=len(output)
            self.raise_irq(route,self.IRQ)
            self.irqs_raised+=1
            self.movrsz_irqs_raised+=1
            request['irq_raised']=True
            self.movrsz_last_job=dict(request)
            self.record('MOVRSZ approximate resize complete',cpu=request['cpu'],stage=plan.stage,
                output_bytes=len(output),irq_cpu=route,physical_irq=hex(self.IRQ))
        self.schedule(self.movrsz_ticks,complete)

    def write(self, cpu_id, address, size, value):
        offset = self._offset(address, size)
        if type(value) is not int or not 0<=value<=0xffffffff:
            raise UnsupportedDevice("Fuji shared engine requires a typed unsigned WORD")
        if offset == 0x4c:
            raise UnsupportedDevice("Fuji decompressor output count is read-only")
        if offset == 0x88:
            #9326C8 MVN r2,#0;9326D0 STR to88 uses all-ones W1C.
            #It clears already present bits only; it never creates a result.
            if value!=0xffffffff and value & ~0xf01:
                raise UnsupportedDevice(f"Unknown Fuji decompressor acknowledge bits {value:#x}")
            request=self.movrsz_request
            if request is not None and self.movrsz_profile is not None:
                self._movrsz_valid()
                if cpu_id!=request['cpu'] or request['state'] not in ('completed-awaiting-ack','acknowledged','released'):
                    raise UnsupportedDevice('MOVRSZ completion acknowledgment before result or by different owner')
                if value not in (1,0xffffffff):
                    raise UnsupportedDevice('MOVRSZ unsupported completion acknowledgment')
            if self.regs.get(0x88,0) & value & 1:
                self.completion_acknowledgements += 1
                if request is not None and self.movrsz_profile is not None:
                    request['state']='acknowledged'
            self.regs[0x88] = self.regs.get(0x88, 0) & ~value
            return
        if offset in self.MOVRSZ_CONFIG and (offset!=0x80 or self.movrsz_request is not None):
            self._movrsz_configure(cpu_id,offset,value)
            return
        if self.movrsz_request is not None:
            self._movrsz_valid()
            request=self.movrsz_request
            if self.movrsz_profile is not None and cpu_id==request['cpu']:
                if offset==0x8c and value==1 and request['state'] in ('completed-awaiting-ack','acknowledged'):
                    return #1211808 reads8C and writes the retained1 unchanged.
                if offset==0x8c and value==0xffffffff and request['state']=='acknowledged':
                    self.regs[0x8c]=value;request['state']='released'
                    self.movrsz_last_job=dict(request)
                    return #9326D8 release follows the IRQ acknowledgment.
                if offset==0 and value==0 and request['state']=='released':
                    self.movrsz_request=None #Next1212550 starts a new reset/configuration.
                else:
                    if request['state'] not in ('configuring','configured'):
                        raise UnsupportedDevice('MOVRSZ active/completed shared-engine mutation is unsupported')
            if self.movrsz_request is None:
                pass #Continue into the ordinary reset implementation below.
            elif (offset==0x8c and value==1 and self.movrsz_request['state']=='configured'
                    and cpu_id==self.movrsz_request['cpu']):
                self.regs[0x8c]=1;self.movrsz_request['interrupt_enable_written']=True
                return
            elif offset==0x04 and value==1:
                self._movrsz_start(cpu_id)
                return
            else:
                raise UnsupportedDevice("MOVRSZ active/configured shared-engine mutation is not modeled")
        if offset == 0x8c and value not in (0,1):
            raise UnsupportedDevice(f"Unknown Fuji decompressor interrupt-enable bits {value:#x}")
        if offset == 0x00:
            if value != 0:
                raise UnsupportedDevice(f"Unknown Fuji decompressor reset {value:#x}")
            self.generation += 1
            generation = self.generation
            self.regs[0x00] = 0
            self.regs[0x88] = 0
            self.regs[0x4c] = 0
            self.busy = False
            self.stream = FujiLZStream()
            self.next_destination = None
            def reset_complete():
                if generation == self.generation:
                    self.regs[0x00] = 1
                    self.record("Fuji decompressor reset complete", cpu=cpu_id)
            self.schedule(2, reset_complete)
            return
        self.regs[offset] = value
        if offset == 0x04:
            if value != 2:
                raise UnsupportedDevice(f"Unknown Fuji decompressor trigger {value:#x}")
            self._start(cpu_id)

    def _start(self, cpu_id):
        if self.busy or self.regs.get(0x00) != 1:
            raise UnsupportedDevice("Fuji decompressor start while busy or not reset")
        if self.regs.get(0x80) != 0x30003000 or self.regs.get(0x8c) != 1:
            raise UnsupportedDevice("Unknown Fuji decompressor setup")
        control = self.regs.get(0x44, 0)
        if control & ~0x7ffff:
            raise UnsupportedDevice(f"Unsupported Fuji decompressor method/length {control:#x}")
        # 0x3edc can submit0x80000;0x3c90 deliberately stores low19bits.
        # Treat encoded0as a full512KiB counter. This is an explicit inferred
        # counter convention, confirmed as necessary by an unmodified loader
        # trace, not an independently measured decompressor register contract.
        count = (control & 0x7ffff) or 0x80000
        source, destination = self.regs.get(0x40, 0), self.regs.get(0x48, 0)
        if self.next_destination is not None and destination != self.next_destination:
            raise UnsupportedDevice("Noncontiguous Fuji decompressor job")
        data = self.read_bytes(source, count, cpu_id=cpu_id)
        irq_cpu = None
        if self.irq_targets is not None:
            targets = list(self.irq_targets(self.IRQ))
            if any(n not in (0,1) for n in targets) or len(set(targets)) != len(targets) or len(targets)>1:
                raise UnsupportedDevice("Ambiguous Fuji decompressor completion IRQ route")
            irq_cpu = targets[0] if targets else None
        self.busy = True
        self.regs[0x88] = 0
        self.regs[0x4c] = 0
        self.jobs_started += 1
        self.total_input_bytes += count
        self.last_job = {"cpu": cpu_id, "source": hex(source),
                         "destination": hex(destination), "compressed_bytes": count,
                         "completion_irq_cpu": irq_cpu,
                         "physical_irq": hex(self.IRQ) if irq_cpu is not None else None}
        job = self.last_job
        generation = self.generation
        self.record("Fuji decompressor start", cpu=cpu_id, source=hex(source),
                    destination=hex(destination), compressed_bytes=count)
        def complete():
            if generation != self.generation:
                return
            if irq_cpu is not None and self.regs.get(0x8c,0) & 1:
                if list(self.irq_targets(self.IRQ)) != [irq_cpu]:
                    raise UnsupportedDevice("Fuji decompressor IRQ routing changed during active job")
            try:
                output = self.stream.feed(data)
            except ValueError as exc:
                # Error bit 8 is an explicit model fixture: stock checks the
                # entire 0xf00 error field, its precise hardware cause is unknown.
                self.regs[0x88] = 0x101
                self.busy = False
                self.record("Fuji decompressor malformed stream", error=str(exc))
                return
            self.write_bytes(destination, output, cpu_id=cpu_id)
            self.next_destination = destination + len(output)
            self.regs[0x4c] = len(output)
            self.regs[0x88] = 1
            self.busy = False
            self.jobs_completed += 1
            self.total_output_bytes += len(output)
            self.record("Fuji decompressor complete", cpu=cpu_id,
                        output_bytes=len(output), pending_token_bytes=len(self.stream.pending))
            if irq_cpu is not None and self.regs.get(0x8c,0) & 1:
                self.raise_irq(irq_cpu,self.IRQ)
                self.irqs_raised += 1
                job["irq_raised"] = True
                self.record("Fuji decompressor completion IRQ raised", cpu=irq_cpu,
                            line=hex(self.IRQ), output_bytes=len(output),
                            provenance="Actual0x919510 logical100 maps physical0x45; stock0x932be8 acknowledges statusbit0")
        # Approximate byte budget, matched to virtual NAND so the scheduler
        # does not manufacture oversized backlogs absent in the faster physical
        # LZ engine. Actual hardware latency has not been measured.
        self.schedule(max(2, count // self.bytes_per_tick), complete)


@dataclass
class PeripheralDmaState:
    generation: int = 0
    active: bool = False
    scheduled: bool = False
    cpu: int = 0
    alternate: bool = False


class LoaderMemoryDMA(_Registers):
    """Channel0 loader/application memory transfers and bounded channel1 audio TX."""
    BASE = 0xfff02000
    OFFSETS = {channel * 0x40 + offset for channel in range(8) for offset in (0x00, 0x04, 0x08, 0x0c, 0x10, 0x14, 0x24, 0x28, 0x2c, 0x30, 0x34)} | {0x300}

    def __init__(self, *, raise_irq=None, irq_targets=None, **callbacks):
        super().__init__(**callbacks)
        self.raise_irq,self.irq_targets=raise_irq,irq_targets
        self.generation = 0
        self.busy = False
        self.bytes_completed = 0
        self.jobs_aborted = 0
        self.peripheral_channels = {channel: PeripheralDmaState() for channel in range(1, 8)}
        self.audio_endpoint = None

    def contains(self, address, size):
        return ((self.BASE <= address and address + size <= self.BASE + 0x200)
                or (self.BASE + 0x300 <= address and address + size <= self.BASE + 0x304))

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return ((self.BASE, self.BASE + 0x200), (self.BASE + 0x300, self.BASE + 0x304))

    def snapshot(self):
        return {**super().snapshot(), "bytes_completed": self.bytes_completed,
                "jobs_aborted": self.jobs_aborted,
                "supported_mode": "channel0 loader/application fill and copy; X-M1 channel1 audio TX to the declared sink",
                "limits": ["Legacy application mode interpretation comes from X70 0x924f74/0x925108 callers, not measured DMA hardware",
                           "Alternate descriptor words+0xc/+0x10/+0x14 are retained but inactive for these exact nonlinked modes",
                           "Application completion IRQ0x5b routing follows exactly one enabled firmware bank; loader IRQ suppression is inferred",
                           "X-M1 channel1 audio TX consumes bounded, even-length DRAM byte streams; paired descriptors and physical IRQ0x5c are modeled, not DAC output or measured latency"],
                "timing": "deterministic virtual ticks; hardware latency unvalidated"}

    def write(self, cpu_id, address, size, value):
        offset = self._offset(address, size)
        channel, relative = divmod(offset, 0x40)
        if offset < 0x200 and channel:
            return self._write_audio_channel(cpu_id, channel, relative, value)
        value &= 0xffffffff
        if offset == 0x300:
            # Actual INIT0x9246dc stores0after clearing its software registry;
            # it does not reset each channel or poll a peripheral ready bit.
            if value:
                raise UnsupportedDevice("Unknown global memory DMA configuration")
            self.regs[offset] = value
            self.record("Memory DMA global configuration", cpu=cpu_id, value=hex(value))
            return
        if offset == 0x24:
            raise UnsupportedDevice("Loader DMA completion status is read-only")
        self.regs[offset] = value
        if offset != 0x28:
            return
        if value == 8:
            if self.busy:
                self.jobs_aborted += 1
            self.generation += 1
            self.busy = False
            self.regs[0x24] = 0
            return
        if value not in (1, 5) or self.busy:
            raise UnsupportedDevice(f"Unknown loader DMA control {value:#x}")
        config = self.regs.get(0x2c, 0)
        source_width, destination_width = (config >> 12) & 15, (config >> 16) & 15
        mode=config & ~0xff000
        if mode not in (0x1500400,0x500400,0x400400) or not 2 <= source_width <= 5 or source_width != destination_width:
            raise UnsupportedDevice(f"Unknown loader DMA mode {config:#x}")
        application=mode!=0x1500400
        linear_copy=mode==0x400400
        if (any(self.regs.get(n,0) for n in (0x30,0x34)) or
                (not application and any(self.regs.get(n,0) for n in (0x0c,0x10,0x14)))):
            raise UnsupportedDevice("Unsupported loader DMA strides")
        irq_cpu=None
        if application:
            if self.raise_irq is None or self.irq_targets is None:
                raise UnsupportedDevice("Application memory DMA completion IRQ routing is not configured")
            targets=list(self.irq_targets(0x5b))
            if len(targets)!=1 or targets[0] not in (0,1):
                raise UnsupportedDevice("Application memory DMA IRQ bank missing or ambiguous")
            irq_cpu=targets[0]
        source, destination, count = (self.regs.get(n, 0) for n in (0x00, 0x04, 0x08))
        if not 0 < count <= 0x08000000:
            raise UnsupportedDevice("Invalid loader DMA byte count")
        pattern = self.read_bytes(source, count if linear_copy else 32, cpu_id=cpu_id)
        if len(pattern) != (count if linear_copy else 32):
            raise UnsupportedDevice("Truncated loader DMA source")
        if not linear_copy and pattern != pattern[:4] * 8:
            raise UnsupportedDevice("Nonuniform source is outside loader DMA fill mode")
        generation = self.generation
        self.busy = True
        # Stock0x924a5c polls bit0 until clear; do not let a pending
        # application transaction return as if its RAM write had completed.
        self.regs[0x24] = 1
        self.jobs_started += 1
        self.last_job = {"cpu": cpu_id, "source": hex(source),
                         "destination": hex(destination), "bytes": count, "config": hex(config),
                         "operation":"linear copy" if linear_copy else "uniform fill", "irq_cpu":irq_cpu}
        self.record("Loader memory DMA start", cpu=cpu_id, source=hex(source),
                    destination=hex(destination), bytes=count, config=hex(config))
        def complete():
            if generation != self.generation:
                return
            data = pattern if linear_copy else (pattern * ((count + 31) // 32))[:count]
            self.write_bytes(destination, data, cpu_id=cpu_id)
            self.regs[0x24] = 0x40
            self.busy = False
            self.jobs_completed += 1
            self.bytes_completed += count
            self.record("Loader memory DMA complete", cpu=cpu_id, bytes=count)
            if application:
                self.raise_irq(irq_cpu,0x5b)
        self.schedule(max(2, count // 32), complete)

    def _write_audio_channel(self, cpu_id, channel, relative, value):
        base = channel * 0x40
        state = self.peripheral_channels[channel]
        if relative == 0x24:
            raise UnsupportedDevice('Peripheral DMA status is read-only')
        self.regs[base + relative] = value & 0xffffffff
        if relative != 0x28:
            return
        if value & 8:
            state.generation += 1
            state.active = state.scheduled = state.alternate = False
            self.regs[base + 0x24] = 0
            return
        if value & 0x100:
            state.generation += 1
            state.active = state.scheduled = False
            self.regs[base + 0x24] = 0
            return
        if value not in (1, 5) or state.active:
            raise UnsupportedDevice(f'Unsupported peripheral DMA control {channel}:{value:#x}')
        if channel != 1:
            raise UnsupportedDevice('Only target-bound audio DMA channel1 transfer is modeled')
        config = self.regs.get(base + 0x2c, 0)
        if config & ~0x100ff007 != 0x40200010 or config & 7 != channel:
            raise UnsupportedDevice(f'Unsupported audio DMA configuration {channel}:{config:#x}')
        if any(self.regs.get(base + offset, 0) for offset in (0x30, 0x34)):
            raise UnsupportedDevice('Strided audio DMA is unsupported')
        state.active, state.cpu = True, cpu_id
        self.regs[base + 0x24] = 5
        self.wake_audio_channels()

    def wake_audio_channels(self):
        if self.audio_endpoint is None:
            return
        for channel, state in self.peripheral_channels.items():
            base = channel * 0x40
            destination = self.regs.get(base + (0x10 if state.alternate else 0x04), 0)
            if not state.active or state.scheduled or not self.audio_endpoint.can_transmit(destination):
                continue
            state.scheduled = True
            generation = state.generation
            def begin(channel=channel, generation=generation):
                current = self.peripheral_channels[channel]
                if current.generation != generation or not current.active:
                    return
                current.scheduled = False
                self._begin_audio_transfer(channel, generation)
            self.schedule(1000, begin)

    def _begin_audio_transfer(self, channel, generation):
        state, base = self.peripheral_channels[channel], channel * 0x40
        offsets = (0x0c, 0x10, 0x14) if state.alternate else (0x00, 0x04, 0x08)
        source, destination, count = (self.regs.get(base + offset, 0) for offset in offsets)
        if not self.audio_endpoint.can_transmit(destination):
            return
        config = self.regs.get(base + 0x2c, 0)
        if not 0 < count <= 0x100000 or count & 1:
            raise UnsupportedDevice(f'Unsupported audio DMA count {count:#x}')
        if not (0 < source < 0x20000000 or 0x40000000 < source < 0x60000000):
            raise UnsupportedDevice(f'Audio DMA source is not DRAM: {source:#x}')
        if (config >> 12) & 15 not in (0, 1, 2) or (config >> 16) & 15 != 2:
            raise UnsupportedDevice(f'Unsupported audio DMA widths {config:#x}')
        targets = list(self.irq_targets(0x5c)) if self.irq_targets is not None else []
        if len(targets) != 1 or self.raise_irq is None:
            raise UnsupportedDevice('X-M1 audio DMA physical IRQ0x5c requires one configured bank')
        irq_cpu = targets[0]
        payload = bytes(self.read_bytes(source, count, cpu_id=state.cpu))
        if len(payload) != count:
            raise UnsupportedDevice('Truncated audio DMA payload')
        state.scheduled = True
        self.jobs_started += 1
        job = dict(operation='audio transmit', channel=channel, source=hex(source),
                   destination=hex(destination), bytes=count, irq_cpu=irq_cpu,
                   sha256=hashlib.sha256(payload).hexdigest(), completed=False)
        self.last_job = job
        def complete():
            current = self.peripheral_channels[channel]
            if current.generation != generation or not current.active:
                return
            current.scheduled = False
            self.audio_endpoint.transmit_dma(destination, payload)
            self.jobs_completed += 1
            self.bytes_completed += count
            job['completed'] = True
            repeat = bool(self.regs.get(base + 0x2c, 0) & 0x40000000)
            current.active, current.alternate = repeat, repeat
            self.regs[base + 0x24] = 5 if repeat else 0x40
            self.record('Audio DMA payload consumed', channel=channel, bytes=count,
                        sha256=hashlib.sha256(payload).hexdigest(),
                        limits='Atomic byte-stream sink; DMA handshake and latency are declared, unmeasured laws')
            self.raise_irq(irq_cpu, 0x5c)
            self.wake_audio_channels()
        self.schedule(max(1000, count * 64), complete)


class NandController(_Registers):
    """Stock type-1 geometry: 2KiB pages, 64 pages per 128KiB block.

    The supplied DAT provides data bytes only, not OOB/ECC or bad-block maps.
    This model supplies erased spare bytes, clean ECC and erased padding up to
    1024 blocks. One evidenced whole-page TX command programs a volatile
    backend; the original image and captured storage fixtures stay immutable.
    """
    BASE = 0xfff52000
    # Interrupt and completion status words polled by the loader and app:
    # plain register reads, changed only by writes and scheduled completions.
    repeatable_reads = frozenset({BASE + 0x04, BASE + 0x54})
    OFFSETS = {0x00, 0x04, 0x08, 0x10, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x30,
               0x40, 0x44, 0x48, 0x4c, 0x50, 0x54, 0x58, 0x5c, 0x60, 0x70, 0x74, 0x78,
               0x7c, 0x98, 0xc8, 0xe8, 0x804, 0x80c, 0x810, 0x820,
               0x824, 0x828, 0x844, 0x884, 0x88c, 0x890, 0x8a0,
               0x8a4, 0x8a8, 0x8c4}
    FLASH_SIZE = 0x08000000
    _pio_program_cache = None
    _erase_ticks = 64  # Explicit virtual block delay, not hardware timing.

    def __init__(self, native_flash, *, raise_irq=None, persistent_image=None, erase_ticks=None, **callbacks):
        if erase_ticks is not None:
            if type(erase_ticks) is not int or not 1 <= erase_ticks < 1 << 64:
                raise ValueError('NAND erase delay must be positive and bounded')
            self._erase_ticks = erase_ticks
        super().__init__(**callbacks)
        if not 0 < len(native_flash) <= self.FLASH_SIZE:
            raise ValueError("Invalid virtual NAND image size")
        self.flash = bytes(native_flash)
        self.flash_sha256 = hashlib.sha256(self.flash).hexdigest()
        self.raise_irq = raise_irq
        self.nand_irqs_raised = 0
        self.aperture = bytearray(b"\xff" * 512)
        self.busy = False
        self.generation = 0
        self.transaction_counts = {"reset": 0, "spare": 0, "pio": 0, "dma": 0,
                                   "program": 0}
        self.data_bytes_completed = 0
        self.erased_padding_bytes_completed = 0
        self.data_overlay_bytes_completed = 0
        self.volatile_data_bytes_read = 0
        self.jobs_aborted = 0
        self.data_overlays = []
        self.spare_pages = {}
        self.volatile_pages = {}
        self.volatile_spare_pages = {}
        self.program_bytes_completed = 0
        self.program_effects = []
        for offset in (0x40, 0x44, 0x48, 0x4c):
            self.regs[offset] = 0xffffffff
        if persistent_image is not None:
            (self.data_overlays, self.spare_pages, self.volatile_pages,
             self.volatile_spare_pages) = restore_image(persistent_image,
                flash_sha256=self.flash_sha256, flash_size=len(self.flash))

    def export_persistent_image(self):
        """Export full committed backing for an independent modeled cold boot.

        The caller saves bytes to a fresh ignored artifact path. This never
        writes the immutable DAT or original capture; pending jobs must finish.
        """
        if self._pio_program_cache is not None:
            raise ValueError('Persistent NAND export requires a completed PIO program cache')
        return export_image(self)

    def load_initial_persistent_image(self, image):
        """Supply independent cold backing before the first NAND operation.

        The canonical factory owns this controller; a scenario can supply
        saved backing through its fixture API without replacing the device.
        Validate all bytes and identity before mutating any backing map.
        """
        if (self.busy or self.jobs_started or self.data_overlays or self.spare_pages
                or self.volatile_pages or self.volatile_spare_pages):
            raise ValueError('Persistent NAND input requires pristine cold backing')
        backing = restore_image(image, flash_sha256=self.flash_sha256,
                                flash_size=len(self.flash))
        (self.data_overlays, self.spare_pages, self.volatile_pages,
         self.volatile_spare_pages) = backing
        self.record('NAND initial persistent backing', sha256=hashlib.sha256(image).hexdigest(),
                     basis='explicit independent cold input; no emulated program/erase')

    @staticmethod
    def _fixture_provenance(provenance):
        if not isinstance(provenance, (str, dict)) or not provenance:
            raise ValueError("Synthetic NAND fixture needs explicit provenance")
        # Retain a JSON-safe independent copy in evidence snapshots.
        return json.loads(json.dumps(provenance))

    def add_data_overlay(self, offset, data, provenance):
        """Supply an explicit synthetic physical mapping without changing DAT."""
        if self.busy or self.jobs_started:
            raise ValueError("NAND fixtures must be configured before execution")
        if isinstance(offset, bool) or not isinstance(offset, int):
            raise ValueError("NAND overlay offset must be an integer")
        data = bytes(data)
        if offset < 0 or not data or offset + len(data) > self.FLASH_SIZE:
            raise ValueError("NAND overlay exceeds type-1 geometry")
        if any(offset < item["offset"] + len(item["data"]) and item["offset"] < offset + len(data)
               for item in self.data_overlays):
            raise ValueError("Synthetic NAND data overlays cannot overlap")
        self.data_overlays.append({"offset": offset, "data": data,
                                  "sha256": hashlib.sha256(data).hexdigest(),
                                  "provenance": self._fixture_provenance(provenance)})
        self.data_overlays.sort(key=lambda item: item["offset"])

    def add_spare_fixture(self, page, data, provenance):
        """Set one explicit OOB prefix; unprovided bytes remain erased."""
        if self.busy or self.jobs_started:
            raise ValueError("NAND fixtures must be configured before execution")
        if isinstance(page, bool) or not isinstance(page, int) or not 0 <= page < self.FLASH_SIZE // 2048:
            raise ValueError("NAND spare page exceeds type-1 geometry")
        data = bytes(data)
        if not 0 < len(data) <= 512 or page in self.spare_pages:
            raise ValueError("Invalid or duplicate NAND spare fixture")
        self.spare_pages[page] = {"data": data + b"\xff" * (512 - len(data)),
                                  "prefix_bytes": len(data),
                                  "sha256": hashlib.sha256(data).hexdigest(),
                                  "provenance": self._fixture_provenance(provenance)}

    def contains(self, address, size):
        return self.BASE <= address and address + size <= self.BASE + 0x900

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return ((self.BASE, self.BASE + 0x900),)

    def snapshot(self):
        return {**super().snapshot(), "native_data_bytes": len(self.flash),
                "native_data_sha256": self.flash_sha256,
                "geometry": {"page_bytes": 2048, "pages_per_block": 64, "blocks": 1024},
                "completed_transactions": dict(self.transaction_counts),
                "data_bytes_completed": self.data_bytes_completed,
                "erased_padding_bytes_completed": self.erased_padding_bytes_completed,
                "data_overlay_bytes_completed": self.data_overlay_bytes_completed,
                "volatile_data_bytes_read": self.volatile_data_bytes_read,
                "program_bytes_completed": self.program_bytes_completed,
                "program_effects": list(self.program_effects),
                "volatile_data_pages": [{"page": page, "bytes": len(data),
                                          "sha256": hashlib.sha256(data).hexdigest(),
                                          "data_hex": data.hex()}
                                         for page, data in sorted(self.volatile_pages.items())],
                "volatile_spare_pages": [{"page": page, "bytes": len(data),
                                           "sha256": hashlib.sha256(data).hexdigest(),
                                           "data_hex": data.hex()}
                                          for page, data in sorted(self.volatile_spare_pages.items())],
                "nand_irqs_raised": self.nand_irqs_raised,
                "jobs_aborted": self.jobs_aborted,
                "aperture_prefix": bytes(self.aperture[:32]).hex(),
                "data_overlays": [{"offset": hex(item["offset"]), "bytes": len(item["data"]),
                                    "sha256": item["sha256"], "provenance": item["provenance"]}
                                   for item in self.data_overlays],
                "spare_page_fixtures": [{"page": page, "prefix_bytes": item["prefix_bytes"],
                                         "sha256": item["sha256"], "provenance": item["provenance"]}
                                        for page, item in sorted(self.spare_pages.items())],
                "fixtures": ["Spare/ECC bytes clean; no bad blocks", "Erased padding beyond DAT data extent",
                             "Whole-page command802362 uses volatile NAND bitwise-AND programming",
                             "Metadata registers40..4c supply first16 OOB bytes per programmed page; ECC unresolved"],
                "timing": "deterministic virtual ticks; hardware latency unvalidated"}

    def _aperture_range(self, address, size):
        offset = address - self.BASE - 0x400
        return offset if 0 <= offset and offset + size <= 512 and size in (1, 2, 4) else None

    def read(self, cpu_id, address, size):
        aperture_offset = self._aperture_range(address, size)
        if aperture_offset is not None:
            return int.from_bytes(self.aperture[aperture_offset:aperture_offset + size], "little")
        return super().read(cpu_id, address, size)

    def write(self, cpu_id, address, size, value):
        aperture_offset = self._aperture_range(address, size)
        if aperture_offset is not None:
            self.aperture[aperture_offset:aperture_offset + size] = value.to_bytes(size, "little")
            return
        offset = self._offset(address, size)
        value &= 0xffffffff
        if offset in (0x810, 0x890):
            raise UnsupportedDevice("NAND DMA active status is read-only")
        if offset == 0x04:
            # The loader clears interrupt flags *after* command start. Bit 2
            # remains busy until that transaction really copies its data.
            self.regs[offset] = self.regs.get(offset, 0) & ~(value & ~4)
            return
        if offset == 0x54:
            # ROM uses e0000/e0001; app 0x9abca4 enables with80009 and
            # 0x9abccc disables with80001. Bit0 is W1C done, bit3 is
            # completion IRQ enable; bits17..19 retain written control.
            if value & ~0xe0009:
                raise UnsupportedDevice("Unknown NAND completion/mask controls")
            self.regs[offset] = ((self.regs.get(offset, 0) & 1 & ~value)
                                 | (value & 0xe0008))
            return
        if offset in (0x80c, 0x88c):
            # App start/stop routines and both channel ISRs write 1 here.
            # These acknowledge pending completion; bit28 of +20 is enable.
            if value & ~1:
                raise UnsupportedDevice("Unknown NAND DMA completion acknowledge")
            self.regs[offset] = self.regs.get(offset, 0) & ~value
            return
        if offset in (0x844, 0x8c4) and value:
            # 0x9acd24 clears this two-bit field before enabling a channel;
            # its additional hardware meanings have not been recovered.
            raise UnsupportedDevice("Unknown NAND DMA status-field control")
        self.regs[offset] = value
        if offset != 0x08:
            return
        if value == 0:
            if self._pio_program_cache is not None:
                self._pio_program_cache = None
            if self.busy:
                self.generation += 1
                self.busy = False
                self.jobs_aborted += 1
                self.regs[0x04] = self.regs.get(0x04, 0) & ~4
                self.regs[0x890] = 0
                self.regs[0x810] = 0
                self.record("NAND transaction aborted", cpu=cpu_id)
            return
        if value & 0x800000:
            self._start(cpu_id, value)

    def _read_flash(self, offset, count):
        if offset < 0 or count < 0 or offset + count > self.FLASH_SIZE:
            raise UnsupportedDevice("Virtual NAND read outside type-1 geometry")
        data = self.flash[offset:offset + count]
        data = bytearray(data + b"\xff" * (count - len(data)))
        for item in self.data_overlays:
            start, end = max(offset, item["offset"]), min(offset + count, item["offset"] + len(item["data"]))
            if start < end:
                data[start - offset:end - offset] = item["data"][start - item["offset"]:end - item["offset"]]
        for page in range(offset // 2048, (offset + count + 2047) // 2048):
            if page in self.volatile_pages:
                page_start = page * 2048
                start, end = max(offset, page_start), min(offset + count, page_start + 2048)
                data[start - offset:end - offset] = self.volatile_pages[page][start - page_start:end - page_start]
        return bytes(data)

    def _read_spare(self, row):
        return self.volatile_spare_pages.get(row,
            self.spare_pages.get(row, {}).get("data", b"\xff" * 512))

    def _read_source_counts(self, offset, count):
        # The precedence here matches _read_flash: native/erased, supplied
        # overlay, then committed volatile program pages. Count final byte
        # origins rather than calling newly programmed tail bytes "erased".
        origins = bytearray(count)
        padding_start = max(0, len(self.flash) - offset)
        origins[min(count, padding_start):] = b"\x01" * max(0, count - padding_start)
        for item in self.data_overlays:
            start, end = max(offset, item["offset"]), min(offset + count, item["offset"] + len(item["data"]))
            if start < end:
                origins[start - offset:end - offset] = b"\x02" * (end - start)
        for page in range(offset // 2048, (offset + count + 2047) // 2048):
            if page in self.volatile_pages:
                start, end = max(offset, page * 2048), min(offset + count, (page + 1) * 2048)
                origins[start - offset:end - offset] = b"\x03" * (end - start)
        return origins.count(1), origins.count(2), origins.count(3)

    def _start_pio_program(self, cpu_id, command):
        """Four stock512-byte aperture shifts; commit only at program confirm."""
        row, column = self.regs.get(0x24, 0), self.regs.get(0x20, 0)
        if column not in (0, 0x210, 0x420, 0x630) or not 0 <= row < self.FLASH_SIZE // 2048:
            raise UnsupportedDevice('Unknown NAND PIO program row/sector')
        sector = column // 0x210
        expected = ((0x882062, 0x80000001), (0x882004, 1),
                    (0x882004, 1), (0x802064, 0x00107001))[sector]
        if ((command, self.regs.get(0x1c)) != expected or self.regs.get(0x5c) != 0x1010
                or self.regs.get(0x18) != 0x8093):
            raise UnsupportedDevice('Unknown NAND PIO cache command/setup')
        metadata = tuple(self.regs.get(offset, 0xffffffff) for offset in (0x40, 0x44, 0x48, 0x4c))
        cache = self._pio_program_cache
        if sector == 0:
            if cache is not None:
                raise UnsupportedDevice('NAND PIO cache start before prior page confirm')
            cache = dict(row=row, sector=0, data=b'', metadata=metadata)
        elif cache is None or (cache['row'], cache['sector'], cache['metadata']) != (row, sector, metadata):
            raise UnsupportedDevice('NAND PIO cache sequence/row/metadata changed')
        payload = cache['data'] + bytes(self.aperture)
        if sector == 3:
            self._start_program(cpu_id, command, pio_payload=payload)
            return
        generation = self.generation
        self.busy = True
        self.jobs_started += 1
        self.regs[4] = self.regs.get(4, 0) | 4
        self.regs[0x54] = self.regs.get(0x54, 0) & ~1
        job = dict(cpu=cpu_id, command=hex(command), operation='pio-program-cache',
                   row=row, sector=sector, bytes=512, payload_sha256=hashlib.sha256(bytes(self.aperture)).hexdigest(),
                   basis='stock four-sector aperture sequence; no persistent commit before final10 opcode')
        self.last_job = dict(job)
        self.record('NAND PIO program cache start', **job)
        def complete():
            if self.generation != generation:
                return
            self._pio_program_cache = dict(cache, sector=sector + 1, data=payload)
            self.busy = False
            self.regs[4] &= ~4
            self.regs[0x54] |= 1
            self.regs[0x28] = self.regs[0xc8] = 0
            self.jobs_completed += 1
            self.transaction_counts['program'] += 1
            self.program_bytes_completed += 512
            self.record('NAND PIO program cache complete', **job)
            self._completion_irq(cpu_id)
        self.schedule(2, complete)

    def _start_program(self, cpu_id, command, *, pio_payload=None):
        """Whole-page DMA TX or exactly assembled four-sector PIO confirm."""
        setup, row = self.regs.get(0x1c, 0), self.regs.get(0x24, 0)
        if pio_payload is None:
            units = setup & 0xff
            units = units or 256
            count = units * 512
            if (setup & ~0xff != 0x80107000 or units % 4 or
                    self.regs.get(0x20, 0) or self.regs.get(0x5c) != 0x1010):
                raise UnsupportedDevice("Unknown NAND whole-page TX command/setup")
            config = self.regs.get(0x820, 0)
            if (config & ~0x90700000 != 0x1000 or not config & 0x80000000 or
                    (config >> 20) & 7 not in (3, 4)):
                raise UnsupportedDevice("Unknown NAND transmit DMA width/mode")
            if config & 0x10000000:
                raise UnsupportedDevice("Application NAND transmit DMA completion IRQ is not modeled")
            if self.regs.get(0x804) != 0x10001 or self.regs.get(0x828) != 0x100000:
                raise UnsupportedDevice("NAND transmit DMA is not enabled/configured")
            source = self.regs.get(0x824, 0)
            alignment = 64 if (config >> 20) & 7 == 4 else 32
            if source % alignment:
                raise UnsupportedDevice("NAND transmit DMA source is misaligned")
            payload = self.read_bytes(source, count, cpu_id=cpu_id)
            if len(payload) != count:
                raise UnsupportedDevice("Short NAND transmit DMA source read")
        else:
            count, payload, source = 2048, pio_payload, self.BASE + 0x400
            if len(payload) != count or command != 0x802064 or self._pio_program_cache is None:
                raise UnsupportedDevice('Incomplete NAND PIO page confirmation')
        if row * 2048 + count > self.FLASH_SIZE:
            raise UnsupportedDevice("NAND programming exceeds type-1 geometry")
        # 0x979b98 packs each four source metadata bytes big-endian into
        # +40..4c. Their placement in the first16 OOB bytes is a named
        # model inference, matching the application's actual spare decoder.
        spare_prefix = b"".join(self.regs.get(offset, 0xffffffff).to_bytes(4, "big")
                                for offset in (0x40, 0x44, 0x48, 0x4c))
        prefix_request = spare_prefix + b"\xff" * (512 - len(spare_prefix))
        page_jobs = []
        for n in range(count // 2048):
            page = row + n
            before = self._read_flash(page * 2048, 2048)
            requested = payload[n * 2048:(n + 1) * 2048]
            spare_before = self._read_spare(page)
            after = bytes(a & b for a, b in zip(before, requested))
            spare_after = bytes(a & b for a, b in zip(spare_before, prefix_request))
            invalid_bits = sum((b & ~a).bit_count() for a, b in zip(before, requested))
            invalid_bits += sum((b & ~a).bit_count() for a, b in zip(spare_before[:16], spare_prefix))
            page_jobs.append((page, after, spare_after, invalid_bits, {
                "page": page, "before_sha256": hashlib.sha256(before).hexdigest(),
                "requested_sha256": hashlib.sha256(requested).hexdigest(),
                "after_sha256": hashlib.sha256(after).hexdigest(),
                "spare_before_sha256": hashlib.sha256(spare_before).hexdigest(),
                "spare_after_sha256": hashlib.sha256(spare_after).hexdigest(),
                "changed_data_bytes": sum(a != b for a, b in zip(before, after)),
                "changed_spare_bytes": sum(a != b for a, b in zip(spare_before, spare_after)),
                "attempted_zero_to_one_bits": invalid_bits}))
        generation = self.generation
        self.busy = True
        self.jobs_started += 1
        self.regs[0x04] = self.regs.get(0x04, 0) | 4
        self.regs[0x54] = self.regs.get(0x54, 0) & ~1
        if pio_payload is None:
            self.regs[0x810] = 1
        self.last_job = {"cpu": cpu_id, "command": hex(command), "operation": "program",
                         "flash_offset": hex(row * 2048), "bytes": count,
                         "source": hex(source), "source_sha256": hashlib.sha256(payload).hexdigest(),
                         "spare_prefix": spare_prefix.hex(), "metadata70": hex(self.regs.get(0x70, 0)),
                         "backend": "volatile only; immutable DAT/storage fixtures",
                         "provenance": "unchanged firmware 0x9af328 -> 0x979940 command802362"}
        if pio_payload is not None:
            self.last_job["provenance"] = "stock four-sector PIO aperture sequence with final program confirm; ideal existing data/OOB law"
        job = dict(self.last_job)
        self.record("NAND volatile program start", **job)

        def complete():
            if generation != self.generation:
                return
            failed = False
            for page, data, spare, invalid_bits, effect in page_jobs:
                self.volatile_pages[page] = data
                self.volatile_spare_pages[page] = spare
                failed |= bool(invalid_bits)
                self.program_effects.append({**job, **effect})
            if pio_payload is None:
                self.regs[0x824] = source + count
                self.regs[0x828] -= count
                self.regs[0x810] = 0
            else:
                self._pio_program_cache = None
            self.busy = False
            self.regs[0x04] &= ~4
            self.regs[0x54] = self.regs.get(0x54, 0) | 1
            # 0x979aec extracts the first status byte from bits24..31;
            # 0x9af728 treats its bit0 as program failure. Assigning an
            # illegal0->1 request this failure is an explicit model rule.
            self.regs[0x28] = (1 << 24) if failed else 0
            self.regs[0xc8] = 0
            self.jobs_completed += 1
            self.transaction_counts["program"] += 1
            self.program_bytes_completed += count if pio_payload is None else 512
            self.record("NAND volatile program complete", **job, failed=failed,
                        changed_pages=len(page_jobs))
            self._completion_irq(cpu_id)

        self.schedule(max(2, count // 512), complete)

    def _completion_irq(self, cpu_id):
        if self.regs[0x54] & 8:
            if self.raise_irq is None:
                raise UnsupportedDevice("NAND completion IRQ requires an interrupt-controller route")
            self.raise_irq(cpu_id, 0x50)
            self.nand_irqs_raised += 1
            self.record("NAND completion IRQ raised", cpu=cpu_id, line=hex(0x50))

    def _start_erase(self, cpu_id, command):
        """Bounded type1 block erase from observed MMIO setup, never RAM state."""
        row = self.regs.get(0x24, 0)
        if (self.regs.get(0x1c) != 0x60d07000 or self.regs.get(0x18) != 0x8093
                or self.regs.get(0x20, 0) or row % 64
                or not 0 <= row < self.FLASH_SIZE // 2048):
            raise UnsupportedDevice('Unknown NAND block erase setup/alignment')
        generation = self.generation
        self.busy = True
        self.jobs_started += 1
        self.regs[0x04] = self.regs.get(0x04, 0) | 4
        self.regs[0x54] = self.regs.get(0x54, 0) & ~1
        job = dict(cpu=cpu_id, command=hex(command), operation='erase',
                   first_page=row, pages=64, bytes=0x20000, erase_ticks=self._erase_ticks,
                   limits='Ideal data/OOB erase; no bad blocks or electrical timing; immutable DAT/fixtures preserved')
        self.last_job = dict(job)
        self.record('NAND block erase start', **job)

        def complete():
            if generation != self.generation:
                return
            for page in range(row, row + 64):
                self.volatile_pages[page] = b'\xff' * 2048
                self.volatile_spare_pages[page] = b'\xff' * 512
            self.busy = False
            self.regs[0x04] &= ~4
            self.regs[0x54] = self.regs.get(0x54, 0) | 1
            self.regs[0x28] = 0
            self.regs[0xc8] = 0
            self.jobs_completed += 1
            self.transaction_counts['erase'] = self.transaction_counts.get('erase', 0) + 1
            self.record('NAND block erase complete', **job)
            self._completion_irq(cpu_id)
        self.schedule(self._erase_ticks, complete)

    def _start(self, cpu_id, command):
        if self.busy:
            raise UnsupportedDevice("NAND command while prior transaction is busy")
        if self.regs.get(0x00) != 1:
            raise UnsupportedDevice("Only ROM/loader NAND type-1 geometry is modeled")
        if command in (0x882062, 0x882004, 0x802064):
            self._start_pio_program(cpu_id, command)
            return
        if self._pio_program_cache is not None:
            raise UnsupportedDevice('NAND command interrupts an unconfirmed PIO program page')
        if command == 0x802362:
            self._start_program(cpu_id, command)
            return
        if command in (0x800006, 0x800046, 0x802046):
            self._start_erase(cpu_id, command)
            return
        if command in (0x802042, 0x802242):
            raise UnsupportedDevice("Virtual NAND programming is not implemented")
        reset = command in (0x800007, 0x802007)
        spare = command in (0x800001, 0x802001, 0x802041)
        pio = command in (0x800033, 0x802033, 0x8020f3)
        rom_dma = command in (0x8013b3, 0x8033b3)
        verify_dma = command == 0x8023c3
        dma = command == 0x8023f3 or verify_dma or rom_dma
        if not (reset or spare or pio or dma):
            raise UnsupportedDevice(f"Unknown NAND command {command:#x}")
        if reset:
            if self.regs.get(0x1c) != 0xff007000:
                raise UnsupportedDevice("Unknown NAND reset command sequence")
            offset, count, data, destination = 0, 0, b"", None
        else:
            row, column = self.regs.get(0x24, 0), self.regs.get(0x20, 0)
            offset = row * 2048
            if offset >= self.FLASH_SIZE:
                raise UnsupportedDevice("NAND row outside type-1 geometry")
            if spare:
                if self.regs.get(0x1c) != 0x300000 or column != 0x200:
                    raise UnsupportedDevice("Unknown NAND spare read setup")
                count, destination = 512, None
                data = self._read_spare(row)
            elif pio:
                if self.regs.get(0x1c) not in (0x300000, 0x300001) or column:
                    raise UnsupportedDevice("Unknown NAND PIO read setup")
                count, destination = 512, None
                data = self._read_flash(offset, count)
            else:
                setup = self.regs.get(0x1c, 0)
                receive_config = self.regs.get(0x8a0, 0)
                app_receive = bool(receive_config & 0x80000000)
                if verify_dma and not app_receive:
                    raise UnsupportedDevice("NAND verify RX requires the observed application DMA mode")
                units = setup & 0x1ff
                # 0x9af7fc caps a burst at 256 sectors, then 0x979a20
                # inserts only its low 8 bits. Encoded 0 means 256 here.
                if not units and app_receive:
                    units = 256
                count = units * 512
                if setup & ~0x1ff != 0x300000 or not count or count > 0x20000 or column:
                    raise UnsupportedDevice("Unknown NAND DMA transfer size/setup")
                remaining = self.regs.get(0x8a8, 0)
                if self.regs.get(0x884) != 0x10001 or remaining < count:
                    raise UnsupportedDevice("NAND receive DMA is not enabled/configured")
                if receive_config & ~0x90700000 != 0x1000:
                    raise UnsupportedDevice("Unknown NAND receive DMA width mode")
                if app_receive and (receive_config >> 20) & 7 not in (3, 4):
                    raise UnsupportedDevice("Unknown application NAND DMA width")
                if receive_config & 0x10000000:
                    # Completion IRQ timing/routing has not yet been connected
                    # to the interrupt model. Never pretend that IRQ ran.
                    raise UnsupportedDevice("Application NAND DMA completion IRQ is not modeled")
                if rom_dma and (count != 2048 or self.regs.get(0x8a0) != 0x401000):
                    raise UnsupportedDevice("Unknown type-1 ROM page DMA setup")
                if not rom_dma and remaining != 0x100000:
                    raise UnsupportedDevice("Unknown loader NAND DMA receive limit")
                destination = self.regs.get(0x8a4, 0)
                data = self._read_flash(offset, count)
        generation = self.generation
        operation = "reset" if reset else "spare" if spare else "pio" if pio else "dma"
        padding_bytes, overlay_bytes, volatile_bytes = (0, 0, 0) if reset or spare else self._read_source_counts(offset, count)
        spare_profile = self.spare_pages.get(self.regs.get(0x24, 0)) if spare else None
        self.busy = True
        self.jobs_started += 1
        self.last_job = {"cpu": cpu_id, "command": hex(command), "operation": operation,
                         "flash_offset": hex(offset), "bytes": count,
                         "destination": None if destination is None else hex(destination),
                         "erased_padding_bytes": padding_bytes,
                         "data_overlay_bytes": overlay_bytes,
                         "volatile_program_bytes": volatile_bytes,
                         "spare_provenance": spare_profile["provenance"] if spare_profile else None}
        self.regs[0x04] = self.regs.get(0x04, 0) | 4
        self.regs[0x54] = self.regs.get(0x54, 0) & ~1
        if destination is not None:
            # 0x9acd94 waits for channel +10 bit0 to clear after disabling
            # +04 bit0. Active status tracks the actual queued RX transfer.
            self.regs[0x890] = 1
        self.record("NAND transaction start", cpu=cpu_id, command=hex(command),
                    flash_offset=hex(offset), bytes=count,
                    destination=None if destination is None else hex(destination), spare_fixture=spare,
                    data_overlay_bytes=overlay_bytes,
                    volatile_program_bytes=volatile_bytes,
                    spare_provenance=spare_profile["provenance"] if spare_profile else None,
                    erased_padding=bool(padding_bytes))
        def complete():
            if generation != self.generation:
                return
            if destination is not None:
                self.write_bytes(destination, data, cpu_id=cpu_id)
                self.regs[0x8a4] = destination + count
                self.regs[0x8a8] -= count
                self.regs[0x890] = 0
            elif data:
                self.aperture[:] = data
            self.busy = False
            self.regs[0x04] &= ~4
            self.regs[0x54] = self.regs.get(0x54, 0) | 1
            self.regs[0x28] = 0
            self.regs[0xc8] = 0
            self.jobs_completed += 1
            self.transaction_counts[operation] += 1
            if not (reset or spare):
                self.data_bytes_completed += count
                self.erased_padding_bytes_completed += padding_bytes
                self.data_overlay_bytes_completed += overlay_bytes
                self.volatile_data_bytes_read += volatile_bytes
            self.record("NAND transaction complete", cpu=cpu_id, command=hex(command), bytes=count)
            # Logical4b->physical50 in0xaeafe0; real callback0x9abd58
            # belongs to the kernel and is not executed by the model.
            self._completion_irq(cpu_id)
        self.schedule(max(2, count // 512), complete)
