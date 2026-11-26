import type { Routine, PersistentAgent } from '~/domain/interfaces/agents.interface';
import type { HistoryRow, RoutineForm, TriggerKind } from '~/usecase/util/agentRoutines';

import React from 'react';
import ReactDOM from 'react-dom';
import { X } from 'lucide-react';

import styles from './styles.module.scss';
import { showToast } from '~/usecase/util/toasts';
import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';
import { clampToViewport } from '~/usecase/util/agentMessages';
import { useAgents, agentActions } from '~/usecase/hooks/useAgents';
import { hookUrl, emptyForm, formToBody, historyRows, outcomeChip, validateForm, formFromRoutine } from '~/usecase/util/agentRoutines';
import { runRoutine, routineRuns, listRoutines, createRoutine, deleteRoutine, updateRoutine, setRoutineEnabled, rotateRoutineToken } from '~/adapter/agents/agents.client';

interface AgentRoutinesProps {
  agent: PersistentAgent;
  anchorRect: DOMRect;
  onClose: () => void;
}

const POPOVER_SIZE = { width: 380, height: 560 };
const DAYS = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
const KINDS: { value: TriggerKind; label: string }[] = [
  { value: 'daily', label: 'Daily' },
  { value: 'weekdays', label: 'Weekdays' },
  { value: 'weekly', label: 'Weekly days' },
  { value: 'every', label: 'Every' },
  { value: 'cron', label: 'Cron' },
  { value: 'hook', label: 'Webhook' },
  { value: 'manual', label: 'Manual' }
];

const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const formatTime = (at: number): string =>
  new Date(at).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });

const AgentRoutines = ({ agent, anchorRect, onClose }: AgentRoutinesProps) => {
  const { agents } = useAgents();
  const [routines, setRoutines] = React.useState<Routine[]>([]);
  const [expanded, setExpanded] = React.useState<string | null>(null);
  const [history, setHistory] = React.useState<HistoryRow[]>([]);
  const [editing, setEditing] = React.useState<{ id: string | null; form: RoutineForm } | null>(null);
  const [formError, setFormError] = React.useState<string | null>(null);
  const [token, setToken] = React.useState<{ id: string; value: string } | null>(null);
  const [confirmDelete, setConfirmDelete] = React.useState<string | null>(null);
  const popRef = React.useRef<HTMLDivElement>(null);
  const names = React.useMemo(() => Object.fromEntries(agents.map((a) => [a.id, a.name])), [agents]);
  const summaryKey = JSON.stringify(agent.routines ?? null);

  const load = React.useCallback(() => {
    listRoutines(agent.name)
      .then((r) => setRoutines(r.routines))
      .catch((err: unknown) => showToast({ kind: 'error', message: errorText(err) }));
  }, [agent.name]);

  React.useEffect(() => {
    load();
  }, [load, summaryKey]);

  React.useEffect(() => {
    if (!expanded) return;
    routineRuns(expanded, 10)
      .then((r) => setHistory(historyRows(r.runs, r.skips)))
      .catch(() => setHistory([]));
  }, [expanded, summaryKey]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const onOutside = (e: PointerEvent) => {
      if (!popRef.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('pointerdown', onOutside, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('pointerdown', onOutside, true);
    };
  }, [onClose]);

  const refresh = () => {
    load();
    agentActions.refresh();
  };

  const handleToggle = (r: Routine) => {
    setRoutineEnabled(r.id, !(r.enabled && !r.pausedReason)).then(refresh).catch((err: unknown) => showToast({ kind: 'error', message: errorText(err) }));
  };

  const handleRunNow = (r: Routine) => {
    runRoutine(r.id)
      .then((res) => showToast({ kind: 'info', message: res.runId ? `Started run ${res.runId}` : `Skipped: ${res.skipped}` }))
      .then(refresh)
      .catch((err: unknown) => showToast({ kind: 'error', message: errorText(err) }));
  };

  const handleDelete = (id: string) => {
    setConfirmDelete(null);
    deleteRoutine(id).then(refresh).catch((err: unknown) => showToast({ kind: 'error', message: errorText(err) }));
  };

  const handleRotate = (id: string) => {
    rotateRoutineToken(id).then((r) => setToken({ id, value: r.token })).catch((err: unknown) => showToast({ kind: 'error', message: errorText(err) }));
  };

  const handleCopy = (text: string) => {
    void navigator.clipboard?.writeText(text);
  };

  const patchForm = (patch: Partial<RoutineForm>) => setEditing((prev) => (prev ? { ...prev, form: { ...prev.form, ...patch } } : prev));

  const toggleDay = (day: string) =>
    setEditing((prev) => {
      if (!prev) return prev;
      const days = prev.form.days.includes(day) ? prev.form.days.filter((d) => d !== day) : [...prev.form.days, day];
      return { ...prev, form: { ...prev.form, days } };
    });

  const handleSave = () => {
    if (!editing) return;
    const problem = validateForm(editing.form);
    if (problem) {
      setFormError(problem);
      return;
    }
    setFormError(null);
    const body = formToBody(editing.form);
    const request = editing.id
      ? updateRoutine({ ...body, id: editing.id })
      : createRoutine({ ...body, agent: agent.name });
    request
      .then((res) => {
        const created = res as { routine: Routine; token?: string };
        if (created.token) setToken({ id: created.routine.id, value: created.token });
        setEditing(null);
        refresh();
      })
      .catch((err: unknown) => setFormError(errorText(err)));
  };

  const style: React.CSSProperties = clampToViewport(anchorRect.right + 8, anchorRect.top, POPOVER_SIZE, {
    width: window.innerWidth,
    height: window.innerHeight
  });

  const form = editing?.form;

  return ReactDOM.createPortal(
    <div ref={popRef} className={styles.popover} style={style}>
      <div className={styles.header}>
        <span className={styles.title}>{agent.name} routines</span>
        <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>

      {token && (
        <div className={styles.tokenPane}>
          <span className={styles.meta}>Webhook (local only). The token is shown once.</span>
          <code className={styles.code}>{hookUrl(SIDECAR_HTTP, token.id)}</code>
          <code className={styles.code}>X-Panorama-Routine-Token: {token.value}</code>
          <div className={styles.actions}>
            <button type="button" className={styles.btn} onClick={() => handleCopy(token.value)}>Copy token</button>
            <button type="button" className={styles.btn} onClick={() => setToken(null)}>Done</button>
          </div>
        </div>
      )}

      {form ? (
        <div className={styles.form}>
          <label className={styles.field}>
            Name
            <input className={styles.input} value={form.name} maxLength={60} onChange={(e) => patchForm({ name: e.target.value })} />
          </label>
          <label className={styles.field}>
            Instruction
            <textarea className={styles.textarea} rows={4} value={form.text} onChange={(e) => patchForm({ text: e.target.value })} />
          </label>
          <label className={styles.field}>
            When
            <select className={styles.input} value={form.kind} onChange={(e) => patchForm({ kind: e.target.value as TriggerKind })}>
              {KINDS.map((k) => <option key={k.value} value={k.value}>{k.label}</option>)}
            </select>
          </label>
          {['daily', 'weekdays', 'weekly'].includes(form.kind) && (
            <label className={styles.field}>
              Time (local)
              <input className={styles.input} value={form.time} placeholder="09:00" onChange={(e) => patchForm({ time: e.target.value })} />
            </label>
          )}
          {form.kind === 'weekly' && (
            <div className={styles.days}>
              {DAYS.map((d) => (
                <label key={d} className={styles.check}>
                  <input type="checkbox" checked={form.days.includes(d)} onChange={() => toggleDay(d)} />
                  {d}
                </label>
              ))}
            </div>
          )}
          {form.kind === 'every' && (
            <label className={styles.field}>
              Every
              <input className={styles.input} value={form.every} placeholder="30m" onChange={(e) => patchForm({ every: e.target.value })} />
            </label>
          )}
          {form.kind === 'cron' && (
            <label className={styles.field}>
              Cron (local time)
              <input className={styles.input} value={form.cron} placeholder="30 8 * * 1-5" onChange={(e) => patchForm({ cron: e.target.value })} />
            </label>
          )}
          <div className={styles.row2}>
            <label className={styles.field}>
              Missed runs
              <select className={styles.input} value={form.missed} onChange={(e) => patchForm({ missed: e.target.value as RoutineForm['missed'] })}>
                <option value="catch-up">One catch-up</option>
                <option value="skip">Skip</option>
              </select>
            </label>
            <label className={styles.field}>
              If still running
              <select className={styles.input} value={form.overlap} onChange={(e) => patchForm({ overlap: e.target.value as RoutineForm['overlap'] })}>
                <option value="skip">Skip</option>
                <option value="queue">Queue one</option>
              </select>
            </label>
          </div>
          <div className={styles.row2}>
            <label className={styles.field}>
              Report to
              <select className={styles.input} value={form.reportTo} onChange={(e) => patchForm({ reportTo: e.target.value })}>
                <option value="user">Messages</option>
                <option value="none">History only</option>
                {agents.filter((a) => a.id !== agent.id).map((a) => <option key={a.id} value={`agent:${a.name}`}>{a.name}</option>)}
              </select>
            </label>
            <label className={styles.field}>
              Max per day
              <input className={styles.input} type="number" min={1} max={288} value={form.maxPerDay} onChange={(e) => patchForm({ maxPerDay: Number(e.target.value) })} />
            </label>
          </div>
          <label className={styles.field}>
            Session
            <select className={styles.input} value={form.session} onChange={(e) => patchForm({ session: e.target.value as RoutineForm['session'] })}>
              <option value="keep">Keep the conversation</option>
              <option value="fresh">Fresh session for each run</option>
              <option value="rollover">Renew with a handoff for each run</option>
            </select>
          </label>
          {form.session === 'rollover' && (
            <label className={styles.check}>
              <input type="checkbox" checked={form.noNote} onChange={(e) => patchForm({ noNote: e.target.checked })} />
              Skip the handoff note (state only)
            </label>
          )}
          {form.session === 'fresh' && <span className={styles.warning}>Each run drops this agent&apos;s conversation. Use a dedicated agent.</span>}
          {formError && <span className={styles.error}>{formError}</span>}
          <div className={styles.actions}>
            <button type="button" className={styles.btn} onClick={() => setEditing(null)}>Cancel</button>
            <button type="button" className={styles.primary} onClick={handleSave}>Save</button>
          </div>
        </div>
      ) : (
        <div className={styles.list}>
          {routines.length === 0 && <div className={styles.empty}>No routines yet</div>}
          {routines.map((r) => {
            const chip = r.last?.outcome ? outcomeChip(r.last.outcome) : outcomeChip(null);
            const on = r.enabled && !r.pausedReason;
            return (
              <div key={r.id} className={styles.item}>
                <div className={styles.itemTop}>
                  <input type="checkbox" checked={on} aria-label={on ? 'Disable' : 'Enable'} onChange={() => handleToggle(r)} />
                  <button type="button" className={styles.itemName} aria-expanded={expanded === r.id} onClick={() => setExpanded(expanded === r.id ? null : r.id)}>
                    {r.name}
                  </button>
                  <span className={styles.chip} data-kind={chip.kind}>{chip.label}</span>
                </div>
                <span className={styles.meta}>
                  {r.triggerText}
                  {r.pausedReason ? ` - paused (${r.pausedReason})` : r.nextRunLocal ? ` - next ${r.nextRunLocal}` : ''}
                </span>
                <div className={styles.actions}>
                  <button type="button" className={styles.btn} onClick={() => handleRunNow(r)}>Run now</button>
                  <button type="button" className={styles.btn} onClick={() => setEditing({ id: r.id, form: formFromRoutine(r, names) })}>Edit</button>
                  {r.trigger.kind === 'hook' && <button type="button" className={styles.btn} onClick={() => handleRotate(r.id)}>Rotate token</button>}
                  {confirmDelete === r.id ? (
                    <button type="button" className={styles.danger} onClick={() => handleDelete(r.id)}>Confirm delete</button>
                  ) : (
                    <button type="button" className={styles.btn} onClick={() => setConfirmDelete(r.id)}>Delete</button>
                  )}
                </div>
                {expanded === r.id && (
                  <div className={styles.history}>
                    {history.length === 0 && <span className={styles.meta}>No runs yet</span>}
                    {history.map((h) => (
                      <div key={h.key} className={styles.historyRow}>
                        <span className={styles.chip} data-kind={h.chip.kind}>{h.chip.label}</span>
                        <span className={styles.meta}>{formatTime(h.at)}</span>
                        <span className={styles.historyText}>{h.text}</span>
                        {h.cost && <span className={styles.meta}>{h.cost}</span>}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
          <div className={styles.footer}>
            <button type="button" className={styles.primary} onClick={() => setEditing({ id: null, form: emptyForm() })}>New routine</button>
          </div>
        </div>
      )}
    </div>,
    document.body,
  );
};

export default AgentRoutines;
