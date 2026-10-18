#!/usr/bin/env python3
"""Print the next unblocked roadmap task, ordered by tier then WSJF.

Mirrors the ordering logic of `rank.py` (Tier -> effective/bundle WSJF ->
own WSJF -> id, predecessors placed before dependents), then returns the
first step that is ready (all Preds already archived, i.e. not in the open
set R*.md/U*.md) and Status == pending. That is the next task to pick up.

Run: python3 docs/plans/roadmap/next.py   (on Windows `python3` may be a
store stub; use `py docs/plans/roadmap/next.py` instead).
"""
import re, pathlib, sys

D = pathlib.Path(__file__).parent
TIER = {'P0': 0, 'P1': 1, 'P2': 2, 'P3': 3, 'Paid': 4}

steps = {}
for f in sorted(list(D.glob('R*.md')) + list(D.glob('U*.md'))):
    t = f.read_text().split('\n')
    h = {}
    for l in t[1:16]:
        m = re.match(r'(Status|Score|Preds|Tier|Pin): ?(.*)', l)
        if m:
            h[m.group(1)] = m.group(2).strip()
    sc = dict(re.findall(r'([VTRS])=(\d+)', h.get('Score', '')))
    v, t_, r, size = (int(sc.get(k, 0)) for k in 'VTRS')
    own = (v + t_ + r) / max(size, 1)
    preds = [] if h.get('Preds', 'none') == 'none' else [p.strip() for p in h['Preds'].split(',')]
    steps[f.stem] = {
        'id': f.stem,
        'title': re.sub(r'^# [RU]\d+[a-z]? ', '', t[0]),
        'status': h.get('Status', 'pending'),
        'tier': h.get('Tier', 'P3'),
        'pin': h.get('Pin', ''),
        'v': v, 't': t_, 'r': r, 'size': size,
        'own': own,
        'preds': preds,
    }

open_ids = set(steps)

def ancestors(k, seen=None):
    if seen is None:
        seen = set()
    for p in steps[k]['preds']:
        if p in steps and p not in seen:
            seen.add(p)
            ancestors(p, seen)
    return seen

descendants = {k: [d for d in steps if k in ancestors(d)] for k in steps}

def cod(k):
    s = steps[k]
    return s['v'] + s['t'] + s['r']

def bundle(k):
    chain = ancestors(k) | {k}
    return sum(cod(x) for x in chain) / sum(max(steps[x]['size'], 1) for x in chain)

eff = {k: max([steps[k]['own']] + [bundle(d) for d in descendants[k]]) for k in steps}

keyf = lambda k: (TIER.get(steps[k]['tier'], 9), -eff[k], -steps[k]['own'], k)

order, placed = [], set()
def place(k):
    if k in placed:
        return
    placed.add(k)
    for p in sorted([p for p in steps[k]['preds'] if p in steps], key=keyf):
        place(p)
    order.append(k)

for k in sorted(steps, key=keyf):
    place(k)

# Pins keep a sequenced programme in plan order: move a pinned step to
# directly after its target when the target is open.
for _ in range(len(steps)):
    moved = False
    for k in list(order):
        m = re.match(r'after ([RU]\d+[a-z]?)', steps[k]['pin'])
        if m and m.group(1) in order:
            i, j = order.index(k), order.index(m.group(1))
            if i != j + 1:
                order.remove(k)
                order.insert(j + 1, k)
                moved = True
    if not moved:
        break

# Pull any predecessor a pin left behind to directly before the step needing it.
for _ in range(len(steps) * 2):
    pos = {k: i for i, k in enumerate(order)}
    fixed = True
    for k in list(order):
        for p in steps[k]['preds']:
            if p in pos and pos[p] > pos[k]:
                order.remove(p)
                order.insert(order.index(k), p)
                fixed = False
                break
        if not fixed:
            break
    if fixed:
        break

for k in order:
    s = steps[k]
    if s['status'] != 'pending':
        continue
    if any(p in open_ids for p in s['preds']):
        continue
    print(f"Next: {k} — {s['title']}")
    print(f"  Tier: {s['tier']}  WSJF(own): {s['own']:.2f}  WSJF(eff): {eff[k]:.2f}  Preds: {', '.join(s['preds']) or '-'}")
    print(f"  File: roadmap/{k}.md")
    sys.exit(0)

print('No ready pending task.')
sys.exit(0)
