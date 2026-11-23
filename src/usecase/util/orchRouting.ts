import type { Dispatch, TaskWait, CandidateKey, DispatchTrigger } from '~/domain/interfaces/orch.interface';

const HOUR_MS = 3600000;
const MINUTE_MS = 60000;

export const DEFAULT_MAX_OPERATIONAL_ATTEMPTS = 6;

const TRIGGER_VERB: Record<DispatchTrigger, string> = {
  initial: 'Selected',
  fallback: 'Fallback to',
  escalation: 'Escalated to',
  resume: 'Resumed',
  manual: 'Pinned'
};

export const formatClock = (ms: number): string =>
  new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });

export const candidateLabel = (key: CandidateKey): string => [key.agent, key.model].filter(Boolean).join(':');

const resumeSuffix = (resumeAt: number | undefined, now: number): string => {
  if (resumeAt == null) return '';
  const remaining = resumeAt - now;
  if (remaining < HOUR_MS) return ` - resumes in ${Math.max(1, Math.ceil(remaining / MINUTE_MS))} min`;
  return ` - resumes ${formatClock(resumeAt)}`;
};

export const waitLabel = (wait: TaskWait, now: number): string => {
  if (wait.kind === 'intervention') return `Needs you: ${wait.reason}`;
  if (wait.kind === 'quota') return `Waiting for quota${resumeSuffix(wait.resumeAt, now)}`;
  return `Waiting: ${wait.reason}${resumeSuffix(wait.resumeAt, now)}`;
};

export const decisionLine = (dispatch: Dispatch): string | null => {
  const { decision, trigger } = dispatch;
  if (!decision || !trigger) return null;
  const model = candidateLabel(decision.effective);
  if (trigger === 'manual') return `Pinned ${model}`;
  if (trigger === 'resume') return `Resumed ${model}`;
  const reason = decision.reason.replace(/^(fallback|escalation): /, '').replace(/_/g, ' ');
  return `${TRIGGER_VERB[trigger]} ${model}: ${reason}`;
};
