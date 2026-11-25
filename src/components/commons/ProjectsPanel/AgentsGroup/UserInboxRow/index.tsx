import type { AgentMessage } from '~/domain/interfaces/agents.interface';

import React from 'react';
import ReactDOM from 'react-dom';
import { X, Inbox, ArrowDown, ArrowUp } from 'lucide-react';

import styles from './styles.module.scss';
import { showToast } from '~/usecase/util/toasts';
import { markAllRead, replyMessage } from '~/adapter/agents/agents.client';
import { useAgentMessages, refreshMessages } from '~/usecase/hooks/useAgentMessages';
import { messageDirection, messagePreview, statusChipKind, clampToViewport } from '~/usecase/util/agentMessages';

const PARTY = 'user';
const POPOVER_SIZE = { width: 360, height: 480 };

const formatTime = (at: number): string => {
  const d = new Date(at);
  return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
};

interface InboxPopoverProps {
  messages: AgentMessage[];
  anchorRect: DOMRect;
  onClose: () => void;
}

const InboxPopover = ({ messages, anchorRect, onClose }: InboxPopoverProps) => {
  const [expanded, setExpanded] = React.useState<string | null>(null);
  const [replyTexts, setReplyTexts] = React.useState<Record<string, string>>({});
  const [replying, setReplying] = React.useState<string | null>(null);
  const popRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const hasUnread = messages.some((m) => m.to.kind === 'user' && !m.read);
    if (hasUnread) {
      markAllRead()
        .then(() => refreshMessages(PARTY))
        .catch(() => undefined);
    }
  }, [messages]);

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

  const handleExpandRow = (msgId: string) => {
    setExpanded((prev) => (prev === msgId ? null : msgId));
  };

  const handleReplyChange = (msgId: string, value: string) => {
    setReplyTexts((prev) => ({ ...prev, [msgId]: value }));
  };

  const handleSendReply = (msg: AgentMessage) => {
    const text = replyTexts[msg.id]?.trim() ?? '';
    if (!text || replying) return;
    setReplying(msg.id);
    replyMessage(msg.id, text)
      .then((r) => {
        showToast({ kind: 'info', message: r.text });
        setReplyTexts((prev) => ({ ...prev, [msg.id]: '' }));
        refreshMessages(PARTY);
      })
      .catch((err: unknown) => {
        showToast({ kind: 'error', message: err instanceof Error ? err.message : String(err) });
      })
      .finally(() => setReplying(null));
  };

  const handleReplyKey = (msg: AgentMessage, e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      handleSendReply(msg);
    }
  };

  const style: React.CSSProperties = clampToViewport(
    anchorRect.left,
    anchorRect.bottom + 4,
    POPOVER_SIZE,
    { width: window.innerWidth, height: window.innerHeight }
  );

  const handleRowKey = (e: React.KeyboardEvent, msgId: string) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleExpandRow(msgId);
    }
  };

  return ReactDOM.createPortal(
    <div ref={popRef} className={styles.popover} style={style}>
      <div className={styles.header}>
        <span className={styles.title}>Messages</span>
        <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.list}>
        {messages.length === 0 && <div className={styles.empty}>No messages</div>}
        {messages.map((msg) => {
          const dir = messageDirection(msg, PARTY);
          const chip = statusChipKind(msg);
          const isExpanded = expanded === msg.id;
          const peer = dir === 'in' ? msg.fromLabel : msg.toLabel;
          const isAnswered = msg.repliedBy !== null;
          const canReply = msg.expectsReply && !isAnswered && dir === 'in';
          const rowCls = [styles.msgRow, msg.notice && styles.muted].filter(Boolean).join(' ');
          const replyText = replyTexts[msg.id] ?? '';

          return (
            <div key={msg.id} className={rowCls}>
              <div
                className={styles.msgTop}
                role="button"
                tabIndex={0}
                aria-expanded={isExpanded}
                onClick={() => handleExpandRow(msg.id)}
                onKeyDown={(e) => handleRowKey(e, msg.id)}
              >
                <div className={styles.msgMeta}>
                  <span className={styles.arrow}>
                    {dir === 'in' ? <ArrowDown size={10} strokeWidth={2.5} /> : <ArrowUp size={10} strokeWidth={2.5} />}
                  </span>
                  <span className={styles.peer}>{peer}</span>
                  <span className={styles.chip} data-chip={chip}>{chip}</span>
                  {msg.expectsReply && !isAnswered && <span className={styles.replyExpected}>reply expected</span>}
                  {isAnswered && <span className={styles.answered}>answered</span>}
                  <span className={styles.time}>{formatTime(msg.at)}</span>
                </div>
                <div className={styles.msgText}>{messagePreview(msg)}</div>
              </div>
              {isExpanded && (
                <div className={styles.expanded}>
                  <div className={styles.fullText}>{msg.text}</div>
                  {msg.reason && <div className={styles.reason}>{msg.reason}</div>}
                  {canReply && (
                    <div className={styles.replyCompose}>
                      <textarea
                        className={styles.textarea}
                        value={replyText}
                        rows={2}
                        placeholder="Your reply..."
                        onChange={(e) => handleReplyChange(msg.id, e.target.value)}
                        onKeyDown={(e) => handleReplyKey(msg, e)}
                      />
                      <button
                        type="button"
                        className={styles.replyBtn}
                        disabled={!replyText.trim() || replying === msg.id}
                        onClick={() => handleSendReply(msg)}
                      >
                        Send reply
                      </button>
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>,
    document.body,
  );
};

const UserInboxRow = () => {
  const { messages, userUnread } = useAgentMessages(PARTY);
  const [open, setOpen] = React.useState(false);
  const [anchorRect, setAnchorRect] = React.useState<DOMRect | null>(null);
  const rowRef = React.useRef<HTMLDivElement>(null);

  const handleOpen = () => {
    if (rowRef.current) setAnchorRect(rowRef.current.getBoundingClientRect());
    setOpen(true);
  };

  const handleClose = () => setOpen(false);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleOpen();
    }
  };

  return (
    <>
      <div
        ref={rowRef}
        className={styles.row}
        role="button"
        tabIndex={0}
        onClick={handleOpen}
        onKeyDown={handleKeyDown}
      >
        <Inbox size={11} strokeWidth={2} className={styles.icon} />
        <span className={styles.label}>Messages</span>
        {userUnread > 0 && <span className={styles.badge}>{userUnread}</span>}
      </div>
      {open && anchorRect && (
        <InboxPopover messages={messages} anchorRect={anchorRect} onClose={handleClose} />
      )}
    </>
  );
};

export default UserInboxRow;
