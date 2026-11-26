import type { AgentMessage, PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import ReactDOM from 'react-dom';
import { X, ArrowDown, ArrowUp, Clock } from 'lucide-react';

import styles from './styles.module.scss';
import { showToast } from '~/usecase/util/toasts';
import { isRolloverMessage } from '~/usecase/util/agentMemory';
import { useAgentMessages } from '~/usecase/hooks/useAgentMessages';
import { readMessage, sendMessage } from '~/adapter/agents/agents.client';
import { messageDirection, messagePreview, statusChipKind, clampToViewport } from '~/usecase/util/agentMessages';

interface AgentMessagesProps {
  agent: PersistentAgent;
  anchorRect: DOMRect;
  onClose: () => void;
}

const formatTime = (at: number): string => {
  const d = new Date(at);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

const AgentMessages = ({ agent, anchorRect, onClose }: AgentMessagesProps) => {
  const { messages } = useAgentMessages(agent.id);
  const [expanded, setExpanded] = React.useState<string | null>(null);
  const [fullTexts, setFullTexts] = React.useState<Record<string, string>>({});
  const [text, setText] = React.useState('');
  const [expectsReply, setExpectsReply] = React.useState(false);
  const [sending, setSending] = React.useState(false);
  const popRef = React.useRef<HTMLDivElement>(null);

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

  const handleExpandRow = (msg: AgentMessage) => {
    if (expanded === msg.id) {
      setExpanded(null);
      return;
    }
    setExpanded(msg.id);
    if (msg.truncated && !fullTexts[msg.id]) {
      readMessage(msg.id)
        .then((r) => setFullTexts((prev) => ({ ...prev, [msg.id]: r.message.text })))
        .catch(() => undefined);
    }
  };

  const handleSend = () => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    sendMessage({ to: agent.name, text: trimmed, expectsReply })
      .then((r) => {
        showToast({ kind: 'info', message: r.text });
        setText('');
        setExpectsReply(false);
      })
      .catch((err: unknown) => {
        showToast({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      })
      .finally(() => setSending(false));
  };

  const handleTextKey = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleExpectsChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setExpectsReply(e.target.checked);
  };

  const handleTextChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setText(e.target.value);
  };

  const style: React.CSSProperties = clampToViewport(
    anchorRect.right + 8,
    anchorRect.top,
    { width: 360, height: 520 },
    { width: window.innerWidth, height: window.innerHeight }
  );

  const handleRowKey = (e: React.KeyboardEvent, msg: AgentMessage) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleExpandRow(msg);
    }
  };

  return ReactDOM.createPortal(
    <div ref={popRef} className={styles.popover} style={style}>
      <div className={styles.header}>
        <span className={styles.title}>{agent.name}</span>
        <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.list}>
        {messages.length === 0 && <div className={styles.empty}>No messages</div>}
        {messages.map((msg) => {
          const dir = messageDirection(msg, agent.id);
          const chip = isRolloverMessage(msg) ? 'rollover' : statusChipKind(msg);
          const isExpanded = expanded === msg.id;
          const displayText = isExpanded ? (fullTexts[msg.id] ?? msg.text) : messagePreview(msg);
          const peer = dir === 'in' ? msg.fromLabel : msg.toLabel;
          const isAnswered = msg.repliedBy !== null;
          const rowCls = [styles.msgRow, msg.notice && styles.muted].filter(Boolean).join(' ');

          return (
            <div
                key={msg.id}
                className={rowCls}
                role="button"
                tabIndex={0}
                onClick={() => handleExpandRow(msg)}
                onKeyDown={(e) => handleRowKey(e, msg)}
              >
              <div className={styles.msgMeta}>
                <span className={styles.arrow}>
                  {dir === 'in' ? <ArrowDown size={10} strokeWidth={2.5} /> : <ArrowUp size={10} strokeWidth={2.5} />}
                </span>
                <span className={styles.peer}>{peer}</span>
                <span className={styles.chip} data-chip={chip}>{chip}</span>
                {msg.expectsReply && !isAnswered && (
                  <span className={styles.replyExpected}>
                    <Clock size={9} strokeWidth={2} />
                  </span>
                )}
                {isAnswered && <span className={styles.answered}>answered</span>}
                <span className={styles.time}>{formatTime(msg.at)}</span>
              </div>
              <div className={styles.msgText}>{displayText}</div>
              {isExpanded && msg.reason && <div className={styles.reason}>{msg.reason}</div>}
            </div>
          );
        })}
      </div>
      <div className={styles.composer}>
        <textarea
          className={styles.textarea}
          value={text}
          rows={3}
          placeholder={`Message ${agent.name}...`}
          onChange={handleTextChange}
          onKeyDown={handleTextKey}
        />
        <div className={styles.composerFooter}>
          <label className={styles.checkLabel}>
            <input type="checkbox" checked={expectsReply} onChange={handleExpectsChange} />
            Expect reply
          </label>
          <button
            type="button"
            className={styles.sendBtn}
            disabled={!text.trim() || sending}
            onClick={handleSend}
          >
            Send
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
};

export default AgentMessages;
