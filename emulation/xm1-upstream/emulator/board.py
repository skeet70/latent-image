"""Shared X70 board services for Rust-owned CPU execution.

The platform retains scheduling, MMIO dispatch, mapping identity and bounded
architectural exception entry. CPU construction and instruction hooks belong
to x70_native; this module cannot instantiate a Python execution engine.
"""
import ctypes
import heapq
import os
from pathlib import Path
import sys

from unicorn import (UcError, UC_PROT_ALL, UC_PROT_READ, UC_PROT_EXEC,
                     UC_MEM_WRITE, UC_ERR_INSN_INVALID)
from unicorn.arm_const import (UC_ARM_REG_PC, UC_ARM_REG_CPSR, UC_ARM_REG_SPSR,
    UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_R0, UC_ARM_REG_FPEXC,
    UC_ARM_REG_FPSCR, UC_ARM_REG_C1_C0_2)
from .hardware import UnsupportedDevice
from .boards.bindings import MemoryBus, firmware_bindings

ROOT = Path(__file__).resolve().parents[1]
if str(ROOT/'tools') not in sys.path:
    sys.path.append(str(ROOT/'tools'))
import firmware_registry  # noqa: E402

from .boards.profile import resolve

BOARD_FIRMWARE = os.environ.get('XM1_VIEWER_INITIAL_FIRMWARE', 'xm1-150')
_PROFILE = resolve(BOARD_FIRMWARE)
ROM_FILE = _PROFILE.rom_file
ROM_HASH = _PROFILE.rom_hash
RAM_SIZE = _PROFILE.ram_size
EXTRA_MMIO_RANGES = _PROFILE.extra_mmio_ranges
ENTRY_LABELS = _PROFILE.entry_labels

_FIRMWARE_BINDINGS = firmware_bindings(_PROFILE)




# Upper bound on quanta run natively in one quiet batch (see _QuietBatch).
QUIET_BATCH_QUANTA = 4096


class _QuietBatch:
    """Consecutive quanta of one core run as a single native run.

    run() starts one only when every boundary before its end is provably
    quiet without Python: the other core stays parked (not runnable, or
    waiting with nothing pending for it), this core has no IRQ candidate
    (so per-quantum injection is a no-op), no scheduled callback or device
    event is due by those boundaries' ticks, and the pause threshold and run
    limit lie at or after its end. Those boundaries would only have deferred
    their device ticks; the latest one is deferred here instead, at its exact
    tick. Any native event makes the remaining boundaries unprovable: the
    handler defers the boundary just passed and clamps the run to end at the
    current quantum's boundary, which run() then processes as usual.
    """
    __slots__ = ('start', 'ticks', 'phase', 'num', 'den', 'quantum', 'end', 'tickers')

    def __init__(self, board, end, tickers):
        self.start, self.ticks, self.phase = board.instructions, board.virtual_ticks, board.tick_phase
        self.num, self.den = board.tick_ratio
        self.quantum, self.end, self.tickers = board.quantum, end, tickers

    def ticks_at(self, instructions):
        """Virtual tick after `instructions` (native tick accounting)."""
        return self.ticks + (self.phase + (instructions - self.start) * self.den) // self.num

    def defer_through(self, board, completed):
        """Defer device ticks to the last boundary at or before `completed`."""
        boundaries = (completed - self.start) // self.quantum
        if boundaries >= 1:
            boundary = self.start + boundaries * self.quantum
            board._ev.deferred = (self.ticks_at(boundary), self.tickers)
        return boundaries

    def interrupt(self, board):
        """A native event arrived: end the run at this quantum's boundary."""
        # The board's count includes the instruction raising the event.
        boundaries = self.defer_through(board, board.instructions - 1)
        stop = self.start + (boundaries + 1) * self.quantum
        if stop < self.end:
            self.end = stop
            board._native.clamp_run(stop)

    @staticmethod
    def plan(board, core, end, start, limit, pause_after, due):
        """End of a quiet batch beginning now, or None for one ordinary quantum."""
        other = board.core_state[1 - core]
        interrupts = getattr(board, 'interrupts', None)
        if interrupts is None or due is None:
            return None
        bank = interrupts.banks[1 - core]
        if other['runnable'] and not (other['waiting'] and not bank.pending & bank.enabled):
            return None
        # Controllers without irq_candidate() keep one quantum per run.
        candidate = getattr(interrupts, 'irq_candidate', None)
        if candidate is None or candidate(core):
            return None
        num, den = board.tick_ratio
        quantum, now, ticks, phase = board.quantum, board.instructions, board.virtual_ticks, board.tick_phase
        def first_boundary(tick):
            # Smallest j >= 1 whose boundary tick reaches `tick`.
            need = (tick - ticks) * num - phase
            return 1 if need <= 0 else max(1, -(-need // (quantum * den)))
        quanta = min(QUIET_BATCH_QUANTA, first_boundary(due))
        if board.scheduled:
            quanta = min(quanta, first_boundary(board.scheduled[0][0]))
        if pause_after is not None:
            quanta = min(quanta, max(1, -(-(pause_after - now) // quantum)))
        batch_end = min(now + quanta * quantum, start + limit)
        return batch_end if quanta > 1 and batch_end > end else None

class BoardPlatform:
    """Engine-neutral services inherited by the sole Rust executor."""
    _reset_release = _FIRMWARE_BINDINGS.reset
    def __init__(self, *args, **kwargs):
        raise TypeError("BoardPlatform supplies shared board services; construct RustBoard for execution")

    def _track_memory_layout(self,cpu):
        """Invalidate checkpoint backing identity after host mapping changes.

        Unicorn's mem_regions reports ranges/permissions, but cannot identify
        replacement allocations at identical addresses. Track the supported
        Python mapping APIs too. Only _remap_low_alias's coordinated full
        low16KiB known-pointer transaction is restorable. Arbitrary changes
        inside the same address range invalidate identity as well. Native
        FFI bypass is unsupported.
        """
        self.memory_layout_tracking_contract='coordinated-low-alias-v2'
        if not hasattr(self,'memory_layout_origin_token'):
            self.memory_layout_origin_token=self.memory_layout_token
        if not hasattr(self,'_restorable_low_remap'):
            self._restorable_low_remap=None
        for name in ('mem_map','mem_map_ptr','mem_unmap','mem_protect'):
            original=getattr(cpu,name)
            def tracked(address,size,*args,_original=original,_name=name,**kwargs):
                result=_original(address,size,*args,**kwargs)
                known=getattr(self,'_restorable_low_remap',None)
                restorable=(known is not None and address==0 and size==0x4000
                    and not kwargs and ((_name=='mem_unmap' and not args) or
                        (_name=='mem_map_ptr' and args==known)))
                if not restorable:
                    self.memory_layout_token=object()
                return result
            setattr(cpu,name,tracked)

    def _remap_low_alias(self,rom_alias):
        """Coordinate a paused full low16KiB remap to owned ROM or DRAM.

        Checkpoint restore and the ROM handoff use this same transaction.
        A failed/partial transaction invalidates checkpoint backing identity.
        No arbitrary pointer, width, protection or CPU subset is accepted.
        """
        if self.executing_cpu is not None or getattr(self,'_restorable_low_remap',None) is not None:
            raise UnsupportedDevice('Low ROM/RAM alias remap requires paused CPU engines')
        if not isinstance(rom_alias,bool):
            raise ValueError('Low alias selection must be boolean')
        pointer=ctypes.addressof(self.rom_storage) if rom_alias else self.ram_pointer
        protection=UC_PROT_READ|UC_PROT_EXEC if rom_alias else UC_PROT_ALL
        self._restorable_low_remap=(protection,pointer)
        completed=False
        try:
            for cpu in self.cpus:
                cpu.mem_unmap(0,0x4000)
                cpu.mem_map_ptr(0,0x4000,protection,pointer)
            completed=True
        finally:
            self._restorable_low_remap=None
            if not completed:
                self.memory_layout_token=object()

    @property
    def memory_bus(self):
        # A stateless view, kept outside checkpoint/fingerprint dictionaries.
        return MemoryBus(self)

    def read_memory(self, address, size, cpu_id=0):
        return self.memory_bus.read(address, size, cpu_id)


    def remap_low_ram(self):
        if self.low_rom_alias and self.executing_cpu is not None:
            # Unicorn must not invalidate its current MMIO TLB entry by
            # changing mappings inside a live guest store callback. Yield
            # after that store, then remap before the next firmware instruction.
            self.remap_requested=True
            return
        if self.low_rom_alias:
            self.invalidate_execution_caches()
            self._remap_low_alias(False)
            for cpu in self.cpus:
                cpu.ctl_remove_cache(0,0x4000)
            self.low_rom_alias=False
            self.remap_requested=False
            self.record('ROM low alias replaced with DRAM')

    def attach(self, device):
        self.devices.append(device)
        return device

    def record(self, name, **data):
        self.event_log.append({'tick': self.virtual_ticks, 'name': name, **data})
        if len(self.event_log)>20000:
            del self.event_log[:10000]

    # Boards that log every MMIO transaction (the driver harness) turn this off.
    repeatable_mmio_reads = True

    def _native_fast_execution(self):
        native=getattr(self,'_native',None)
        return native is not None and native.execution()=='fast'

    def flush_deferred_ticks(self):
        """Run the device ticks deferred from quiet quantum boundaries.

        run() skips a boundary's tick round when the quantum made no native
        event (MMIO, WFI, milestone, ...), nothing scheduled is due and no
        device reports an event at or before that tick. Every attached
        ticking device then declares ``quiet_ticks_defer``: a tick at such a
        boundary only advances elapsed-time bookkeeping, which one later tick
        reproduces exactly. The latest skipped boundary is replayed here at
        its own virtual tick before anything can observe device state. The
        pending boundary (virtual tick, ticking devices) is kept in the
        board's `_ev` bookkeeping, outside board state; only boards with it
        defer.
        """
        events = getattr(self, '_ev', None)
        if events is None or events.deferred is None:
            return
        tick, tickers = events.deferred
        events.deferred = None
        now, order = self.virtual_ticks, self.schedule_order
        last = self.event_log[-1] if self.event_log else None
        banks = getattr(getattr(self, 'interrupts', None), 'banks', ())
        pending = [bank.pending for bank in banks]
        self.virtual_ticks = tick
        try:
            for device in tickers:
                device.tick(self)
        finally:
            self.virtual_ticks = now
        if (self.schedule_order != order or (self.event_log[-1] if self.event_log else None) is not last
                or [bank.pending for bank in banks] != pending):
            raise RuntimeError('A deferred quiet device tick scheduled, recorded or raised an event; '
                               'its class must not declare quiet_ticks_defer')

    def schedule(self, delay_ticks, callback):
        self.schedule_order += 1
        heapq.heappush(self.scheduled, (self.virtual_ticks+delay_ticks, self.schedule_order, callback))

    def queue_irq(self, cpu_id, line):
        self.interrupts.raise_irq(cpu_id,line)

    def release_cpu(self, cpu_id, pc):
        self.cpus[cpu_id].reg_write(UC_ARM_REG_PC,pc)
        self.core_state[cpu_id]['runnable']=True
        self.core_state[cpu_id]['reset_released']=True
        self.record('CPU released',cpu=cpu_id,pc=hex(pc))

    def _stop(self, cpu, core, message, **details):
        self.failure = {'message': message, 'cpu': core,
                        'pc': hex(cpu.reg_read(UC_ARM_REG_PC)), **details}
        cpu.emu_stop()

    def _invalid(self, cpu, kind, address, size, value, core):
        self._stop(cpu, core, 'Unmapped or protected memory access',
                   kind=kind, address=hex(address), size=size, value=hex(value))
        return False

    def _native_interrupt(self,cpu,number,core):
        """Retain native synchronous fault identity, then yield out of TCG.

        Unicorn reports exception3 before AArch32 abort entry or copying its
        internal fault fields to IFSR/IFAR. Unknown exceptions still stop.
        No CPU context/mapping changes are made inside this callback.
        """
        if number!=3:
            self._stop(cpu,core,'Unsupported native CPU exception',exception=number)
            return
        self.core_state[core]['pending_native_exception']=dict(number=number,
            pc=cpu.reg_read(UC_ARM_REG_PC),cpsr=cpu.reg_read(UC_ARM_REG_CPSR))
        cpu.emu_stop()

    def _prefetch_permission_region(self,cpu,pc,old):
        """Verify an actual PMSAv7 permission denial, retaining its selector."""
        selected=cpu.cpr_read(15,0,6,2,0,0,False)
        count=(cpu.cpr_read(15,0,0,0,4,0,False)>>8)&0xff
        if not 1<=count<=16:
            raise UnsupportedDevice('Native prefetch abort has an unsupported MPU region count')
        matched=None
        try:
            for region in range(count-1,-1,-1):
                cpu.cpr_write(15,0,6,2,0,0,False,region)
                base=cpu.cpr_read(15,0,6,1,0,0,False)
                size=cpu.cpr_read(15,0,6,1,2,0,False)
                access=cpu.cpr_read(15,0,6,1,4,0,False)
                if not size&1:continue
                exponent=(size>>1)&31
                if exponent<4:raise UnsupportedDevice('Native prefetch abort has an invalid enabled MPU region size')
                length=1<<(exponent+1)
                if base&(length-1):raise UnsupportedDevice('Native prefetch abort has a misaligned MPU region base')
                if not base<=pc<base+length:continue
                subregion=(pc-base)//(length//8)
                if length>=256 and size&(1<<(8+subregion)):continue
                ap=(access>>8)&7;xn=bool(access&(1<<12))
                if ap not in (0,1,2,3,5,6):
                    raise UnsupportedDevice('Native prefetch abort has reserved MPU access permissions')
                denied=xn or (ap in (0,1,5) if old&0x1f==0x10 else ap==0)
                if denied:matched=dict(region=region,base=hex(base),size=hex(size),access=hex(access),
                    bytes=length,subregion=subregion,ap=ap,execute_never=xn)
                break  # Highest matching enabled region wins, even if it allows access.
        finally:
            cpu.cpr_write(15,0,6,2,0,0,False,selected)
        return matched

    def _enter_native_prefetch_abort(self,cpu,core):
        pending=self.core_state[core].get('pending_native_exception')
        if pending is None:return False
        pc=cpu.reg_read(UC_ARM_REG_PC);old=cpu.reg_read(UC_ARM_REG_CPSR)
        sctlr=cpu.cpr_read(15,0,1,0,0,0,False)
        if (pending['number']!=3 or pending['pc']!=pc or pending['cpsr']!=old
                or pc&3 or old&(0x20|0x01000000|0x200) or old&0x1f not in (0x10,0x13,0x1f)
                or not sctlr&1 or sctlr&((1<<30)|(1<<25))):
            raise UnsupportedDevice('Native prefetch abort is outside the validated ARM/nonsecure MPU contract')
        if not any(begin<=pc<=end and protection&UC_PROT_EXEC for begin,end,protection in cpu.mem_regions()):
            raise UnsupportedDevice('Native prefetch abort is not backed by executable host memory')
        region=self._prefetch_permission_region(cpu,pc,old)
        if region is None:
            raise UnsupportedDevice('Native prefetch abort is not explained by an actual MPU permission denial')
        # PMSAv7 permission fault status0xD, matching the stock8A073C handler.
        # These are architectural fault fields, not a peripheral-ready fixture.
        cpu.cpr_write(15,0,5,0,1,0,False,0xd)
        cpu.cpr_write(15,0,6,0,2,0,False,pc)
        vector=0xffff000c if sctlr&(1<<13) else 0xc
        new=(old&~(0x1f|0x20|0x0600fc00|0x01000000|0x200))|0x17|0x180
        cpu.reg_write(UC_ARM_REG_CPSR,new)
        cpu.reg_write(UC_ARM_REG_SPSR,old)
        cpu.reg_write(UC_ARM_REG_LR,(pc+4)&0xffffffff)
        cpu.reg_write(UC_ARM_REG_PC,vector)
        self.exclusive[core]=None
        state=self.core_state[core];state['pending_native_exception']=None
        state['prefetch_abort_entries']=state.get('prefetch_abort_entries',0)+1
        self.record('Architectural MPU permission prefetch abort entry',cpu=core,fault_pc=hex(pc),
            native_exception=3,vector=hex(vector),old_cpsr=hex(old),new_cpsr=hex(new),
            lr_abt=hex((pc+4)&0xffffffff),ifsr='0xd',ifar=hex(pc),sctlr=hex(sctlr),
            cp_register_bank='nonsecure (native sec=0; Python labels this argument el)',mpu=region,
            limit='ARM nonsecure PMSAv7 permission entry only; firmware handles loading/retry; no MPU permission or task changes')
        return True

    def _enter_lazy_vfp_undefined(self,cpu,core):
        """Enter the actual ARM undefined vector for a disabled VFP operation.

        Bounded to ARM-state CP10/11 operations with access granted in CPACR
        and FPEXC.EN clear. No handler, task ID, register image or enable bit
        is substituted: the installed stock vector/handler runs normally.
        """
        old=cpu.reg_read(UC_ARM_REG_CPSR)
        if old&0x20 or old&0x1f==0x1b:
            return False
        pc=cpu.reg_read(UC_ARM_REG_PC)
        raw=bytes(cpu.mem_read(pc,4))
        word=int.from_bytes(raw,'little')
        if (word>>28==15 or (word>>8)&15 not in (10,11)
                or word&0x0e000000 not in (0x0c000000,0x0e000000)):
            return False
        cpacr=cpu.reg_read(UC_ARM_REG_C1_C0_2)
        fpexc=cpu.reg_read(UC_ARM_REG_FPEXC)
        if cpacr&0x00f00000!=0x00f00000 or fpexc&0x40000000:
            return False
        sctlr=cpu.cpr_read(15,0,1,0,0,0,False)
        if sctlr&((1<<30)|(1<<25)):
            raise UnsupportedDevice('Thumb/big-endian undefined exception entry is not implemented')
        vector=0xffff0004 if sctlr&(1<<13) else 4
        # ARMv7-R: banked UND LR points one ARM instruction beyond the fault;
        # the stock 40100/D0100 prologue subtracts4 before MOVS PC,LR retries.
        new=(old&~(0x1f|0x20|0x0600fc00|0x01000000|0x200))|0x1b|0x80
        cpu.reg_write(UC_ARM_REG_CPSR,new)
        cpu.reg_write(UC_ARM_REG_SPSR,old)
        cpu.reg_write(UC_ARM_REG_LR,(pc+4)&0xffffffff)
        cpu.reg_write(UC_ARM_REG_PC,vector)
        self.exclusive[core]=None
        state=self.core_state[core]
        state['undefined_entries']=state.get('undefined_entries',0)+1
        self.record('Architectural undefined instruction entry',cpu=core,fault_pc=hex(pc),
                    vector=hex(vector),word=hex(word),old_cpsr=hex(old),
                    cpacr=hex(cpacr),fpexc=hex(fpexc),
                    cause='VFP disabled; stock kernel handler must initialize/restore it')
        return True









    def _mmio_device(self, address, size):
        """The device that owns an access: the first attached device whose
        contains() accepts it, or None. RustBoard overrides this with an
        address index that returns exactly the same device."""
        return next((d for d in self.devices if d.contains(address,size)),None)

    def _mmio(self, cpu, kind, address, size, value, core):
        try:
            device = self._mmio_device(address,size)
            if device is None:
                raise UnsupportedDevice(f'Unknown MMIO {address:#x}')
            if kind == UC_MEM_WRITE:
                device.write(core,address,size,value)
                # The loader copies CPU1's reset-vector island, then pulses
                # software source 120. Reset release is a board assumption,
                # distinct from delivery of a later architectural IRQ.
                release=self._reset_release
                if (address==release.register and value&release.mask and
                        not self.core_state[1]['runnable'] and
                        not self.core_state[1].get('reset_released', self.core_state[1].get('started', False))):
                    if any(self.read_memory(at,len(expected))!=expected for at,expected in release.checks):
                        raise UnsupportedDevice('CPU1 release without expected stock reset vectors')
                    self.record('Assumed secondary reset release',source=release.source,
                                evidence=f'Loader vector copy and source-{release.source} pulse; physical reset protocol unmeasured')
                    self.release_cpu(1,release.target)
            else:
                result=device.read(core,address,size)
                cpu.mem_write(address,int(result).to_bytes(size,'little'))
                # A device's repeatable_reads are side-effect free and depend
                # only on its own state, which changes only when Python runs:
                # the engine may answer repeats from the value just written
                # until its next event (x70_native Machine.cache_mmio_read).
                repeatable=getattr(device,'repeatable_reads',None)
                if (repeatable and self.repeatable_mmio_reads and address in repeatable
                        and self.failure is None):
                    self._native.cache_mmio_read(core,address,size)
        except UnsupportedDevice as exc:
            self._stop(cpu,core,str(exc),address=hex(address),size=size,
                       operation='write' if kind==UC_MEM_WRITE else 'read',value=hex(value))


    def run(self, limit=100000, *, pause_after=None):
        """Advance execution; optional absolute threshold pauses at a clean cycle.

        pause_after does not shorten a CPU quantum. The hard instruction limit
        should leave at least two quanta beyond the requested threshold. A true
        _checkpoint_boundary means callbacks/ticks finished and the next run
        may safely begin its ordinary CPU0-first scheduling cycle.
        """
        if pause_after is not None and (isinstance(pause_after,bool)
                or not isinstance(pause_after,int) or pause_after<0):
            raise ValueError('pause_after must be a nonnegative instruction count')
        start=self.instructions
        self.quiescent=False
        self._checkpoint_boundary=False
        # Quiet-boundary deferral needs the native event counter and devices
        # that all declare quiet_ticks_defer (see flush_deferred_ticks).
        events=getattr(self,'_ev',None)
        try:
            self._run_loop(start,limit,pause_after,events)
        finally:
            self.flush_deferred_ticks()
        return self.snapshot()

    def _run_loop(self,start,limit,pause_after,events):
        # Devices with tick(), in attach order; rebuilt if a device is attached.
        tickers,attached=[],-1
        deferrable,due=False,None
        while self.instructions-start<limit and not self.failure:
            active=False
            for core,cpu in enumerate(self.cpus):
                if not self.core_state[core]['runnable']:
                    continue
                if self.core_state[core]['waiting']:
                    bank=self.interrupts.banks[core] if hasattr(self,'interrupts') else None
                    if bank is None or not bank.pending&bank.enabled:
                        continue
                    self.core_state[core]['waiting']=False
                    self.record('CPU woke from interrupt signal',cpu=core)
                active=True
                self.active_cpu=core
                self.core_state[core]['started']=True
                natural_end=self.instructions+self.quantum
                seen=events.events if events is not None else None
                batch=None
                try:
                    if hasattr(self,'interrupts'):
                        if self.interrupts.inject(core,cpu):
                            self.exclusive[core]=None
                            seen=None
                    end=min(natural_end,start+limit)
                    if deferrable and seen is not None and self._native_fast_execution():
                        batch_end=_QuietBatch.plan(self,core,end,start,limit,pause_after,due)
                        if batch_end is not None:
                            batch=events.batch=_QuietBatch(self,batch_end,tickers)
                            end=batch_end
                    while self.instructions<(end:=batch.end if batch is not None else end):
                        begin=cpu.reg_read(UC_ARM_REG_PC)|(1 if cpu.reg_read(UC_ARM_REG_CPSR)&0x20 else 0)
                        self.executing_cpu=core
                        recovered=False
                        try:
                            cpu.emu_start(begin,0x0fff0000,count=end-self.instructions)
                        except UcError as exc:
                            self.executing_cpu=None
                            if (exc.errno==UC_ERR_INSN_INVALID and not self.failure
                                    and self._enter_lazy_vfp_undefined(cpu,core)):
                                recovered=True
                            else:
                                raise
                        finally:
                            self.executing_cpu=None
                        if not self.failure and self._enter_native_prefetch_abort(cpu,core):
                            recovered=True
                        maintenance=bool(self._execution_invalidations[0] or self._execution_invalidations[1])
                        self._flush_execution_invalidations()
                        if batch is not None:
                            # An event may have clamped the batch during that run.
                            end=batch.end
                        # Cache maintenance is invisible to the guest scheduler:
                        # finish this same quantum before callbacks, device ticks
                        # or another CPU's IRQ injection. Existing ROM-remap and
                        # WFI yields retain their original scheduler behavior.
                        if (not (maintenance or recovered) or self.remap_requested or self.failure
                                or self.core_state[core]['waiting']):
                            break
                except UcError as exc:
                    if not self.failure:
                        self._stop(cpu,core,'CPU engine exception',error=str(exc))
                except UnsupportedDevice as exc:
                    self._stop(cpu,core,str(exc))
                finally:
                    self.executing_cpu=None
                    if batch is not None:
                        natural_end=batch.end
                        if events.batch is batch:
                            # No event: every boundary before here was quiet.
                            events.batch=None
                            batch.defer_through(self,self.instructions-1)
                            if (self.instructions==batch.end and not self.failure
                                    and self.virtual_ticks!=batch.ticks_at(batch.end)):
                                raise RuntimeError('Quiet batch tick accounting differs from the native count')
                self._flush_execution_invalidations()
                natural_yield=self.remap_requested or self.core_state[core]['waiting']
                if self.remap_requested:
                    self.remap_low_ram()
                if len(self.devices)!=attached:
                    tickers,attached=[d for d in self.devices if hasattr(d,'tick')],len(self.devices)
                    deferrable=events is not None and all(getattr(d,'quiet_ticks_defer',False) for d in tickers)
                    due=None
                if (deferrable and seen is not None and seen==events.events and not natural_yield
                        and due is not None and due>self.virtual_ticks and not self.failure
                        and not (self.scheduled and self.scheduled[0][0]<=self.virtual_ticks)):
                    events.deferred=(self.virtual_ticks,tickers)
                else:
                    self.flush_deferred_ticks()
                    while self.scheduled and self.scheduled[0][0]<=self.virtual_ticks:
                        _,_,callback=heapq.heappop(self.scheduled)
                        callback()
                    for device in tickers:
                        device.tick(self)
                    if deferrable:
                        due=None
                        for device in tickers:
                            tick=device.next_event_tick(self.virtual_ticks)
                            if tick is not None and (due is None or tick<due):
                                due=tick
                        if due is None:
                            due=1<<64
                self.last_quantum_complete=self.instructions>=natural_end or natural_yield
                if (pause_after is not None and self.instructions>=pause_after
                        and self.last_quantum_complete and not self.failure):
                    secondary=self.core_state[1]
                    bank=self.interrupts.banks[1] if hasattr(self,'interrupts') else None
                    secondary_idle=(not secondary['runnable'] or (secondary['waiting']
                                    and (bank is None or not bank.pending&bank.enabled)))
                    if core==1 or secondary_idle:
                        self.flush_deferred_ticks()
                        self._checkpoint_boundary=True
                        break
                if self.failure or self.instructions-start>=limit:
                    break
            if self._checkpoint_boundary:
                break
            if not active and not self.failure:
                self.flush_deferred_ticks()
                due=None
                future=self.scheduled[0][0] if self.scheduled else None
                for device in self.devices:
                    if hasattr(device,'next_event_tick'):
                        tick=device.next_event_tick(self.virtual_ticks)
                        if tick is not None and (future is None or tick<future):
                            future=tick
                if future is None:
                    self.quiescent=True
                    self.record('All released CPUs waiting; no future modeled event')
                    break
                self.virtual_ticks=max(self.virtual_ticks+1,future)
                while self.scheduled and self.scheduled[0][0]<=self.virtual_ticks:
                    _,_,callback=heapq.heappop(self.scheduled)
                    callback()
                if len(self.devices)!=attached:
                    tickers,attached=[d for d in self.devices if hasattr(d,'tick')],len(self.devices)
                for device in tickers:
                    device.tick(self)

    def snapshot(self):
        return {'entry':self.entry,'firmware_sha256':self.manifest['source_sha256'],
                'rom_sha256':self.memory_bus.rom_hash,'instructions':self.instructions,'virtual_ticks':self.virtual_ticks,
                'dram_bytes':self.memory_bus.ram_size,
                'low_rom_alias_active':self.low_rom_alias,
                'quiescent':self.quiescent,
                'failure':self.failure,'cores':[
                    {**state,'pc':hex(cpu.reg_read(UC_ARM_REG_PC)),
                     'sp':hex(cpu.reg_read(UC_ARM_REG_SP)),
                     'registers':{f'r{i}':hex(cpu.reg_read(UC_ARM_REG_R0+i)) for i in range(13)},
                     'lr':hex(cpu.reg_read(UC_ARM_REG_LR)),
                     'cpsr':hex(cpu.reg_read(UC_ARM_REG_CPSR)),
                     'floating_point':{'cpacr':hex(cpu.reg_read(UC_ARM_REG_C1_C0_2)),
                                       'fpexc':hex(cpu.reg_read(UC_ARM_REG_FPEXC)),
                                       'fpscr':hex(cpu.reg_read(UC_ARM_REG_FPSCR))},
                     'recent_instructions':[{'pc':pc,'bytes':raw} for pc,raw in self.recent[n]]}
                    for n,(cpu,state) in enumerate(zip(self.cpus,self.core_state))],
                'events':self.event_log,'milestones':self.milestones,
                'loader_memory_validation':self.loader_memory_validation,
                'exclusive_model':{'monitors':self.exclusive,'counts':self.exclusive_stats,
                    'limit':'Aligned ARM word LDREX/STREX with 4-byte reservations; physical reservation granule unvalidated.',
                    'exception_policy':'Conservative reservation clear on IRQ entry; physical Cortex-R4 behavior unvalidated.'},
                'limits':['R5F ISA engine with explicit CP14 CPU-ID fixture; R4 semantics not fully validated.',
                          'Two software cores; hardware reset mapping and release are under investigation.',
                          'Unknown MMIO and CPU/device errors stop execution; no RTOS return stubs.']}


def __getattr__(name):
    """Keep legacy Board imports native-only without an import cycle."""
    if name == 'Board':
        from x70_native.board import RustBoard
        return RustBoard
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
