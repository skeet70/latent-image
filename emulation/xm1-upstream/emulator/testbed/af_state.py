"""Explicit portable pre-S1 AF state, extending the native data-only graph.

Only curated model types, the PDAF edge closure and pinned graphics completions are
reconstructed. No pickle, opaque CPU contexts, code from disk, or new Uc.
Observation hooks must never have been installed on the saved board.
"""
import ctypes
import gzip
import hashlib
import importlib
import json
from pathlib import Path
from types import MethodType, FunctionType

SCHEMA = 'x70-af-pre-s1-v1'
MODULES = ('emulator.testbed.sensor_stream', 'emulator.testbed.scene_sources',
           'emulator.testbed.inputs', 'emulator.testbed.patch',
           'emulator.testbed.events', 'emulator.testbed.lens_trace',
           'emulator.testbed.sensor_model', 'emulator.testbed.radiance_stacks', 'emulator.pdaf_stream')
CLOSURES = (('emulator.pdaf_stream', 'PdafStream._hook', 'on_edge'),
            ('emulator.graphics_devices', 'GraphicsAccelerator._start_copy', 'complete'),
            ('emulator.graphics_devices', 'GraphicsAccelerator._start_ui_draws', 'complete'),
            ('emulator.graphics_devices', 'GraphicsAccelerator._start_commands', 'complete'),
            ('emulator.graphics_devices', 'GraphicsAccelerator._start_fill', 'complete'))

def closure_code(module, parent, name):
    value=importlib.import_module(module)
    for part in parent.split('.'): value=getattr(value,part)
    return next(c for c in value.__code__.co_consts if hasattr(c,'co_name') and c.co_name==name)

def paused(board):
    # Export at the original source ACK, preserving the declared pending
    # graphics completion rather than advancing guest time to drain it.
    if board.executing_cpu is not None or board.failure or board.remap_requested or not board.last_quantum_complete or not board._checkpoint_boundary:
        raise ValueError('AF snapshot requires a successful completed boundary')
    if getattr(board,'_restorable_low_remap',None) is not None or getattr(board,'running',False):
        raise ValueError('AF snapshot refuses incomplete remap/execution')
    for core in range(board.active_cpu+1,len(board.cpus)):
        state=board.core_state[core]
        if not state.get('runnable',True): continue
        bank=board.interrupts.banks[core]
        if state.get('waiting',False) and not bank.pending & bank.enabled: continue
        raise ValueError('AF snapshot would change CPU scheduling order')



def classes():
    result = {}
    for name in MODULES:
        module = importlib.import_module(name)
        for value in vars(module).values():
            if isinstance(value, type) and value.__module__ == name:
                result[name + '.' + value.__qualname__] = value
    return result


def graph_types(pc):
    import numpy as np
    curated = classes()

    class Encoder(pc._Encoder):
        def encode(self, value, owner=None, field=None):
            kind = pc._name(type(value))
            custom = (kind in curated or isinstance(value, (np.ndarray, np.generic, MethodType))
                      or (isinstance(value, FunctionType) and any(value.__module__==m and value.__code__ is closure_code(m,p,n) for m,p,n in CLOSURES)))
            if not custom or id(value) in self.references:
                try:
                    return super().encode(value, owner, field)
                except pc.PortableCheckpointError as error:
                    if isinstance(value, FunctionType):
                        raise ValueError('Uncurated AF callback '+value.__module__+'.'+value.__qualname__) from error
                    raise
            if id(value) in self.memo:
                return {'node': self.memo[id(value)]}
            index = len(self.nodes)
            if index >= pc.MAX_NODES:
                raise ValueError('AF graph bound exceeded')
            self.memo[id(value)] = index
            node = {}; self.nodes.append(node)
            if isinstance(value, np.ndarray):
                if value.dtype.hasobject:
                    raise ValueError('Object array refused')
                node.update(kind='af_array', dtype=value.dtype.str, shape=list(value.shape), data=self.writer.blob(value.tobytes(order='C')))
            elif isinstance(value, np.generic):
                node.update(kind='af_scalar', dtype=value.dtype.str, data=self.writer.blob(value.tobytes()))
            elif isinstance(value, MethodType):
                target = value.__self__
                if target is not self.board and id(target) not in self.references and pc._name(type(target)) not in curated:
                    raise ValueError('Uncurated AF callback owner')
                node.update(kind='af_method', owner=self.encode(target), method=value.__name__, identity=pc._callback_identity(value))
            elif isinstance(value, FunctionType):
                if value.__defaults__ is not None or value.__kwdefaults__ is not None:
                    raise ValueError('Uncurated AF closure')
                node.update(kind='af_edge', module=value.__module__, qualname=value.__qualname__, identity=pc._callback_identity(value),
                            cells=[self.encode(c.cell_contents) for c in value.__closure__])
            else:
                node.update(kind='af_object', cls=kind, fields=self.state(vars(value), None))
            return {'node': index}

    class Decoder(pc._Decoder):
        def decode(self, value):
            if type(value) is not dict or 'node' not in value:
                return super().decode(value)
            index = value['node']
            if type(index) is not int or not 0 <= index < len(self.nodes):
                raise ValueError('Invalid AF graph reference')
            if index in self.cache:
                return self.cache[index]
            node = self.nodes[index]; kind = node['kind']
            if not kind.startswith('af_'):
                return super().decode(value)
            if kind == 'af_object':
                if node['cls'] not in curated:
                    raise ValueError('Uncurated AF class')
                result = object.__new__(curated[node['cls']]); self.cache[index] = result
                result.__dict__.update(self.state(node['fields']))
            elif kind in ('af_array', 'af_scalar'):
                dtype = np.dtype(node['dtype'])
                if dtype.hasobject:
                    raise ValueError('Object array refused')
                data = self.reader.blob(node['data'])
                if kind == 'af_array':
                    result = np.frombuffer(data, dtype=dtype).reshape(node['shape']).copy()
                else:
                    result = np.frombuffer(data, dtype=dtype)[0]
            elif kind == 'af_method':
                result = getattr(self.decode(node['owner']), node['method'])
                if not isinstance(result, MethodType) or pc._callback_identity(result) != node['identity']:
                    raise ValueError('AF method code identity differs')
            elif kind == 'af_edge':
                matches=[(m,p,n) for m,p,n in CLOSURES if m==node['module'] and p+'.<locals>.'+n==node['qualname']]
                if len(matches)!=1: raise ValueError('Uncurated closure')
                code=closure_code(*matches[0])
                def cell(v):
                    return (lambda: v).__closure__[0]
                module = importlib.import_module(node['module'])
                result = FunctionType(code, vars(module), closure=tuple(cell(self.decode(v)) for v in node['cells']))
                if pc._callback_identity(result) != node['identity']:
                    raise ValueError('AF edge code identity differs')
            else:
                raise ValueError('Unknown AF node')
            self.cache[index] = result
            return result
    return Encoder, Decoder


def save(bench, directory, alignment, preparation=None):
    from x70_native import portable_checkpoint as pc
    from x70_native.architecture import export_cpu, validate_cpu_state
    board = bench.board
    pc._require_native_modes_disabled(board)
    paused(board)
    eligible = pc._require_native_portable(board)
    directory = Path(directory); directory.mkdir(parents=True, exist_ok=False)
    writer = pc._Writer(directory); Encoder, _ = graph_types(pc)
    encoder = Encoder(board, writer)
    model = dict(board=encoder.state({k:v for k,v in pc.reference._board_state(board).items() if k!='testbed_events'}, 'board'),
                 devices=[encoder.state(vars(d), i) for i,d in enumerate(board.devices)])
    patches = {name:dict(base=p.base,code=writer.blob(p.code),entries=p.entries,handle=encoder.encode(p.handle),
                 state_base=p.state_base,state_offsets=p.state_offsets) for name,p in bench.patches.items()}
    model['nodes'] = encoder.nodes
    raw = pc._json(model); compressed = gzip.compress(raw, compresslevel=1, mtime=0)
    (directory/'model.json.gz').write_bytes(compressed)
    size, pages, ramhash = pc.reference._sparse_ram(board)
    arch = [export_cpu(cpu) for cpu in board.cpus]
    for state in arch: validate_cpu_state(state)
    manifest = dict(schema=SCHEMA, identity=pc._identity(board), native_eligibility=eligible,
        af_classes={name:pc._class(cls)['sha256'] for name,cls in classes().items()},
        factory_options=board._portable_factory_options, device_classes=[pc._name(type(d)) for d in board.devices],
        model=dict(bytes=len(raw),sha256=pc._sha(raw),compressed_sha256=pc._sha(compressed)),
        ram=dict(bytes=size,sha256=ramhash,pages=[[o,writer.blob(d)] for o,d in pages.items()]),
        sram=writer.blob(bytes(board.sram)), vectors=writer.blob(bytes(board.vector_scratch)),
        private=[[[a,writer.blob(d)] for a,d in ranges] for ranges in pc.reference._private_memory(board)],
        architecture=arch,low_rom_alias=board.low_rom_alias,mappings=pc.reference._mapping_layout(board),
        bench=dict(instructions_per_tick=bench.instructions_per_tick, timing=bench.timing.name),
        patches=patches,preparation=preparation, alignment=alignment,helper=dict(entry=bench._move_entry,counter=bench._counter,receipt=bench._helper_receipt),
        blobs=writer.blobs)
    (directory/'manifest.json').write_bytes(pc._json(manifest)+b'\n')
    return dict(path=str(directory),ram_sha256=ramhash,nodes=len(encoder.nodes))


def load(directory):
    from x70_native import portable_checkpoint as pc
    from x70_native.architecture import import_cpu, export_cpu, validate_cpu_state
    from emulator.boot import make_board
    from emulator.pdaf_stream import PhasePrefilterControl
    from emulator.testbed.afbench import AFBench
    from emulator.testbed.testbed import Testbed
    from emulator.testbed import timing
    directory=Path(directory); m=json.loads((directory/'manifest.json').read_bytes())
    if m['schema'] != SCHEMA: raise ValueError('Unknown AF state schema')
    current_classes={name:pc._class(cls)['sha256'] for name,cls in classes().items()}
    if any(current_classes.get(name)!=sha for name,sha in m['af_classes'].items()): raise ValueError('AF class identity differs')
    board=make_board(**m['factory_options'])
    expected=[pc._name(type(d)) for d in board.devices]
    if m['device_classes'] != expected+['emulator.pdaf_stream.PhasePrefilterControl']:
        raise ValueError('AF snapshot device topology differs')
    extra=object.__new__(PhasePrefilterControl); extra.board=board
    board.attach(extra)
    if ctypes.sizeof(board.ram)!=m['ram']['bytes']: raise ValueError('AF DRAM size differs')
    if pc._identity(board) != m['identity']: raise ValueError('AF snapshot model/native identity differs')
    if pc._require_native_portable(board) != m['native_eligibility']: raise ValueError('Native eligibility differs')
    if pc._json(pc.reference._mapping_layout(board)) != pc._json(m['mappings']): raise ValueError('AF mappings differ')
    reader=pc._Reader(directory,m['blobs']); _,Decoder=graph_types(pc)
    compressed=(directory/'model.json.gz').read_bytes()
    if pc._sha(compressed)!=m['model']['compressed_sha256']: raise ValueError('AF model compressed hash differs')
    if not 0<=m['model']['bytes']<=pc.MAX_STATE_BYTES: raise ValueError('AF graph size bound exceeded')
    import io
    with gzip.GzipFile(fileobj=io.BytesIO(compressed)) as stream: raw=stream.read(m['model']['bytes']+1)
    if len(raw)!=m['model']['bytes'] or pc._sha(raw)!=m['model']['sha256']: raise ValueError('AF model hash differs')
    model=json.loads(raw)
    if any(n.get('kind')=='af_object' and n['cls'] not in m['af_classes'] for n in model['nodes']):
        raise ValueError('AF graph class lacks a saved identity')
    decoder=Decoder(board,reader,model['nodes'])
    python=decoder.state(model['board']); devices=[decoder.state(s) for s in model['devices']]
    if len(devices)!=len(board.devices) or len(m['architecture'])!=len(board.cpus) or len(m['private'])!=len(board.cpus) or m['ram']['bytes']!=ctypes.sizeof(board.ram): raise ValueError('AF topology/backing counts differ')
    if set(python)&(pc.reference.BOARD_IDENTITIES|pc.reference.DERIVED_FIELDS): raise ValueError('Backing field refused')
    _,fresh,_=pc.reference._sparse_ram(board)
    for offset,data in fresh.items(): ctypes.memset(board.ram_pointer+offset,0,len(data))
    last=-1
    for offset,blob in m['ram']['pages']:
        data=reader.blob(blob)
        if offset<=last or offset%pc.PAGE_SIZE or len(data)!=pc.PAGE_SIZE or offset+len(data)>ctypes.sizeof(board.ram): raise ValueError('AF RAM page invalid')
        ctypes.memmove(board.ram_pointer+offset,data,len(data)); last=offset
    for backing,key in ((board.sram,'sram'),(board.vector_scratch,'vectors')):
        data=reader.blob(m[key])
        if len(data)!=ctypes.sizeof(backing): raise ValueError('AF backing size differs')
        ctypes.memmove(ctypes.addressof(backing),data,len(data))
    for core,(cpu,ranges) in enumerate(zip(board.cpus,m['private'])):
        expected=pc.reference._private_ranges(board,core)
        if len(ranges)!=len(expected): raise ValueError('AF private ranges differ')
        for (address,blob),(a,n) in zip(ranges,expected):
            data=reader.blob(blob)
            if address!=a or len(data)!=n: raise ValueError('AF private range invalid')
            cpu.mem_write(address,data)
    if board.low_rom_alias!=m['low_rom_alias']: board._remap_low_alias(m['low_rom_alias'])
    for k in list(vars(board)):
        if k not in pc.reference.BOARD_IDENTITIES|pc.reference.DERIVED_FIELDS: del vars(board)[k]
    vars(board).update(python)
    for device,state in zip(board.devices,devices): vars(device).clear();vars(device).update(state)
    for cpu,state in zip(board.cpus,m['architecture']):
        validate_cpu_state(state);import_cpu(cpu,state)
        if export_cpu(cpu)!=state: raise ValueError('AF CPU restore differs')
    board.invalidate_execution_caches()
    paused(board);pc._require_native_portable(board)
    if pc.reference._sparse_ram(board)[2]!=m['ram']['sha256']: raise ValueError('AF RAM restore hash differs')
    bench=AFBench(Testbed(board),board.sensor_stream,board.pdaf_stream,
                  board.sensor_stream.lens,board.lens_trace,board.sensor_stream.scene,None,m.get('bench',{}).get('instructions_per_tick',24),
                  timing=timing.resolve(m.get('bench',{}).get('timing','camera')))
    bench._move_entry=m['helper']['entry'];bench._counter=m['helper']['counter'];bench._helper_receipt=m['helper']['receipt']
    from emulator.testbed.afbench import InstalledPatch
    bench.patches = {name:InstalledPatch(bench, p['base'], reader.blob(p['code']), p['entries'],
                    decoder.decode(p['handle']), p['state_base'], p['state_offsets']) for name,p in m.get('patches',{}).items()}
    return bench,m
