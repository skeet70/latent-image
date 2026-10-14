"""Execute the validated profile graph inside the canonical boot factory.

Constructors and scenario hooks are supplied by boot.py. This module owns no
board constructor, firmware selection, device list or per-body branches.
"""

def wire_graph(board, profile, builders, after_attach, *, instances):
    nodes = {node['name']: node for node in profile['devices']}
    pending = iter(profile['attachment_order'])
    next_attach = next(pending, None)
    for name in profile['construction_order']:
        node = nodes[name]
        for reference in node.get('refs', {}).values():
            if reference not in instances:
                raise ValueError('Unconstructed graph dependency: ' + reference)
        try:
            build = builders[node['kind']]
        except KeyError:
            raise ValueError('No constructor for graph kind: ' + node['kind']) from None
        instances[name] = build(**{key:instances[ref] for key,ref in node.get('refs', {}).items()})
        while next_attach in instances:
            device = instances[next_attach]
            board.attach(device)
            after_attach(nodes[next_attach]['kind'], device)
            next_attach = next(pending, None)
    if next_attach is not None:
        raise ValueError('Graph did not construct every attachment')
