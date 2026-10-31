// The steps a case declares once (DECISION-0161): the chapter's numbered list is generated from them, and a render fails when
// the case skips one, repeats one or performs them out of order.

/** A step is {id, caption, label}: `caption` is what the screen says, `label` the exact text or name that is tapped (null when nothing is). */
export const checkDeclared = (caseId, steps) => {
  if (!Array.isArray(steps) || steps.length === 0) throw new Error(`case ${caseId}: declares no steps`);
  const seen = new Set();
  for (const s of steps) {
    if (!s.id || !s.caption || s.label === undefined) throw new Error(`case ${caseId}: a step needs id, caption and label (null for no tap)`);
    if (seen.has(s.id)) throw new Error(`case ${caseId}: step ${s.id} is declared twice`);
    seen.add(s.id);
  }
};

/** Tracks the steps a run performs. `reach(id)` returns the declared step; `missing()` lists those never reached. */
export const stepTracker = (steps) => {
  let next = 0;
  return {
    reach(id) {
      const declared = steps.find((s) => s.id === id);
      if (!declared) throw new Error(`step ${id} is not declared by this case`);
      if (steps[next]?.id !== id) {
        const expected = steps[next] ? `expected ${steps[next].id}` : 'all steps were already reached';
        throw new Error(`step ${id} is out of order: ${expected}`);
      }
      next++;
      return declared;
    },
    missing: () => steps.slice(next).map((s) => s.id),
    assertComplete() {
      const left = this.missing();
      if (left.length) throw new Error(`steps never reached: ${left.join(', ')}`);
    },
  };
};
