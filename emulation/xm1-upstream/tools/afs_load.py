"""Load a portable snapshot allowing reviewed revisions of named model modules only."""
import json
from pathlib import Path


def load_dev(directory, allowed=('emulator.afs_measurement_data',)):
    from emulator.boot import make_board
    from x70_native import portable_checkpoint as pc
    directory = Path(directory)
    manifest = json.loads((directory/'manifest.json').read_text())
    dummy = make_board(**manifest['factory_options'])
    old, cur = manifest['identity'], pc._identity(dummy)
    del dummy
    revisions = {}
    for key in old:
        if key == 'models':
            for name in set(old[key]) | set(cur[key]):
                if old[key].get(name) != cur[key].get(name):
                    if name not in allowed:
                        raise RuntimeError('Unreviewed model change: '+name)
                    revisions['models.'+name] = {'saved': old[key][name], 'current': cur[key][name]}
        elif old[key] != cur[key]:
            raise RuntimeError('Identity differs outside models: '+key)
    return pc.load_for_development(directory, revisions) if revisions else pc.load(directory)
