"""Deterministic cold-boot NAND images; immutable DAT is referenced by hash.

Only committed data/OOB and declared initial fixtures survive reload. Pending
controller work, registers, IRQs and RAM belong to execution checkpoints.
Geometry and erased/ECC assumptions remain those of the type1 NAND model.
"""
import gzip
import hashlib
import json

GEOMETRY = {'page_bytes': 2048, 'spare_bytes': 512, 'pages_per_block': 64, 'blocks': 1024}
IMAGE_KIND = 'type1-nand-committed-image'


def export_image(nand):
    if nand.busy:
        raise ValueError('Persistent NAND export requires no pending transaction')
    value = dict(schema=1, kind=IMAGE_KIND, geometry=GEOMETRY,
        base=dict(sha256=nand.flash_sha256, size=len(nand.flash)),
        data_overlays=[{**item, 'data': item['data'].hex()} for item in nand.data_overlays],
        spare_pages=[[page, {**item, 'data': item['data'].hex()}]
                     for page, item in sorted(nand.spare_pages.items())],
        committed_data_pages=[[page, data.hex()] for page, data in sorted(nand.volatile_pages.items())],
        committed_spare_pages=[[page, data.hex()] for page, data in sorted(nand.volatile_spare_pages.items())])
    return gzip.compress(json.dumps(value, sort_keys=True, separators=(',', ':'),
                                    allow_nan=False).encode(), mtime=0)


def restore_image(image, *, flash_sha256, flash_size):
    """Validate the entire image before returning independent mutable maps."""
    try:
        if type(image) is not bytes:
            raise ValueError('Persistent NAND image must be immutable bytes')
        value = json.loads(gzip.decompress(image))
        expected = {'schema', 'kind', 'geometry', 'base', 'data_overlays', 'spare_pages',
                    'committed_data_pages', 'committed_spare_pages'}
        if (type(value) is not dict or set(value) != expected or type(value['schema']) is not int
                or value['schema'] != 1 or value['kind'] != IMAGE_KIND or value['geometry'] != GEOMETRY
                or value['base'] != dict(sha256=flash_sha256, size=flash_size)):
            raise ValueError('Persistent NAND identity/geometry/schema mismatch')
        flash_bytes = GEOMETRY['page_bytes'] * GEOMETRY['pages_per_block'] * GEOMETRY['blocks']
        pages = flash_bytes // GEOMETRY['page_bytes']

        def provenance(item):
            if not isinstance(item, (str, dict)) or not item:
                raise ValueError('Persistent NAND fixtures require explicit provenance')

        overlays, previous_end = [], 0
        if type(value['data_overlays']) is not list:
            raise ValueError('Invalid persistent NAND overlays')
        for item in value['data_overlays']:
            if type(item) is not dict or set(item) != {'offset', 'data', 'sha256', 'provenance'}:
                raise ValueError('Invalid persistent NAND overlay record')
            offset, data = item['offset'], bytes.fromhex(item['data'])
            if (type(offset) is not int or offset < previous_end or not data
                    or offset + len(data) > flash_bytes
                    or hashlib.sha256(data).hexdigest() != item['sha256']):
                raise ValueError('Persistent NAND overlay extent/hash mismatch')
            provenance(item['provenance'])
            overlays.append({**item, 'data': data})
            previous_end = offset + len(data)

        def page_rows(rows, decode):
            if type(rows) is not list:
                raise ValueError('Invalid persistent NAND page table')
            result, previous = {}, -1
            for row in rows:
                if (type(row) is not list or len(row) != 2 or type(row[0]) is not int
                        or not previous < row[0] < pages):
                    raise ValueError('Persistent NAND pages must be ordered, unique and in bounds')
                result[row[0]] = decode(row[1])
                previous = row[0]
            return result

        def raw(data, size):
            decoded = bytes.fromhex(data)
            if len(decoded) != size:
                raise ValueError('Persistent NAND page size mismatch')
            return decoded

        def spare_fixture(item):
            if type(item) is not dict or set(item) != {'data', 'prefix_bytes', 'sha256', 'provenance'}:
                raise ValueError('Invalid persistent NAND spare fixture')
            data = raw(item['data'], GEOMETRY['spare_bytes'])
            count = item['prefix_bytes']
            if (type(count) is not int or not 0 < count <= len(data)
                    or data[count:] != b'\xff' * (len(data) - count)
                    or hashlib.sha256(data[:count]).hexdigest() != item['sha256']):
                raise ValueError('Persistent NAND spare prefix/hash mismatch')
            provenance(item['provenance'])
            return {**item, 'data': data}

        spare = page_rows(value['spare_pages'], spare_fixture)
        data = page_rows(value['committed_data_pages'], lambda item: raw(item, GEOMETRY['page_bytes']))
        oob = page_rows(value['committed_spare_pages'], lambda item: raw(item, GEOMETRY['spare_bytes']))
        if set(data) != set(oob):
            raise ValueError('Committed NAND data/OOB pages must be paired')
        return overlays, spare, data, oob
    except (KeyError, TypeError, OSError, EOFError, UnicodeError, json.JSONDecodeError) as exc:
        raise ValueError('Malformed persistent NAND image') from exc
