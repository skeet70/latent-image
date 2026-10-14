"""Opt-in AF device boundaries; the full firmware still runs on RustBoard.

The first boundary retains graphics job validation, DMA size, virtual latency,
status, ACK and IRQ handling, but omits UI pixels. This is deliberately called
``reduced``: task isolation is a separate, stronger contract.
"""
from __future__ import annotations

import hashlib
import json
from types import FunctionType, MethodType


OUTCOME_FIELDS = ('reason', 'final', 'rounds', 'settled', 'trajectory', 'af_outcome',
                  'time_s', 'guest_instructions', 'virtual_ticks', 'raw')


def outcome(bench, result, *, start_instructions, start_ticks):
    """Observe an AFBench half-press for its backend equivalence tests.

    Preparation, mod setup and execution belong to AFBench. This helper only
    reads the finished result/model state; it does not create another runner.
    """
    return dict(reason=result.reason, final=result.final_position, rounds=result.pdaf_results,
                settled=bench._word(0xccde90) == 0 and bench._word(0xccde04) == 0,
                trajectory=[(m['phase'], m['quantity'], m['direction']) for m in result.lens_moves],
                af_outcome=bench._word(0x162dff4), time_s=result.time_to_lock_s,
                guest_instructions=bench.board.instructions - start_instructions,
                virtual_ticks=bench.ticks - start_ticks, raw=bench.pdaf.raw.summary())


def compare_outcomes(reference, candidate, *, reference_input, candidate_input):
    """Strict reduced/full comparison with caller-supplied input provenance.

    Inputs include the prepared-state, firmware and mod identities and setup
    policy. Missing outcome fields fail instead of weakening the comparison.
    Guest instructions are an additional invariant of the current reduced
    boundary; a future isolated backend needs its own qualified timing gate.
    """
    canonical = lambda value: json.loads(json.dumps(value))
    if not reference_input or canonical(reference_input) != canonical(candidate_input):
        raise ValueError('AF backend equivalence requires identical recorded inputs')
    differences = [key for key in OUTCOME_FIELDS if canonical(reference[key]) != canonical(candidate[key])]
    return dict(equivalent=not differences, different_fields=differences,
                raw_equal='raw' not in differences)


def configure(bench, backend):
    """Small AFBench factory hook, applied after preparation/restoration.

    AFBench owns caching, scene construction, mod builds and half_press. The
    reduced option combines the qualified pixel boundary and host trace-copy
    optimization. Full remains the pixel-preserving stock-task path.
    """
    if backend == 'full':
        return getattr(bench, 'backend_receipt', dict(backend='full'))
    if backend != 'reduced':
        raise ValueError('Isolated AF is not yet qualified; select full or reduced')
    install(bench)
    if not getattr(bench.board, '_af_trace_on_demand', False): trace_on_demand(bench)
    return bench.backend_receipt


def trace_on_demand(bench):
    """Avoid materializing the 32-instruction diagnostic trace every quantum.

    Machine retains the trace natively. Snapshot calls and all exception/error
    paths pull it; only the successful per-quantum Python mirror is deferred.
    Guest counters, scheduler inputs and peripheral callbacks are unchanged.
    """
    from unicorn import UcError
    board = bench.board
    if board.executing_cpu is not None or board.failure:
        raise ValueError('Trace optimization needs a successful paused RustBoard')
    if getattr(board, '_af_trace_on_demand', False):
        raise ValueError('Trace optimization is already installed')
    for cpu in board.cpus:
        names = cpu.emu_start.__func__.__code__.co_names
        if not {'_pull_native', '_push_native', '_machine', '_board_ref'}.issubset(names):
            raise ValueError('NativeCore emu_start contract changed')

    def start(cpu, begin, until, timeout=0, count=0):
        owner = cpu._board_ref()
        if owner is None:
            raise RuntimeError('Board no longer exists')
        owner._push_native()
        previous = owner.executing_cpu
        owner.executing_cpu = cpu.core
        try:
            status = cpu._machine.emu_start(cpu.core, begin, until, timeout, count)
        except Exception as error:
            owner._pull_native()
            if owner.failure is None:
                from x70_native.registers import PC
                owner.failure = dict(message='Native execution callback exception', cpu=cpu.core,
                                     pc=hex(cpu.reg_read(PC)), error_type=type(error).__name__, error=str(error))
            raise
        finally:
            owner.executing_cpu = previous
            owner._pull_native(trace=False)
        if status:
            owner._pull_native()
            raise UcError(status)

    original = board.snapshot
    def snapshot(owner):
        owner._pull_native()
        return original()

    for cpu in board.cpus: cpu.emu_start = MethodType(start, cpu)
    board.snapshot = MethodType(snapshot, board)
    board._af_trace_on_demand = True
    receipt = getattr(bench, 'backend_receipt', dict(backend=getattr(bench, 'backend', 'full')))
    receipt.setdefault('host_optimizations', []).append('native diagnostic trace pulled at snapshot/error boundaries')
    bench.backend_receipt = receipt
    return receipt


def retained_ui(draws, *, read_bytes, cpu_id):
    """Stand-in for the graphics pixel model, with the same destination sizes.

    Job parsing has already checked dimensions, sources, command banks and IRQ
    ownership. Preserve destination bytes and all DMA/completion timing. AF
    statistics, preview images, RAW, sensor noise and lens laws are untouched.
    """
    from emulator.hardware import UnsupportedDevice
    targets = {}
    for draw in draws:
        address, width, height, _ = draw['target']
        if address not in targets:
            data = bytes(read_bytes(address, width * height * 4, cpu_id=cpu_id))
            if len(data) != width * height * 4:
                raise UnsupportedDevice('AF graphics boundary: incomplete destination')
            targets[address] = data
    return list(targets.items()), 0, {hex(a): hashlib.sha256(d).hexdigest() for a, d in targets.items()}


def install(bench):
    """Attach after verified snapshot restore; return an explicit receipt.

    Binding a private copy of the stock host method avoids mutating a global
    module or affecting another board. The shared graphics protocol and clock
    continue to own completion. No firmware words or RTOS state are patched.
    """
    if getattr(bench, 'backend', 'full') != 'full':
        raise ValueError('An AF backend is already selected')
    board = bench.board
    if board.executing_cpu is not None or board.failure:
        raise ValueError('AF backend installation requires a successful paused board')
    devices = [d for d in board.devices if type(d).__name__ == 'GraphicsAccelerator']
    if len(devices) != 1:
        raise ValueError('AF graphics boundary needs one GraphicsAccelerator')
    device = devices[0]
    method = device._start_ui_draws.__func__
    # Refuse a changed contract instead of silently skipping an unknown stage.
    if '_ui_render' not in method.__code__.co_names:
        raise ValueError('Graphics UI method no longer has the declared pixel boundary')
    namespace = dict(method.__globals__, _ui_render=retained_ui)
    private = FunctionType(method.__code__, namespace, method.__name__, method.__defaults__)
    private.__kwdefaults__ = method.__kwdefaults__
    device._start_ui_draws = MethodType(private, device)
    receipt = dict(backend='reduced', boundaries=['graphics.ui_pixels'],
                   guest_tasks='all stock tasks on both cores',
                   virtual_clock='unchanged stock scheduler and device timing',
                   omitted='OSD pixel rasterization; screenshots are stale',
                   firmware_word_writes=[], skipped_firmware_ranges=[])
    bench.backend, bench.backend_receipt = 'reduced', receipt
    return receipt
