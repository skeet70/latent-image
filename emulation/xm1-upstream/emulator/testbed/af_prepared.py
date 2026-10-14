"""Shared AF preparation/cache policy. Models and guest inputs remain unchanged.

Caches contain a verified data-only native snapshot, never a live board or CPU
context. Host observers/backends are selected only after the parent is saved.
"""
from dataclasses import fields, is_dataclass
from collections.abc import Mapping
from functools import wraps
import gc
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tempfile
import time

CACHE = Path(__file__).resolve().parents[2] / '.tools/afbench/cache'
_VECTOR_RESTORE = None
_VALIDATION_RESTORE = None


def reference_graphics():
    global _VECTOR_RESTORE
    if _VECTOR_RESTORE is not None:
        _VECTOR_RESTORE()
        _VECTOR_RESTORE = None


def vector_graphics():
    global _VECTOR_RESTORE
    from .af_graphics import install
    if _VECTOR_RESTORE is None:
        _VECTOR_RESTORE = install()


def periodic_validation(every=64):
    """Keep the per-tick wire ledger check; hash immutable history periodically."""
    global _VALIDATION_RESTORE
    from emulator import lens_af_motion as lam
    if _VALIDATION_RESTORE is not None: return
    original=lam.validate_generic
    calls=0
    def validate(endpoint):
        nonlocal calls
        calls+=1
        if calls % every == 1: return original(endpoint)
        request=endpoint.requests['focus']
        if type(request) is not dict or request.get('profile')!=lam.GENERIC_PROFILE or request.get('state')!='generic':
            return original(endpoint)
        return lam.generic_validate(endpoint,request)
    lam.validate_generic=validate
    _VALIDATION_RESTORE=lambda:setattr(lam,'validate_generic',original)


def reference_validation():
    global _VALIDATION_RESTORE
    if _VALIDATION_RESTORE is not None:
        _VALIDATION_RESTORE();_VALIDATION_RESTORE=None


def single_factory_load(directory, *, instructions_per_tick=None):
    # The ordinary portable loader still verifies every identity/blob/state;
    # hand it the already verified fresh factory instead of allocating twice.
    from emulator import boot
    from x70_native import portable_checkpoint as pc
    manifest = json.loads((Path(directory) / 'manifest.json').read_bytes())
    factory = boot.make_board
    board = factory(**manifest['factory_options'])
    old, current = manifest['identity'], pc._identity(board)
    revisions = {}
    for key in old:
        if key == 'models':
            for name in set(old[key]) | set(current[key]):
                if old[key].get(name) != current[key].get(name):
                    revisions['models.' + name] = {'saved': old[key].get(name), 'current': current[key].get(name)}
        elif key == 'factory' and old[key] != current[key]:
            if {k:v for k,v in old[key].items() if k!='code_sha256'} != {k:v for k,v in current[key].items() if k!='code_sha256'}:
                raise RuntimeError('Snapshot factory identity differs')
            revisions['factory.code_sha256'] = {'saved':old[key]['code_sha256'], 'current':current[key]['code_sha256']}
        elif key == 'portable_code' and old[key] != current[key]:
            revisions['portable_code'] = {'saved':old[key], 'current':current[key]}
        elif old[key] != current[key]:
            raise RuntimeError('Snapshot identity differs outside models: ' + key)
    used = False

    @wraps(factory)
    def handoff(**options):
        nonlocal used
        boot.make_board = factory
        if used or options != manifest['factory_options']:
            raise RuntimeError('Portable factory handoff differs')
        used = True
        return board

    boot.make_board = handoff
    try:
        options = dict(instructions_per_tick=instructions_per_tick, override_ratio=instructions_per_tick is not None)
        loaded = pc.load_for_development(directory, revisions, **options) if revisions else pc.load(directory, **options)
        if not used or loaded is not board:
            raise RuntimeError('Portable loader did not consume verified factory')
    finally:
        boot.make_board = factory
    loaded.stream_changed_models = sorted(revisions)
    return loaded


def fingerprint(value, seen=None):
    """Data identity including scene arrays, parameters and files behind Paths."""
    import numpy as np
    seen = {} if seen is None else seen
    if value is None or type(value) in (str, int, float, bool):
        return value
    if isinstance(value, Path):
        files = sorted(p for p in value.rglob('*') if p.is_file()) if value.is_dir() else [value]
        return dict(path=str(value.resolve()), files=[(str(p.relative_to(value) if value.is_dir() else p.name),
                    hashlib.sha256(p.read_bytes()).hexdigest()) for p in files])
    if isinstance(value, bytes):
        return dict(bytes=len(value), sha256=hashlib.sha256(value).hexdigest())
    if isinstance(value, np.ndarray):
        if value.dtype.hasobject:
            raise ValueError('Object scene arrays cannot identify a prepared state')
        return dict(dtype=value.dtype.str, shape=value.shape, sha256=hashlib.sha256(value.tobytes()).hexdigest())
    if isinstance(value, np.generic):
        return fingerprint(value.item(), seen)
    if id(value) in seen:
        return dict(reference=seen[id(value)][0])
    # Retain objects, including temporary dataclass/field dictionaries, until
    # this traversal ends. Reused Python IDs must not become false aliases.
    seen[id(value)] = (len(seen),value)
    if isinstance(value, Mapping):
        return {str(k):fingerprint(v, seen) for k,v in sorted(value.items(), key=lambda item:str(item[0]))}
    if isinstance(value, (tuple, list)):
        return [fingerprint(v, seen) for v in value]
    if is_dataclass(value):
        return fingerprint({f.name:getattr(value,f.name) for f in fields(value)}, seen)
    if hasattr(value, '__dict__') and type(value).__module__.startswith('emulator.'):
        cls=type(value).__module__+'.'+type(value).__qualname__
        derived={'_cache','_frame_cache','_pd_cache','_banks','last','last_pd','last_af'}
        if cls=='emulator.testbed.radiance_stacks.SweepProxyStack':
            # load() assigns the requested pose before every texture lookup;
            # the texture field is deterministic from the hashed parameters.
            # Warming this shared stack must not identify another scene.
            derived.update(('_current_position','_field'))
        return dict(cls=cls,fields=fingerprint({k:v for k,v in vars(value).items() if k not in derived},seen))
    raise ValueError('Scene/cache input lacks data identity: '+str(type(value)))


def runtime_identity():
    """Hash actual loaded source trees; frozen model provenance stays explicit."""
    roots = dict(emulator=Path(__file__).resolve().parents[1], native=Path(__file__).resolve().parents[2]/'native/python/x70_native')
    for finder in sys.meta_path:
        name = type(finder).__name__
        if name == 'FrozenFinder': roots['emulator'] = Path(finder.runtime)/'emulator'
        elif name == 'FrozenNativeFinder': roots['native'] = Path(finder.runtime)/'x70_native'
    result = {}
    for name,root in roots.items():
        digest = hashlib.sha256()
        for p in sorted(root.rglob('*')):
            if p.is_file() and (p.suffix in ('.py','.so','.dylib') or '.so.' in p.name):
                digest.update(str(p.relative_to(root)).encode());digest.update(p.read_bytes())
        result[name] = digest.hexdigest()
    # Orchestration overlays are independently named, not hidden as frozen models.
    host = {Path(__file__).parent/name for name in ('afbench.py','af_prepared.py','af_state.py','af_graphics.py','af_config.py')}
    root=Path(__file__).resolve().parents[2]
    for finder in sys.meta_path:
        if type(finder).__name__=='FrozenFinder':
            host.update(root/Path(*name.split('.')).with_suffix('.py') for name in finder.host_modules)
    result['host'] = {str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(host)}
    result['scene_source'] = hashlib.sha256((Path(__file__).parent/'scene_sources.py').read_bytes()).hexdigest()
    result['pd_stream'] = hashlib.sha256((Path(__file__).resolve().parents[1]/'pdaf_stream.py').read_bytes()).hexdigest()
    return result


def align_press(bench, prepared_tick=None, seed=1):
    if seed < 1: raise ValueError('Prepared seed must be positive')
    existing = getattr(bench, 'input_alignment', None)
    if existing is not None and existing['seed'] == seed and bench.ticks == existing['press_tick']:
        return existing
    prepared_tick = bench.ticks if prepared_tick is None else prepared_tick
    stream = bench.stream.input.stream
    period, origin = stream['period_ticks'], stream['enabled_tick']
    frame = (prepared_tick + 3_000_000 - origin)//period + seed
    deadline = origin + frame*period
    bench.wait(lambda tb:stream['frame'] >= frame and stream['pending'] is None,
               max(0, deadline-bench.ticks)/12e6+.1, step=1000)
    bench._check()
    if stream['frame'] != frame or stream['pending'] is not None:
        raise RuntimeError('Common acknowledged sensor frame missed before S1')
    bench.input_alignment = dict(method='acknowledged-source0-frame-v1',frame=frame,
        prepared_tick=prepared_tick,target_tick=deadline,press_tick=bench.ticks,
        source_tick=stream['last_assert_tick'],ack_tick=stream['last_ack_tick'],
        seed=seed,seed_phase='one source0 frame per additional seed',poll_instructions=1000)
    return bench.input_alignment


def observers(bench, coverage):
    from .afbench import Coverage, UI_PACKET
    from . import inputs, memory
    bench.coverage = Coverage(bench.board) if coverage else None
    # The portable parent never had store hooks. Reattach scanner observation
    # only on this disposable trial board; ticket completion also polls RAM.
    if coverage:
        bench._trace._start_position_watch()
        stub=bench.tb.stub
        stub.watches = [memory.watch(bench.board,stub.entry(slot),4,'testbed.queue') for slot in range(inputs.SLOTS)]
        bench.tb.bus.on('mem.write',stub._on_write)
    bench.tb.watch(UI_PACKET,4,'afbench.ui')
    bench.tb.bus.on('mem.write',bench._on_write)


def create(cls, options, *, fresh, prepared_state, cache_dir, seed, backend, trace_on_demand, vectorized, strict_validation=False, explicit=None):
    from . import af_state, inputs
    from .af_fast import configure
    reference_graphics()
    reference_validation()
    started_cpu, started_wall = time.process_time(), time.perf_counter()
    gc.collect()
    coverage = options['coverage']
    if fresh:
        if prepared_state is not None: raise ValueError('fresh=True cannot select a saved prepared state')
        if vectorized: vector_graphics()
        if not strict_validation: periodic_validation()
        bench = cls._create_fresh(**options)
        bench._fresh = True
        bench.preparation = dict(method='fresh',cpu_s=time.process_time()-started_cpu,wall_s=time.perf_counter()-started_wall)
    else:
        if prepared_state is not None:
            directory=Path(prepared_state)
            key=None
        else:
            key = dict(schema='afbench-prepared-v1', snapshot=hashlib.sha256((Path(options['snapshot'])/'manifest.json').read_bytes()).hexdigest(),
                       scene=fingerprint(options['scene']),options=fingerprint({k:v for k,v in options.items() if k not in ('snapshot','scene','coverage')}),
                       seed=seed,runtime=runtime_identity())
            directory = Path(cache_dir or CACHE)/hashlib.sha256(json.dumps(key,sort_keys=True).encode()).hexdigest()[:24]
        if not directory.exists():
            if prepared_state is not None: raise ValueError('Selected prepared state is absent: '+str(directory))
            directory.parent.mkdir(parents=True,exist_ok=True)
            temporary = Path(tempfile.mkdtemp(prefix='.prepare-',dir=directory.parent))
            parent = temporary/'state'
            oldwatch = inputs.watch
            inputs.watch = lambda *a,**k:None
            try:
                if vectorized: vector_graphics()
                if not strict_validation: periodic_validation()
                bench = cls._create_fresh(**dict(options,coverage=False), _portable=True)
                if options['lens_start'] is not None:
                    alignment = align_press(bench,seed=seed)
                else:
                    alignment = None # callers that approach later inherit cached load/warmup only
                reference_graphics()
                reference_validation()
                af_state.save(bench,parent,alignment,preparation=key)
                try: parent.rename(directory)
                except OSError:
                    if not directory.exists(): raise
                del bench
            finally:
                inputs.watch = oldwatch
                reference_graphics()
                reference_validation()
                shutil.rmtree(temporary)
            gc.collect()
        bench,state = af_state.load(directory)
        saved=state.get('preparation')
        if key is not None:
            if saved != json.loads(json.dumps(key)):
                raise ValueError('Prepared state scene/start/timing/snapshot/runtime inputs differ')
        else:
            # Explicit parents are inputs in their own right. Native/model,
            # class, data, RAM and architecture checks above remain mandatory.
            # Omitted scene/start/profile are inferred; declared overrides must
            # agree with the parent and never silently reconfigure it.
            declared=explicit or set()
            if saved:
                checks={k:fingerprint(options[k]) for k in declared if k not in ('snapshot','scene','coverage')}
                if any(saved['options'].get(k)!=v for k,v in checks.items()):
                    raise ValueError('Prepared state declared options differ')
                if 'scene' in declared and saved['scene']!=fingerprint(options['scene']):
                    raise ValueError('Prepared state declared scene differs')
                if 'snapshot' in declared and saved['snapshot']!=hashlib.sha256((Path(options['snapshot'])/'manifest.json').read_bytes()).hexdigest():
                    raise ValueError('Prepared state declared snapshot differs')
                if saved['seed']!=seed: raise ValueError('Prepared state seed differs')
            else:
                if declared-{'scene','lens_start','timing','instructions_per_tick'}:
                    raise ValueError('Legacy parent cannot verify these declared options; prepare a new state')
                receipt=json.loads((directory/'receipt.json').read_bytes())
                if 'scene' in declared:
                    expected=options['scene'].describe();actual=bench.scene.describe()
                    for value in (expected,actual):
                        for name in ('last','last_pd'): value.pop(name,None)
                    if fingerprint(actual)!=fingerprint(expected): raise ValueError('Legacy prepared scene differs')
                if 'lens_start' in declared and receipt['start']!=options['lens_start']:
                    raise ValueError('Legacy prepared start differs')
                if receipt['seed']!=seed: raise ValueError('Legacy prepared seed differs')
                if ('timing' in declared and bench.timing!=options['timing']) or ('instructions_per_tick' in declared and bench.instructions_per_tick!=options['instructions_per_tick']):
                    raise ValueError('Legacy prepared timing differs')
        bench._fresh = False
        bench.input_alignment = state['alignment']
        if bench.input_alignment is not None: bench.input_alignment.setdefault('seed',seed)
        bench.preparation = dict(method='verified-prepared-restore',path=str(directory),
            ram_sha256=state['ram']['sha256'],manifest_sha256=hashlib.sha256((directory/'manifest.json').read_bytes()).hexdigest(),
            cpu_s=time.process_time()-started_cpu,wall_s=time.perf_counter()-started_wall,runtime=runtime_identity())
        observers(bench,coverage)
        if vectorized: vector_graphics()
    if not strict_validation: periodic_validation()
    bench._vectorized = vectorized
    configure(bench,backend)
    if trace_on_demand and backend == 'full':
        from .af_fast import trace_on_demand as install_trace
        install_trace(bench)
    return bench
