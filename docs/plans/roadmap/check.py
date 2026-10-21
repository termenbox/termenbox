#!/usr/bin/env python3
"""Validate roadmap step files: required header fields and values, references, cycles, a done criterion. Exit 1 on errors."""
import re, sys, pathlib
D = pathlib.Path(__file__).parent
REQ = ['Status', 'Score', 'Preds', 'Lane', 'Area', 'Platform', 'Was']
AREAS = {'ux', 'terminal', 'agents', 'server', 'security', 'cloud', 'release', 'ci', 'docs', 'connectors', 'paid'}
PLATS = {'mobile', 'desktop', 'both', 'n/a'}
errs, warns = [], []
def hdr(f):
    t = f.read_text().split('\n'); h = {}
    for l in t[1:16]:
        m = re.match(r'(Status|Score|Preds|Lane|Pin|Area|Platform|Was): ?(.*)', l)
        if m and m.group(1) not in h: h[m.group(1)] = m.group(2).strip()
    return t, h
open_, arch = {}, {}
for f in sorted(list(D.glob('R*.md')) + list(D.glob('U*.md'))): open_[f.stem] = hdr(f)
for f in sorted((D / 'archive').glob('*.md')): arch[f.stem] = hdr(f)
for k, (t, h) in open_.items():
    if not re.match(r'# [RU]\d+[a-z]? ', t[0]): errs.append(f'{k}: title line')
    for r in REQ:
        if r not in h: errs.append(f'{k}: missing {r}')
    if h.get('Lane') not in ('free', 'paid'): errs.append(f"{k}: Lane {h.get('Lane')}")
    if 'Tier' in h: warns.append(f'{k}: Tier is deprecated (decision 0125); delete the line')
    if h.get('Area') not in AREAS: errs.append(f"{k}: Area {h.get('Area')}")
    if h.get('Platform') not in PLATS: errs.append(f"{k}: Platform {h.get('Platform')}")
    if not re.fullmatch(r'V=\d+ T=\d+ R=\d+ S=\d+', h.get('Score', '')): errs.append(f'{k}: Score format')
    st = h.get('Status', '')
    if st.startswith(('done', 'superseded', 'folded')): errs.append(f'{k}: {st.split()[0]} steps belong in archive/')
    body = '\n'.join(t)
    if '## Score rationale' not in body: warns.append(f'{k}: no Score rationale')
    if not re.search(r'Done[- ]when', body, re.I) and 'Check:' not in body: warns.append(f'{k}: no Done-when or Check')
    for p in [x.strip() for x in h.get('Preds', 'none').split(',') if x.strip() and x.strip() != 'none']:
        if p not in open_ and p not in arch: errs.append(f'{k}: unknown predecessor {p}')
        elif p in arch: errs.append(f'{k}: predecessor {p} is archived; drop it')
for k, (t, h) in arch.items():
    if not h.get('Status', '').startswith(('done', 'superseded', 'folded')): errs.append(f'archive/{k}: Status {h.get("Status")}')
# cycles
seen = {}
def visit(k, path):
    if seen.get(k) == 2: return
    if seen.get(k) == 1: errs.append('cycle: ' + ' -> '.join(path + [k])); return
    seen[k] = 1
    for p in [x.strip() for x in open_[k][1].get('Preds', 'none').split(',') if x.strip() in open_]: visit(p, path + [k])
    seen[k] = 2
for k in open_: visit(k, [])
print(f'{len(open_)} open steps, {len(arch)} archived; {len(errs)} errors, {len(warns)} warnings')
for e in errs: print('ERROR', e)
for w in warns: print('warn ', w)
sys.exit(1 if errs else 0)
