"""Assemble explicitly selected registry units and checked native flash slices.

Only ignored, regenerable slice files are materialized for the existing
manifest consumer. There is no header decoder, model switch or section rename.
"""
import hashlib
import json
import os
import tempfile


def boot_layout(entry, firmware, manifest, flash, root):
    from tools import firmware_registry as registry
    from .profile import ProfileError, checked_bytes, number, relative_path
    layout = firmware.get('boot_layout')
    if layout is None:
        return manifest
    sections, selected_units = [], set()
    for item in layout['units']:
        unit = item['unit']
        if unit == 'main' or unit in selected_units or unit not in registry.unit_names(entry):
            raise ProfileError('Boot layout requires unique explicit sibling units')
        selected_units.add(unit)
        ref = item['manifest']
        expected_path = registry.unit_path(entry, 'mapped', unit) / 'manifest.json'
        if relative_path(root, ref['path']) != expected_path.resolve():
            raise ProfileError('Boot unit manifest must come from its registry unit')
        value = json.loads(checked_bytes(root, ref))
        if value.get('unit') != unit or value['source_sha256'] != entry['source']['sha256']:
            raise ProfileError('Boot unit manifest identity differs')
        # This remains the registry's unit decoder/validator, not a second one.
        if value != registry.mapped_manifest(entry, unit=unit):
            raise ProfileError('Boot unit manifest differs from registry resolution')
        sections.extend(value['sections'])
    for item in layout['flash_slices']:
        offset, size, address = (number(item[k]) for k in ('offset', 'size', 'address'))
        if size == 0 or offset + size > len(flash):
            raise ProfileError('Boot native flash slice exceeds firmware')
        data = flash[offset:offset + size]
        if hashlib.sha256(data).hexdigest() != item['sha256']:
            raise ProfileError('Boot native flash slice hash differs')
        path = relative_path(root, item['file'])
        generated_root = (root / '.tools' / 'firmware' / entry['id']).resolve()
        if not path.is_relative_to(generated_root):
            raise ProfileError('Boot flash slice output must be a firmware build artifact')
        if path.exists() and path.read_bytes() != data:
            raise ProfileError('Existing boot slice differs; regenerate in a fresh build location')
        if not path.exists():
            path.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as handle:
                temporary = handle.name
                handle.write(data)
            try:
                # A concurrent materialization can only install the same verified bytes.
                os.link(temporary, path)
            except FileExistsError:
                if path.read_bytes() != data:
                    raise ProfileError('Concurrent boot slice materialization differs')
            finally:
                os.unlink(temporary)
        sections.append(dict(name=item['name'], address=address, size=size,
                             file=item['file'], sha256=item['sha256'], native_flash_offset=offset))
    sections.extend(manifest['sections'])
    if len({s['name'] for s in sections}) != len(sections):
        raise ProfileError('Boot layout section names must be unique')
    spans = sorted((s['address'], s['address'] + s['size']) for s in sections)
    if any(lo < previous_hi for (_, previous_hi), (lo, _) in zip(spans, spans[1:])):
        raise ProfileError('Boot layout contains overlapping mapped sections')
    return {**manifest, 'sections': sections}
