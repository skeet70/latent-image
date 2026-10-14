"""Run the verified C configuration ABI in DriverHarness before S1.

AFBench retains its complete manifest/schema/stock-word preflight and hook
installation. Only its setup host_call runs in isolated native engines:
fetches are confined to the built .text, stores to .bss/shared configuration
plus caller stack, and no MMIO is accepted. Copying that resulting payload
back preserves the prepared scene clock instead of scheduling an RTOS call.
"""
import json
from pathlib import Path


def install(bench,build,config):
    from x70_native.driver_harness import DriverHarness
    path=Path(build);path=path/'manifest.json' if path.is_dir() else path
    manifest=json.loads(path.read_bytes())
    symbols={s['name']:s for s in manifest['symbols']}
    setup=int(symbols['af_setup']['address'],0)
    original=bench.tb.host_call
    def call(entry,*args,**options):
        if entry!=setup:raise ValueError('Quick configuration accepts only the built af_setup ABI')
        payloads=[];stores=[];code=[]
        for region in manifest['regions']:
            base=int(region['address'],0);size=region['size']
            payloads.append((base,bench.board.read_memory(base,size)))
            for section in region['sections']:
                low=base+section['offset'];high=low+section['size']
                if section['name']=='.text':code.append((low,high))
                elif section['name'] in ('.bss','.x70mod.shared','.data'):stores.append((low,high))
        harness=DriverHarness(sections=('kernel0',),ram_fixture=(),payloads=tuple(payloads),fetch_guard=True)
        result=harness.call(entry,args=args,max_instructions=100000,record_stores=True,
                            allowed_stores=stores,allowed_code=code,allowed_functions=(0x4352c,))
        if not result['returned'] or result.get('failure') or result.get('transactions') or result.get('delivered_faults'):
            raise ValueError('C setup is not a pure bounded configuration call: '+str(result.get('failure')))
        for base,data in payloads:
            configured=harness.read(base,len(data))
            for low,high in code:
                if base<=low and high<=base+len(data) and configured[low-base:high-base]!=data[low-base:high-base]:
                    raise ValueError('C setup changed executable bytes')
            bench.board.write_memory(base,configured)
        bench.quick_setup=dict(engine='DriverHarness',entry=entry,call=result,
                               no_scene_clock_advance=True)
        return result['return_value']
    bench.tb.host_call=call
    try:
        from .af_mod import install_mod
        return install_mod(bench,build,config)
    finally:bench.tb.host_call=original


import hashlib
def freeze_mod(source,cache):
    """Pin one verified build before parallel feature trials/rebuilds."""
    import shutil
    import tempfile
    source=Path(source);raw=source.read_bytes();manifest=json.loads(raw)
    identity=hashlib.sha256(raw).hexdigest();parent=Path(cache)/'mods'
    parent.mkdir(parents=True,exist_ok=True);target=parent/identity
    def regions(directory):
        result=[]
        for region in manifest['regions']:
            name=Path(region['file'])
            if name.is_absolute() or '..' in name.parts:
                raise ValueError('Mod region file must stay inside its build')
            data=(directory/name).read_bytes()
            if len(data)!=region['size'] or hashlib.sha256(data).hexdigest()!=region['sha256']:
                raise ValueError('Mod region changed during rebuild or differs from manifest')
            result.append((name,data))
        return result
    if target.exists():
        if (target/'manifest.json').read_bytes()!=raw: raise ValueError('Pinned mod manifest differs')
        regions(target)
        return str(target/'manifest.json')
    data=regions(source.parent)
    temporary=Path(tempfile.mkdtemp(prefix='.prepare-',dir=parent))
    try:
        for name,blob in data:
            destination=temporary/name;destination.parent.mkdir(parents=True,exist_ok=True)
            destination.write_bytes(blob)
        (temporary/'manifest.json').write_bytes(raw)
        try: temporary.rename(target)
        except OSError:
            if not target.exists(): raise
            if (target/'manifest.json').read_bytes()!=raw: raise ValueError('Concurrent pinned manifest differs')
            regions(target)
    finally:
        if temporary.exists(): shutil.rmtree(temporary)
    return str(target/'manifest.json')
