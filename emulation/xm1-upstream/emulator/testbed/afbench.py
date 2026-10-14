"""AF bench: one object that sets up the whole autofocus testbed for a mod author.

    bench = AFBench.create(scene=SyntheticSceneSource(target_position=300, pulses_per_sample=68))
    bench.set_lens(100)                       # stock moves, approaching from infinity
    result = bench.half_press()               # S1 over the debug key until AF done (0x816 / REC21=8, or 0x829)
    result.path, result.ok, result.time_to_lock_s, result.final_position
    bench.release()

`create` loads the REC snapshot, enables the continuous sensor stream (lens motion left to the generic
profile), the generic stock lens moves (focus and iris), the PDAF hardware stream, a `LensTrace`, the
display recorder (`bench.screenshot()`) and light coverage counters of key AF entry points (fetch
guard, entry words only). Virtual seconds use the stream's convention: one virtual tick is one count of
the 12 MHz timer input (`sensor_stream.TICKS_PER_SECOND`), so a virtual second costs about
`12e6 * instructions_per_tick` guest instructions. Everything is declared and unmeasured; the half-press
is stock firmware reacting to the declared scene, lens and frame models. Device latencies and the CPU speed
come from a timing profile (`timing.py`, default `'camera'`, fitted to saved AF-S timing evidence;
`timing='declared'` restores the earlier defaults, research/af-timing/README.md).

RAM patches (`install_patch`, `control`) are built with clang like `tools/x70_focus_sweep.py build()`
and written into a verified-zero heap block; control entries run from the scanner context on CPU0
(`mrc p14` reads 0x4003 on core 0, 0x5003 on core 1 in the native runtime).
"""
from __future__ import annotations

from dataclasses import dataclass, field
import shutil
import struct
import subprocess
import tempfile
from pathlib import Path

from . import events, sensor_stream, timing as timing_profiles
from .lens_trace import LensTrace
from .patch import install_ram_patch, word
from .scene_sources import SyntheticSceneSource
from .testbed import Testbed

TICKS_PER_SECOND = sensor_stream.TICKS_PER_SECOND
POSITION, MOTION, LENS_STATE = 0xccde64, 0xccde90, 0xccde04
UI_PACKET = 0xcb05d8                 # (event, token) of the last UI notification
S1_RESULT_COUNT = 0x162dbac          # u8, incremented by the S1 PDAF callback
OVERSHOOT = 51                       # stock S1 backlash pre-move (cfg 190fb)
REC21_GRACE_TICKS = 1_200_000        # 0.1 virtual s for the 0x816 notification after REC21 reaches 8
AF_DONE = (0x816, 0x829)             # stock AF-S holding route posts 0x816 (REC21=8, REC30=4); 0x829 is also accepted
REC21 = 0x1624240 + 2 * 21           # REC machine 21 (S1 completion) state halfword; 8 = AF done/holding
NOTIFICATIONS = {0x816: 'S1_AF_DONE', 0x829: 'S1_COMPLETE', 0x828: 'S1_AF_FOCALIZE', 0x7: 'S1_ON', 0x87: 'S1_OFF', 0x8: 'S2_ON',
                 0x88: 'S2_OFF', 0x823: 'WRITING', 0x824: 'WRITING_COMPLETE', 0x82b: 'EXPOSURE_DONE'}
# Entry words counted by the fetch guard (a function's first instruction fetches = calls).
KEY_FUNCTIONS = dict(s1_eligibility=0x1244804, pdaf_request=0x124aa04, paf_post=0x1350444,
                     pdaf_decision=0x124acf4, pdaf_result_callback=0x1249a3c, prefilter_request=0x1347f6c,
                     extract_job=0x12129dc, contrast_loop=0x12fc4fc, contrast_iris=0x12fc3a4,
                     contrast_statistics=0x12fcf74, focus_move=0xa08f84, continuous_move=0xa11688)
HELPER_BASE = 0xf50400               # unused heap after the testbed scanner stub (0xf50000..0xf50200)
HEAP_TAIL = 0xf50000                 # the unused heap tail starts here (testbed stub); patches go below it
ASSEMBLER_FLAGS = ('--target=armv7r-none-eabi', '-mcpu=cortex-r4', '-marm', '-c')

HELPER_SOURCE = f"""
    .syntax unified
    .arm
    .text
bench_move:                     @ r0 = delta, r1 = speed index: the stock S1 backlash-move call
    push {{r4, lr}}
    sub sp, sp, #16
    mov r2, #1
    str r2, [sp]
    mov r2, #0xc
    str r2, [sp, #4]
    mov r2, #4
    str r2, [sp, #8]
    mov r2, r1
    mov r1, r0
    adr r0, done_callback
    mov r3, #2
    ldr r12, move_function
    blx r12
    add sp, sp, #16
    pop {{r4, pc}}
done_callback:                  @ lens completion callback: count completions
    ldr r1, counter_address
    ldr r2, [r1]
    add r2, r2, #1
    str r2, [r1]
    bx lr
move_function: .word 0xa08f84
counter_address: .word {HELPER_BASE + 0x80:#x}
"""


class BenchError(RuntimeError):
    pass


def assemble(source, defines=None, flags=ASSEMBLER_FLAGS):
    """Assemble ARM source text/path with clang; returns (.text bytes, {symbol: offset})."""
    from elftools.elf.elffile import ELFFile
    compiler = shutil.which('clang')
    if not compiler:
        raise BenchError('clang is required to assemble patches')
    with tempfile.TemporaryDirectory(prefix='x70-afbench-') as temporary:
        path = Path(temporary) / 'patch.S'
        if isinstance(source, Path) or (isinstance(source, str) and '\n' not in source and Path(source).exists()):
            path.write_bytes(Path(source).read_bytes())
        else:
            path.write_text(source)
        obj = Path(temporary) / 'patch.o'
        command = [compiler, *flags, *(f'-D{k}={v}' for k, v in (defines or {}).items()), str(path), '-o', str(obj)]
        subprocess.run(command, check=True, capture_output=True, text=True)
        with obj.open('rb') as stream:
            elf = ELFFile(stream)
            if any(s['sh_type'] in ('SHT_REL', 'SHT_RELA') and s.num_relocations() for s in elf.iter_sections()):
                raise BenchError('Unresolved relocation in patch')
            code = elf.get_section_by_name('.text').data()
            symbols = {s.name: s['st_value'] for s in elf.get_section_by_name('.symtab').iter_symbols()
                       if s.name and s['st_shndx'] != 'SHN_UNDEF'}
    return code, symbols


class Coverage:
    """Entry-word fetch counters through the native fetch guard (observation only)."""

    def __init__(self, board, functions=None, cores=(0, 1)):
        self.board, self.cores = board, cores
        self.functions = dict(functions or KEY_FUNCTIONS)
        entries = sorted((address, name) for name, address in self.functions.items())
        self.ranges, self.owners, cursor = [], [], 0
        for address, name in entries:
            if address > cursor:
                self.ranges.append((cursor, address))
                self.owners.append(None)
            self.ranges.append((address, address + 4))
            self.owners.append(name)
            cursor = address + 4
        self.ranges.append((cursor, 1 << 32))
        self.owners.append(None)
        if len(self.ranges) > 256:
            raise ValueError('Coverage supports at most 127 functions')
        for core in cores:
            board._native.fetch_guard_begin(core, self.ranges)

    def counts(self):
        total = dict.fromkeys(self.functions, 0)
        for core in self.cores:
            state = self.board._native.fetch_guard_state(core)
            for owner, hits in zip(self.owners, state['range_fetches']):
                if owner:
                    total[owner] += hits
        return total


@dataclass
class InstalledPatch:
    bench: object
    base: int
    code: bytes
    entries: dict
    handle: object
    state_base: int = 0
    state_offsets: dict = field(default_factory=dict)

    def address(self, name):
        return self.entries[name]

    def _state(self, key):
        offset = self.state_offsets.get(key, key)
        if not isinstance(offset, int):
            raise KeyError(f'Unknown patch state word {key!r}')
        return self.base + self.state_base + offset

    def read(self, key, size=4, signed=False):
        return int.from_bytes(self.bench.board.read_memory(self._state(key), size), 'little', signed=signed)

    def write(self, key, value, size=4):
        self.bench.board.write_memory(self._state(key), (value & ((1 << 8 * size) - 1)).to_bytes(size, 'little'))

    def remove(self, force=False):
        self.handle.remove(force)


def failure_free(board):
    return board.failure is None


class AFResult:
    """One half-press: how stock AF went and what it did (all times virtual)."""

    def __init__(self, **fields):
        self.__dict__.update(fields)

    def __repr__(self):
        return ('AFResult(path={path!r}, ok={ok}, time_to_lock_s={time_to_lock_s}, final_position={final_position}, '
                'lens_moves={moves}, pdaf_results={pdaf_results}, reason={reason!r})').format(
                    moves=len(self.lens_moves), **self.__dict__)


class AFBench:
    def __init__(self, testbed, stream, pdaf, lens, trace, scene, coverage, instructions_per_tick, timing=None):
        self.timing = timing
        self.tb, self.board, self.stream, self.pdaf, self.lens = testbed, testbed.board, stream, pdaf, lens
        self._trace, self.scene, self.coverage = trace, scene, coverage
        self.instructions_per_tick = instructions_per_tick
        self.patches, self.ui_events = {}, []
        self._helper = None
        self._press = None

    # ---------------------------------------------------------------- construction
    @classmethod
    def create(cls, snapshot='.tools/sweep/rec', scene=None, fps='idle', instructions_per_tick=None, lens_start=None,
               patches=(), coverage=True, time_scale=1.0, lens_options=None, pdaf_options=None, warmup_ticks=1_000_000,
               stream_options=None, timing=None, backend='reduced', *, fresh=False, prepared_state=None,
               cache_dir=None, seed=1, trace_on_demand=False, vectorized=True, strict_validation=False, settings=None):
        """Restore a verified prepared parent by default; prepare/cache it on first use.

        ``fresh=True`` repeats the original snapshot/warmup/approach path.
        ``prepared_state=Path`` selects a verified portable parent explicitly.
        Cached starts include the acknowledged pre-S1 source frame (``seed``).
        No-start callers cache load/warmup, then retain their own lens setup.
        Reduced selects the qualified graphics/trace adapters; full retains UI pixels
        after preparation/restore. Timing and model identities are mandatory.
        """
        if settings not in (None,'default','owner'):
            raise ValueError('Settings must be default or owner')
        if settings=='owner':
            from emulator.owner_settings import OWNER_SNAPSHOT
            if str(snapshot)=='.tools/sweep/rec':
                snapshot=OWNER_SNAPSHOT
            import json
            selected=json.loads((Path(snapshot)/'manifest.json').read_bytes())['factory_options'].get('settings')
            if selected!='owner':
                raise ValueError('Owner settings require an owner-settings snapshot')
        if backend not in ('full', 'reduced'):
            raise ValueError('AF backend must be full or reduced; isolated execution is unqualified')
        from .af_prepared import create
        explicit = {k for k,v in dict(scene=scene,lens_start=lens_start,timing=timing,
            instructions_per_tick=instructions_per_tick,lens_options=lens_options,pdaf_options=pdaf_options,
            stream_options=stream_options).items() if v is not None}
        for k,v,default in (('snapshot',snapshot,'.tools/sweep/rec'),('fps',fps,'idle'),
                            ('time_scale',time_scale,1.0),('warmup_ticks',warmup_ticks,1_000_000),('patches',patches,())):
            if v!=default: explicit.add(k)
        profile = timing_profiles.resolve(timing)
        options = dict(snapshot=snapshot,scene=scene if scene is not None else SyntheticSceneSource(pulses_per_sample=68),
            fps=fps,instructions_per_tick=profile.instructions_per_tick if instructions_per_tick is None else instructions_per_tick,
            lens_start=lens_start,patches=patches,coverage=coverage,time_scale=time_scale,lens_options=lens_options,
            pdaf_options=pdaf_options,warmup_ticks=warmup_ticks,stream_options=stream_options,timing=profile)
        return create(cls,options,fresh=fresh,prepared_state=prepared_state,cache_dir=cache_dir,seed=seed,
                      backend=backend,trace_on_demand=trace_on_demand,vectorized=vectorized,strict_validation=strict_validation,explicit=explicit)

    @classmethod
    def _create_fresh(cls, snapshot, scene, fps, instructions_per_tick, lens_start, patches, coverage,
                      time_scale, lens_options, pdaf_options, warmup_ticks, stream_options, timing,
                      _portable=False):
        from emulator import pdaf_stream
        profile = timing
        if instructions_per_tick is None:
            instructions_per_tick = profile.instructions_per_tick
        lens_options = dict(profile.lens_options(), **(lens_options or {}))
        pdaf_options = dict(profile.pdaf_options(), **(pdaf_options or {}))
        scene = scene if scene is not None else SyntheticSceneSource(pulses_per_sample=68)
        if _portable:
            from .af_prepared import single_factory_load
            board = single_factory_load(snapshot, instructions_per_tick=instructions_per_tick)
        else:
            board = sensor_stream.load_snapshot(snapshot, instructions_per_tick=instructions_per_tick)
        profile.apply_devices(board)
        stream = sensor_stream.enable(board, scene, fps=fps, lens_motion=False, time_scale=time_scale,
                                      **(stream_options or {}))
        lens = next(d for d in board.devices if type(d).__name__ == 'LensEndpoint')
        lens.enable_generic_focus_moves(**lens_options)
        pdaf = pdaf_stream.enable(board, scene, sensor_stream=stream, **pdaf_options)
        testbed = Testbed(board)
        trace = LensTrace.attach(board, watch_position=not _portable)
        bench = cls(testbed, stream, pdaf, lens, trace, scene,
                    Coverage(board) if coverage else None, instructions_per_tick, timing=profile)
        if not _portable:
            testbed.watch(UI_PACKET, 4, 'afbench.ui')
            testbed.bus.on('mem.write', bench._on_write)
        bench._install_helper()
        for spec in patches:
            bench.install_patch(**spec)
        if warmup_ticks:
            bench.run_ticks(warmup_ticks)
        if lens_start is not None:
            bench.set_lens(lens_start)
        return bench

    def _on_write(self, record):
        if record.get('label') == 'afbench.ui' and record['new']:
            self.ui_events.append(dict(value=record['new'], name=NOTIFICATIONS.get(record['new']),
                                       ticks=record['ticks'], instructions=record['instructions']))

    def _install_helper(self):
        import hashlib
        code, symbols = assemble(HELPER_SOURCE)
        self._helper = install_ram_patch(self.board, code, HELPER_BASE, reserve=0x100)
        self._helper_receipt = dict(base=HELPER_BASE, reserve=0x100, code_bytes=len(code),
                                    code_sha256=hashlib.sha256(code).hexdigest(), word_swaps=[],
                                    purpose='existing bench preparation: stock lens-driver calls')
        self._move_entry = HELPER_BASE + symbols['bench_move']
        self._counter = HELPER_BASE + 0x80

    # ---------------------------------------------------------------- time and running
    @property
    def ticks(self):
        return self.board.virtual_ticks

    @property
    def seconds(self):
        return self.board.virtual_ticks / TICKS_PER_SECOND

    def _instructions(self, ticks):
        return int(ticks * float(self.instructions_per_tick)) + 2 * self.board.quantum

    def run_ticks(self, ticks):
        """Run about `ticks` virtual ticks."""
        deadline = self.board.virtual_ticks + ticks
        result = self.tb.run_until(lambda tb: tb.board.virtual_ticks >= deadline, self._instructions(ticks) * 2,
                                   step=max(20_000, self._instructions(ticks) // 50))
        self._check()
        return result

    def run_seconds(self, seconds):
        return self.run_ticks(round(seconds * TICKS_PER_SECOND))

    def wait(self, predicate, timeout_virtual_s=2.0, step=None):
        """Run until predicate(testbed) or the virtual-time timeout; returns the RunResult."""
        deadline = self.board.virtual_ticks + round(timeout_virtual_s * TICKS_PER_SECOND)
        budget = self._instructions(deadline - self.board.virtual_ticks) * 2
        result = self.tb.run_until(lambda tb: predicate(tb) or tb.board.virtual_ticks >= deadline, budget,
                                   step=step or 50_000)
        return result

    def _check(self):
        if self.board.failure is not None:
            raise BenchError('Board stopped: ' + str(self.board.failure))

    # ---------------------------------------------------------------- lens
    def position(self):
        return int.from_bytes(self.board.read_memory(POSITION, 2), 'little', signed=True)

    def _word(self, address, size=4):
        return int.from_bytes(self.board.read_memory(address, size), 'little')

    def move(self, delta, speed=2, timeout_virtual_s=3.0):
        """One stock relative move (FUN_00a08f84 with the S1 backlash-move arguments) and its full cleanup."""
        done = self._word(self._counter)
        self.tb.host_call(self._move_entry, delta, speed,
                          timeout_instructions=self._instructions(round(timeout_virtual_s * TICKS_PER_SECOND)))
        result = self.wait(lambda tb: self._word(self._counter) > done and self._word(MOTION) == 0
                           and self._word(LENS_STATE) == 0, timeout_virtual_s)
        self._check()
        if self._word(self._counter) <= done:
            raise BenchError(f'Lens move {delta:+d} did not complete ({result.reason})')
        return self.position()

    def set_lens(self, position, timeout_virtual_s=3.0):
        """Move the lens to `position`, finishing toward near after a backlash pre-move (stock S1 style)."""
        current = self.position()
        if position < current:
            current = self.move(position - OVERSHOOT - current, timeout_virtual_s=timeout_virtual_s)
        if position != current:
            current = self.move(position - current, timeout_virtual_s=timeout_virtual_s)
        return current

    # ---------------------------------------------------------------- AF
    def half_press(self, timeout_virtual_s=3.0, settle_virtual_s=0.5):
        """Press S1 (debug key path); run until AF done (UI 0x816 or 0x829, or REC21=8), failure or timeout."""
        self._check()
        start = dict(ticks=self.ticks, instructions=self.board.instructions, ui=len(self.ui_events),
                     coverage=self.coverage.counts() if self.coverage else {},
                     pdaf=self._word(S1_RESULT_COUNT, 1), moves=len(self._trace.records),
                     position=self.position())
        self._press, self._rec21_tick = start, None
        self.tb.debug_key('S1', 'ON')
        seen = {}

        def done(tb):
            if any(e['value'] in AF_DONE for e in self.ui_events[start['ui']:]):
                return True
            if self._word(REC21, 2) == 8:     # holding state; its UI notification follows shortly
                seen.setdefault('rec21', self.ticks)
                self._rec21_tick = seen['rec21']
                return self.ticks - seen['rec21'] >= REC21_GRACE_TICKS
            return False
        result = self.wait(done, timeout_virtual_s)
        if self.board.failure is None:     # AF done can precede the final drive: let the lens settle before reading it
            self.wait(lambda tb: self._word(MOTION) == 0 and self._word(LENS_STATE) == 0, settle_virtual_s)
        return self._result(start, result)

    def _result(self, start, run):
        events_ = self.ui_events[start['ui']:]
        complete = next((e for e in events_ if e['value'] in AF_DONE), None)
        if complete is None and self._word(REC21, 2) == 8 and failure_free(self.board):
            complete = dict(value=None, name='REC21=8', ticks=getattr(self, '_rec21_tick', None) or self.ticks)
        counts = self.coverage.counts() if self.coverage else {}
        delta = {k: counts[k] - start['coverage'].get(k, 0) for k in counts}
        pdaf = delta.get('paf_post', 0) > 0 or delta.get('pdaf_request', 0) > 0
        contrast = any(delta.get(k, 0) > 0 for k in ('contrast_loop', 'contrast_iris', 'contrast_statistics'))
        path = 'pdaf+contrast' if pdaf and contrast else 'pdaf' if pdaf else 'contrast' if contrast else 'none'
        failure = self.board.failure
        moves = [m for m in self._trace.moves() if m['ticks'] >= start['ticks']]
        reason = ('failure' if failure else 'complete' if complete else 'timeout')
        return AFResult(
            provenance=self.provenance(),
            path=path, ok=complete is not None and failure is None, reason=reason, failure=failure,
            done_event=None if complete is None else complete['name'],
            press_tick=start['ticks'],
            s1_complete_ticks=None if complete is None else complete['ticks'] - start['ticks'],
            time_to_lock_s=None if complete is None else (complete['ticks'] - start['ticks']) / TICKS_PER_SECOND,
            start_position=start['position'], final_position=self.position(),
            lens_moves=moves, iris_moves=[m for m in self._trace.iris_moves() if m['ticks'] >= start['ticks']],
            pdaf_results=(self._word(S1_RESULT_COUNT, 1) - start['pdaf']) & 0xff,
            contrast_samples=delta.get('contrast_statistics', 0), coverage=delta,
            events=[dict(e) for e in events_],
            notifications={n: sum(1 for e in events_ if e['name'] == n) for n in
                           sorted({e['name'] for e in events_ if e['name']})},
            refusals=self._trace.of('refusal')[-8:], elapsed_s=(self.ticks - start['ticks']) / TICKS_PER_SECOND)

    def release(self, timeout_virtual_s=1.0):
        """Release S1 and let the stock code settle (until the S1_OFF notification or timeout)."""
        mark = len(self.ui_events)
        self.tb.debug_key('S1', 'OFF')
        result = self.wait(lambda tb: any(e['value'] == 0x87 for e in self.ui_events[mark:]), timeout_virtual_s)
        self._check()
        return result

    def screenshot(self, output='lcd'):
        return self.tb.screenshot(output)

    def trace(self):
        return self._trace

    def summary(self):
        return dict(provenance=self.provenance(), seconds=self.seconds, timing=self.timing.name if self.timing else None,
                    position=self.position(), stream=self.stream.summary()['input'],
                    pdaf=self.pdaf.summary(), coverage=self.coverage.counts() if self.coverage else None,
                    ui_events=[e['name'] or hex(e['value']) for e in self.ui_events[-16:]])

    def provenance(self):
        from .af_mod import provenance
        result = provenance(self)
        result['preparation'] = getattr(self, 'preparation', None)
        result['host_arithmetic'] = 'vectorized-exact' if getattr(self, '_vectorized', True) else 'reference'
        result['backend'] = getattr(self, 'backend_receipt', dict(backend='full'))
        return result

    def install_mod(self, build, config=None):
        """Install an af runtime build/manifest and validate config in its C setup."""
        from .af_config import freeze_mod, install
        from .af_prepared import CACHE
        path = Path(build)
        path = path/'manifest.json' if path.is_dir() else path
        frozen = freeze_mod(path, CACHE)
        if getattr(self, '_fresh', False):
            from .af_mod import install_mod
            return install_mod(self, frozen, config)
        return install(self, frozen, config)

    def align_press(self, prepared_tick=None, seed=1):
        """Align S1 once; a prepared parent already has this acknowledged edge."""
        from .af_prepared import align_press
        return align_press(self, prepared_tick, seed)

    # ---------------------------------------------------------------- RAM patches
    def find_free_block(self, size, below=HEAP_TAIL, floor=0xe00000):
        """Highest 8-byte-aligned all-zero block of `size` bytes below the testbed's heap-tail stub."""
        zero = bytes(size)
        address = (below - size) & ~0xfff
        while address >= floor:
            if bytes(self.board.read_memory(address, size)) == zero:
                return address
            address -= 0x1000
        raise BenchError(f'No unused {size:#x}-byte block found below {below:#x}')

    def install_patch(self, source, entry_symbols=None, code_word_swaps=(), *, base=None, defines=None, init=(),
                      reserve=None, name='patch', state_base=0, state_offsets=None, flags=ASSEMBLER_FLAGS):
        """Assemble (or take) a patch, write it to a zero block and swap code words.

        `source` is ARM assembly (text or path) or raw code bytes. `defines` is a dict or `fn(base) -> dict`
        of `-D` macros. `entry_symbols` selects the symbols exposed in `patch.entries` (default: all).
        `code_word_swaps` are `(address, expected_old, new_or_fn(base))`. `init` is `[(offset, bytes)]` written
        after the code (patch state). `reserve` is the block size to claim (zero-checked).
        """
        size = (reserve or 0) if reserve else (len(source) if isinstance(source, (bytes, bytearray)) else 0x4000)
        base = self.find_free_block(max(size, 0x100)) if base is None else base
        if isinstance(source, (bytes, bytearray)):
            code, symbols = bytes(source), {}
        else:
            macros = defines(base) if callable(defines) else defines
            code, symbols = assemble(source, macros, flags)
        names = list(entry_symbols) if entry_symbols is not None else list(symbols)
        missing = [n for n in names if n not in symbols]
        if missing:
            raise BenchError(f'Patch has no symbol(s) {missing}')
        swaps = [(a, old, new(base) if callable(new) else new) for a, old, new in code_word_swaps]
        handle = install_ram_patch(self.board, code, base, swaps, reserve=reserve)
        for offset, data in init:
            if offset + len(data) > handle.reserve:
                raise BenchError('Patch init lies outside the claimed block')
            self.board.write_memory(base + offset, bytes(data))
        patch = InstalledPatch(self, base, code, {n: base + symbols[n] for n in names}, handle, state_base,
                               dict(state_offsets or {}))
        self.patches[name] = patch
        return patch

    def control(self, op, patch=None, entry='patch_control', timeout_virtual_s=1.0, **state_words):
        """Call a patch's control entry (r0 = op) from the scanner context; returns its r0.

        `state_words` are written first into the patch state (names via `state_offsets`, else int offsets).
        """
        patch = patch or next(iter(self.patches.values()))
        for key, value in state_words.items():
            patch.write(int(key, 0) if isinstance(key, str) and key[:1].isdigit() else key, value)
        result = self.tb.host_call(patch.address(entry), op, timeout_instructions=self._instructions(
            round(timeout_virtual_s * TICKS_PER_SECOND)))
        self._check()
        return result


class FocusSweepPatch:
    """The camera focus-sweep patch (`research/focus-sweep-patch/patch.S`) installed on the bench.

    Layout and operations follow `tools/x70_focus_sweep.py`: one owned block, control entry at +0x400,
    state at +0x1200. Operations run through the stock control convention (r0 = op) via `bench.control`;
    `install` swaps the PDAF request call site 0x124AA4C inside the guest, exactly as on the camera.
    """
    SOURCE = Path(__file__).resolve().parents[2] / 'research/focus-sweep-patch/patch.S'
    CONTROL, ENTRY_REQUEST, ENTRY_RESULT, ENTRY_MOVE_DONE, WORKER, STATE = 0x400, 0x800, 0x900, 0xa00, 0x1100, 0x1200
    MAGIC = 0x50454557
    SITE, SITE_STOCK = 0x124aa4c, 0xeb04167c
    OPERATIONS = dict(install=1, rollback=2, move=3, clear=4)
    S = dict(enabled=8, installed=12, status=16, last_op=20, calls=24, token=28, move_delta=0x20, move_result=0x24,
             move_seq=0x28, move_done=0x2c, move_done_pos=0x30, req_count=0x34, req_buffer=0x38, req_orig=0x3c,
             res_count=0x40, move_start_pos=0x44, move_refusal=0x48, req_pos=0x4c, mailbox=0x80, ring=0x200)
    RECORD, RING, RESERVE = 0x200, 8, 0x2400
    REFUSALS = {1: 'not installed', 2: 'motor running', 3: 'move outstanding', 4: 'bad delta',
                5: 'target out of range', 6: 'stock move request rejected'}

    def __init__(self, bench, patch, nonce):
        self.bench, self.patch, self.nonce, self.token = bench, patch, nonce, 0

    @staticmethod
    def bl_word(site, target):
        delta = target - (site + 8)
        if delta % 4 or not -(1 << 25) <= delta < 1 << 25:
            raise ValueError('bl out of range')
        return 0xeb000000 | ((delta >> 2) & 0xffffff)

    @classmethod
    def install(cls, bench, nonce=0x1234abcd, base=None):
        def defines(address):
            return dict(STATE_ADDR=(address + cls.STATE) | 0x40000000, LEASE_NONCE=nonce, CODE_BASE=address,
                        PAF_BL=cls.bl_word(cls.SITE, address + cls.ENTRY_REQUEST))
        patch = bench.install_patch(cls.SOURCE, ('patch_control', 'sweep_paf_request', 'sweep_paf_result',
                                                 'sweep_move_done'), base=base, defines=defines,
                                    init=[(cls.STATE, struct.pack('<2I', cls.MAGIC, nonce))], reserve=cls.RESERVE,
                                    name='focus-sweep', state_base=cls.STATE, state_offsets=cls.S)
        for name, offset in (('patch_control', cls.CONTROL), ('sweep_paf_request', cls.ENTRY_REQUEST),
                             ('sweep_paf_result', cls.ENTRY_RESULT), ('sweep_move_done', cls.ENTRY_MOVE_DONE)):
            if patch.entries[name] - patch.base != offset:
                raise BenchError('Unexpected focus-sweep payload layout: ' + name)
        return cls(bench, patch, nonce)

    def state(self, name, signed=False):
        return self.patch.read(name, signed=signed)

    @property
    def site_word(self):
        return word(self.bench.board, self.SITE)

    def op(self, name, delta=None):
        """Run one control operation; returns the decoded mailbox (status 0 = success)."""
        if delta is not None:
            self.patch.write('move_delta', delta)
        self.token = (self.token + 1) & 0xffffffff or 1
        self.patch.write('token', self.token)
        self.bench.board.write_memory(self.patch.base + self.STATE + self.S['mailbox'], b'\xa5' * 32)
        returned = self.bench.control(self.OPERATIONS[name], self.patch)
        values = struct.unpack('<8I', self.bench.board.read_memory(self.patch.base + self.STATE + self.S['mailbox'], 32))
        entry = dict(operation=name, returned=returned, nonce=values[0], op=values[1],
                     status=struct.unpack('<i', struct.pack('<I', values[2]))[0], detail=values[3],
                     site=values[4], enabled=values[5], core_id=values[6], token=values[7])
        entry['token_ok'] = entry['token'] == self.token and entry['nonce'] == self.nonce
        if entry['status'] == -1 and name == 'move':
            entry['refusal'] = self.REFUSALS.get(self.state('move_refusal'), self.state('move_refusal'))
        return entry

    def move(self, delta, timeout_virtual_s=3.0):
        """Patch move op, then wait for its stock completion callback and a stopped motor."""
        seq_before = self.state('move_seq')
        entry = self.op('move', delta)
        if entry['status'] != 0:
            raise BenchError(f"Move refused: {entry.get('refusal')} ({entry})")
        self.bench.wait(lambda tb: self.state('move_done') == self.state('move_seq')
                        and self._word(MOTION) == 0 and self._word(LENS_STATE) == 0, timeout_virtual_s)
        self.bench._check()
        if self.state('move_done') != self.state('move_seq') or self.state('move_seq') != seq_before + 1:
            raise BenchError('Move completion callback not seen')
        return self.bench.position()

    def _word(self, address):
        return self.bench._word(address)

    def records(self):
        """Decoded ring records published by the hooked PDAF result callback (oldest first)."""
        count = self.state('res_count')
        out = []
        for seq in range(max(0, count - self.RING), count):
            raw = bytes(self.bench.board.read_memory(
                self.patch.base + self.STATE + self.S['ring'] + (seq % self.RING) * self.RECORD, self.RECORD))
            sequence, position, motion, s1_count, buffer, areas, copied, request_position = struct.unpack_from(
                '<iiIIIIIi', raw)
            out.append(dict(seq=sequence, position=position, motion=motion, s1_result_count=s1_count,
                            pd_buffer=hex(buffer), areas=areas, copied=copied, request_position=request_position,
                            header_hex=raw[0x20:0x28].hex(),
                            area_records_hex=[raw[0x28 + i * 0x74:0x28 + (i + 1) * 0x74].hex()
                                              for i in range(min(copied, 4))]))
        return out


def observe_pd_decisions(bench, costs=False):
    """Read-only stock hybrid/PD diagnostics, shared by every AF feature.

    Stock decision 0124acf4, return 0124b6f0, fb1c override 0124b5c4;
    caller continuation 012330f4 follows the mod hook. S1 fields are from
    s5_4_profile_policy.h: state 162fd48, areas 162fd4e, class 162db68.
    Optional costs observe stock correlation/combine without changing them.
    """
    import struct
    from x70_native import registers as regs
    board = bench.board
    if board.breakpoint_handler is not None:
        raise ValueError('An AF native observer is already attached')
    bench.pd_decisions, bench.pd_trace = [], []
    sites = {0x124acf4:'entry',0x124b6f0:'stock',
             0x124b5c4:'fb1c',0x12330f4:'mod'}
    stacks = {}
    if costs:
        sites.update({0x134a20c:'correlation-entry',0x134a8ac:'correlation-exit',
                      0x134ac34:'combine-entry',0x134b04c:'combine-exit'})
    def observer(cpu, core, address):
        kind = sites[address]
        if '-' not in kind:
            state,lens,areas,selected = struct.unpack('<ihBB',board.read_memory(0x162fd48,8))
            bench.pd_decisions.append(dict(phase=kind,ticks=board.virtual_ticks,
                state=state,lens_value=lens,processed_area_count=areas,selected_area=selected,
                area_class=struct.unpack('<I',board.read_memory(0x162db68,4))[0],
                sample_count=board.read_memory(0x162dbac,1)[0],
                outcome=struct.unpack('<I',board.read_memory(0x162dff4,4))[0],
                target=struct.unpack('<h',board.read_memory(0x162dba4,2))[0]))
            return
        correlation = kind.startswith('correlation')
        stack = stacks.setdefault((core,kind.split('-')[0]),[])
        if kind.endswith('entry'):
            pointer = cpu.reg_read(regs.R0)
            words = struct.unpack('<4I' if correlation else '<6I',board.read_memory(pointer,16 if correlation else 24))
            event = dict(kind=kind,core=core,ticks=board.virtual_ticks,request=words)
            if correlation:
                event.update(area_hex=board.read_memory(words[0],0x1d4).hex(),
                    completion_hex=board.read_memory(words[1],40).hex())
            bench.pd_trace.append(event);stack.append(event)
        else:
            event = stack.pop();words = event['request']
            event['after_hex'] = board.read_memory(words[3] if correlation else words[5],0xb90 if correlation else 0x74).hex()
    board.breakpoint_handler = observer
    for core in range(len(board.cpus)):
        board._native.set_breakpoints(core,sorted(sites))
    return sites
