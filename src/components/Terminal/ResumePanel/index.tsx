import React from 'react';
import { Play, Wrench, Terminal, LoaderCircle } from 'lucide-react';

import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { miniMarkdown } from '~/usecase/util/miniMarkdown';
import { prettyModel } from '~/components/Terminal/AgentBar/parse';
import { claudeSessionSummary } from '~/adapter/claude/claude.client';

import type { SessionTurn, SessionSummary } from '~/domain/interfaces/claude.interface';

import styles from './styles.module.scss';

interface ResumePanelProps {
  sessionId: string;
  cwd?: string;
  active: boolean;
  onResume: () => void;
  onSkip: () => void;
}

const ago = (iso: string | null): string => {
  if (!iso) return '';
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return '';
  const mins = Math.max(0, Math.round((Date.now() - then) / 60000));
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
};

const countLabel = (n: number, partial: boolean): string => {
  const suffix = n === 1 && !partial ? 'prompt' : 'prompts';
  return `${n}${partial ? '+' : ''} ${suffix}`;
};

const toolLabel = (raw: string): string => {
  if (!raw.startsWith('mcp__')) return raw;
  const at = raw.lastIndexOf('__');
  return at > 0 ? raw.slice(at + 2) : raw;
};

const toolTally = (tools: string[]): { name: string; count: number }[] => {
  const order: string[] = [];
  const hits = new Map<string, number>();
  for (const raw of tools) {
    const name = toolLabel(raw);
    if (!hits.has(name)) order.push(name);
    hits.set(name, (hits.get(name) ?? 0) + 1);
  }
  return order.map((name) => ({ name, count: hits.get(name) ?? 1 }));
};

const Rendered = ({ text }: { text: string }) => (
  <>
    {miniMarkdown(text).map((block) => {
      if (block.kind === 'code') return <pre key={block.key} className={styles.code}>{block.body}</pre>;
      if (block.kind === 'bullet')
        return (
          <p key={block.key} className={styles.bullet}>
            {block.body}
          </p>
        );
      return (
        <p key={block.key} className={styles.line}>
          {block.body}
        </p>
      );
    })}
  </>
);

const TurnRow = React.memo(({ turn }: { turn: SessionTurn }) => {
  const user = turn.role === 'user';
  return (
    <div className={user ? styles.turnUser : styles.turnAgent}>
      <span className={styles.who}>{user ? 'You' : 'Claude'}</span>
      <div className={styles.bubble}>
        {turn.text && <Rendered text={turn.text} />}
        {turn.tools.length > 0 && (
          <div className={styles.tools}>
            {toolTally(turn.tools).map(({ name, count }) => (
              <span key={name} className={styles.tool}>
                <Wrench size={9} strokeWidth={2.25} />
                {name}
                {count > 1 && <span className={styles.toolCount}>{count}</span>}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
});

TurnRow.displayName = 'TurnRow';

const ResumePanel = ({ sessionId, cwd, active, onResume, onSkip }: ResumePanelProps) => {
  const [summary, setSummary] = React.useState<SessionSummary | null>(null);
  const [loading, setLoading] = React.useState(true);
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const resumeRef = React.useRef<HTMLButtonElement>(null);

  React.useEffect(() => {
    let alive = true;
    claudeSessionSummary(sessionId, cwd)
      .then((next) => {
        if (!alive) return;
        setSummary(next);
        setLoading(false);
      })
      .catch(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [sessionId, cwd]);

  React.useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [summary]);

  React.useEffect(() => {
    if (active && !loading) resumeRef.current?.focus({ preventScroll: true });
  }, [active, loading]);

  const meta = React.useMemo(() => {
    if (!summary) return [];
    const model = prettyModel(summary.model ?? undefined).model;
    return [summary.branch, ago(summary.endedAt), countLabel(summary.promptCount, summary.partial), model].filter(Boolean);
  }, [summary]);

  if (loading) {
    return (
      <div className={styles.panel}>
        <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
      </div>
    );
  }

  return (
    <div className={styles.panel}>
      <div className={styles.head}>
        <span className={styles.logo}>
          <ClaudeLogo />
        </span>
        <span className={styles.title}>Previous session</span>
        <span className={styles.id}>{sessionId.slice(0, 8)}</span>
      </div>

      {summary ? (
        <>
          {meta.length > 0 && (
            <div className={styles.meta}>
              {meta.map((entry) => (
                <span key={entry} className={styles.metaItem}>
                  {entry}
                </span>
              ))}
            </div>
          )}

          <div className={styles.scroll} ref={scrollRef} data-scroll={active ? 'on' : undefined}>
            {summary.partial && <div className={styles.older}>earlier turns not shown</div>}
            {summary.turns.map((turn, at) => (
              <TurnRow key={at} turn={turn} />
            ))}
          </div>
        </>
      ) : (
        <div className={styles.empty}>No transcript found for this session.</div>
      )}

      <div className={styles.actions}>
        <button type="button" ref={resumeRef} className={styles.primary} onClick={onResume}>
          <Play size={12} strokeWidth={2.25} />
          Resume session
        </button>
        <button type="button" className={styles.ghost} onClick={onSkip}>
          <Terminal size={12} strokeWidth={2.25} />
          Terminal only
        </button>
      </div>
    </div>
  );
};

export default ResumePanel;
