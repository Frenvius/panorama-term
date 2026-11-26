import { describe, it, expect } from 'bun:test';
import type { Routine, RoutineRun } from '~/domain/interfaces/agents.interface';
import { hookUrl, emptyForm, formToBody, historyRows, outcomeChip, validateForm, formFromRoutine } from '~/usecase/util/agentRoutines';

const run = (n: number, outcome: RoutineRun['outcome'], at: number): RoutineRun => ({
  routineId: 'rt1',
  n,
  firedAt: at - 10,
  trigger: 'manual',
  deliveredAt: at - 5,
  endedAt: outcome ? at : null,
  outcome,
  summary: outcome === 'reported' ? 'Build 412 is red\nmore' : null,
  costUsd: n === 1 ? 0.0123 : null
});

describe('outcomeChip', () => {
  it('maps every outcome to a chip', () => {
    expect(outcomeChip('reported')).toEqual({ kind: 'reported', label: 'reported' });
    expect(outcomeChip('no_update')).toEqual({ kind: 'quiet', label: 'no news' });
    expect(outcomeChip('failed').kind).toBe('failed');
    expect(outcomeChip('timeout')).toEqual({ kind: 'failed', label: 'timed out' });
    expect(outcomeChip('skipped').kind).toBe('skipped');
    expect(outcomeChip(null).label).toBe('not run yet');
  });
});

describe('historyRows', () => {
  it('merges runs and skips newest first, capped, with the first summary line and cost', () => {
    const runs = Array.from({ length: 12 }, (_, i) => run(i + 1, i % 2 ? 'no_update' : 'reported', 1000 + i * 100));
    const rows = historyRows(runs, [{ routineId: 'rt1', at: 5000, reason: 'overlap', count: 2 }]);
    expect(rows).toHaveLength(10);
    expect(rows[0].text).toBe('overlap (2)');
    expect(rows[1].chip.label).toBe('no news');
    expect(historyRows([run(1, 'reported', 10)], [])[0]).toMatchObject({ text: 'Build 412 is red', cost: '$0.012' });
    expect(historyRows([run(2, null, 10)], [])[0].chip.label).toBe('running');
  });
});

describe('validateForm', () => {
  const ok = { ...emptyForm(), name: 'Morning check', text: 'Check the build.' };
  it('accepts a valid weekday routine', () => {
    expect(validateForm(ok)).toBeNull();
  });
  it('refuses bad names, times, intervals and cron', () => {
    expect(validateForm({ ...ok, name: '' })).toContain('Name');
    expect(validateForm({ ...ok, name: 'a\nb' })).toContain('single-line');
    expect(validateForm({ ...ok, name: '[panorama routine x]' })).toContain('single-line');
    expect(validateForm({ ...ok, time: '25:00' })).toContain('HH:MM');
    expect(validateForm({ ...ok, kind: 'every', every: '4m' })).toContain('5m');
    expect(validateForm({ ...ok, kind: 'every', every: '30m' })).toBeNull();
    expect(validateForm({ ...ok, kind: 'cron', cron: '0 9 * *' })).toContain('5 fields');
    expect(validateForm({ ...ok, kind: 'weekly', days: [] })).toContain('day');
    expect(validateForm({ ...ok, text: 'x'.repeat(4097) })).toContain('4 KB');
  });
});

describe('formToBody', () => {
  it('maps each trigger kind to the brain fields', () => {
    const base = { ...emptyForm(), name: ' N ', text: 't' };
    expect(formToBody({ ...base, kind: 'weekdays', time: '08:30' })).toMatchObject({ name: 'N', at: '08:30', days: 'weekdays' });
    expect(formToBody({ ...base, kind: 'weekly', days: ['mon', 'thu'] })).toMatchObject({ days: 'mon,thu' });
    expect(formToBody({ ...base, kind: 'every', every: '30m' })).toMatchObject({ every: '30m' });
    expect(formToBody({ ...base, kind: 'hook' }).hook).toBe(true);
    expect(formToBody({ ...base, kind: 'manual' })).not.toHaveProperty('at');
  });
});

describe('formFromRoutine', () => {
  it('reads an interval routine back into the form', () => {
    const r = {
      name: 'N', instruction: 'I', trigger: { kind: 'every', ms: 1_800_000, anchor: 0 }, missed: 'skip', overlap: 'queue',
      session: 'fresh', reportTo: { kind: 'agent', id: 'a1' }, maxPerDay: 4
    } as unknown as Routine;
    expect(formFromRoutine(r, { a1: 'planner' })).toMatchObject({ kind: 'every', every: '30m', missed: 'skip', fresh: true, reportTo: 'agent:planner' });
    expect(hookUrl('http://127.0.0.1:9777', 'rt1')).toBe('http://127.0.0.1:9777/agents/routine-hook/rt1');
  });
});
