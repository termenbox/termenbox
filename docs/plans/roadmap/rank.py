#!/usr/bin/env python3
"""Regenerate the Order table in docs/plans/termenbox-roadmap.md from the step files in this directory.

Step file header (one per step, R01.md, U04.md ...), in this order, directly under the title line:
  Status: pending | blocked (why) | done (PR #n)      [done, superseded and folded steps live in archive/]
  Score:  V=5 T=3 R=4 S=2                              value, time criticality, risk or opportunity (1-10), size (1,2,3,5,8,13)
  Preds:  R02, U04 | none                              open predecessors only (done ones are dropped)
  Lane:   free | paid                                  paid steps come after every free step (decision 0093)
  Pin:    after Rnn                                    (optional) keeps a step where the owner put it
  Area:   ux | terminal | agents | server | security | cloud | release | ci | docs | connectors | paid
  Platform: mobile | desktop | both | n/a              which client the step serves (n/a: no client surface)
  Was:    where the step came from
WSJF = (V+T+R)/S. A step takes the best bundle score of any free-lane chain it unlocks.
There are no priority tiers (decision 0125): the order is lane, chain score, own WSJF, never before a predecessor, pins last.
Run `python3 docs/plans/roadmap/check.py` first (header and reference checks), then this script.
"""
import re, pathlib
D = pathlib.Path(__file__).parent
INDEX = D.parent / 'termenbox-roadmap.md'
steps = {}
for f in sorted(list(D.glob('R*.md')) + list(D.glob('U*.md'))):
    t = f.read_text().split('\n')
    s = {'id': f.stem, 'title': re.sub(r'^# [RU]\d+[a-z]? ', '', t[0]), 'file': f.name}
    for l in t[1:16]:
        m = re.match(r'(Status|Score|Preds|Lane|Pin|Area|Platform|Was): ?(.*)', l)
        if m and m.group(1).lower() not in s: s[m.group(1).lower()] = m.group(2).strip()
    sc = dict(re.findall(r'([VTRS])=(\d+)', s.get('score', '')))
    s['v'], s['t'], s['r'], s['size'] = (int(sc.get(k, 0)) for k in 'VTRS')
    s['preds'] = [] if s.get('preds', 'none') == 'none' else [p.strip() for p in s['preds'].split(',')]
    s['lane'] = s.get('lane', 'free')
    s['own'] = (s['v'] + s['t'] + s['r']) / max(s['size'], 1)
    steps[s['id']] = s
act = steps
def anc(k, seen=None):
    seen = seen if seen is not None else set()
    for p in act[k]['preds']:
        if p in act and p not in seen: seen.add(p); anc(p, seen)
    return seen
cod = lambda k: act[k]['v'] + act[k]['t'] + act[k]['r']
def bundle(d):
    c = anc(d) | {d}; return sum(cod(x) for x in c) / sum(max(act[x]['size'], 1) for x in c)
desc = {k: [d for d in act if k in anc(d) and act[d]['lane'] == act[k]['lane']] for k in act}
eff = {k: max([act[k]['own']] + [bundle(d) for d in desc[k]]) for k in act}
keyf = lambda x: (act[x]['lane'] == 'paid', -eff[x], -act[x]['own'], x)
order, placed = [], set()
def place(k):
    if k in placed: return
    placed.add(k)
    for p in sorted([p for p in act[k]['preds'] if p in act], key=keyf): place(p)
    order.append(k)
for k in sorted(act, key=keyf): place(k)
# Pins keep a sequenced programme in plan order: a pinned step moves to directly after its target.
for _ in range(len(act)):
    moved = False
    for k in list(order):
        m = re.match(r'after ([RU]\d+[a-z]?)', act[k].get('pin', ''))
        if m and m.group(1) in order and order.index(k) != order.index(m.group(1)) + 1:
            order.remove(k); order.insert(order.index(m.group(1)) + 1, k); moved = True
    if not moved: break
# A predecessor that a pin left behind is pulled to directly before the step that needs it.
for _ in range(len(act) * 2):
    pos = {k: i for i, k in enumerate(order)}; fixed = True
    for k in list(order):
        for p in act[k]['preds']:
            if p in pos and pos[p] > pos[k]:
                order.remove(p); order.insert(order.index(k), p); fixed = False; break
        if not fixed: break
    if fixed: break
pos = {k: i for i, k in enumerate(order)}
for k in order:
    for p in act[k]['preds']:
        if p in pos and pos[p] > pos[k]: raise SystemExit(f'order puts {k} before its predecessor {p}; fix a Pin or Preds')
rows = ['| # | Step | Title | Area | Platform | Status | WSJF | Chain | Preds |', '|---|---|---|---|---|---|---|---|---|']
for i, k in enumerate(order, 1):
    s = act[k]
    rows.append(f"| {i} | [{k}](roadmap/{s['file']}) | {s['title']} | {s.get('area', '-')} | {s.get('platform', '-')} | {s.get('status', 'pending')} | {s['own']:.1f} | {eff[k]:.1f} | {', '.join(s['preds']) or '-'} |")
arch = sorted((D / 'archive').glob('*.md'))
nd = sum(1 for f in arch if f.read_text().split('\n')[1].startswith('Status: done'))
ns = len(arch) - nd
ready = [k for k in order if not act[k]['preds'] and act[k].get('status', 'pending').startswith('pending')]
nxt = f"Next ready step: [{ready[0]}](roadmap/{act[ready[0]]['file']}).\n\n" if ready else ''
table = nxt + '\n'.join(rows) + f"\n\n{len(order)} open steps. {nd} done and {ns} superseded or folded steps are in `roadmap/archive/` (their text is kept; `grep -h '^Status' roadmap/archive/*.md`).\n"
txt = INDEX.read_text()
new = re.sub(r'(<!-- order:start -->\n).*?(<!-- order:end -->)', lambda m: m.group(1) + table + m.group(2), txt, flags=re.S)
INDEX.write_text(new)
print(f'{len(order)} open, {nd} done, {ns} superseded/folded; next: ' + ', '.join(order[:4]))
