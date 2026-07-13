import React from 'react';
import { Brain, Sparkles, ChevronDown } from 'lucide-react';

import Suggest from './Suggest';
import Magnifier from './Magnifier';
import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { writeTempImage } from '~/adapter/clipboard/clipboard.client';
import { readFooter, modeKey, prettyMode, prettyModel, looksLikeClaude, detectExitBanner, parseStatusLines, detectSuggestTrigger } from './parse';
import { BPM_END, draftKey, BPM_START, HISTORY_KEY, EFFORT_LEVELS, CLAUDE_MODELS, CLAUDE_SLASH_COMMANDS, MODEL_QUICK_SWITCHES } from './constants';
import { cloneDraft, removeChip, EMPTY_DRAFT, partsToDraft, draftToParts, isDraftEmpty, renderEditor, replaceEditor, getCaretOffset, setCaretOffset, serializeEditor, placeCaretAtEnd, consolidateParts, draftToSendParts, insertPartsAtCaret, isCaretOnLastLine, isCaretOnFirstLine } from './editor';

import type { ClaudeState } from '~/domain/interfaces/pty.interface';
import type { ContentPart, ParsedStatus, SuggestTrigger, AgentBarProps, PromptSuggestion, AgentSuggestHandle } from './types';

import styles from './styles.module.scss';

const MODE_CLASS: Record<string, string> = {
  plan: styles.modePlan,
  auto: styles.modeAuto,
  bypass: styles.modeBypass,
  accept: styles.modeAccept,
  default: ''
};

const loadHistory = (): ContentPart[][] => {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    const arr = raw ? JSON.parse(raw) : [];
    return Array.isArray(arr) ? arr.filter((e) => Array.isArray(e)) : [];
  } catch {
    return [];
  }
};

const historyKey = (draft: { text: string }): string => draft.text.trim();

const pause = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

const progressColor = (p: number): string =>
  p >= 75 ? '#ef4444' : p >= 60 ? '#f97316' : p >= 30 ? '#eab308' : '#22c55e';

const formatCost = (v: number): string => {
  if (v > 0 && v < 0.01) return '<$0.01';
  if (v >= 1000) return `$${(v / 1000).toFixed(1)}k`;
  return `$${v.toFixed(2)}`;
};

const matchQuickSwitchId = (raw: string | undefined): string => {
  if (!raw) return '';
  const m = raw.toLowerCase();
  return MODEL_QUICK_SWITCHES.find((s) => s.id === m)?.id ?? '';
};

type UndoSnap = { text: string; images: string[]; caret: number };

const UNDO_CAP = 200;

const sameSnap = (a: UndoSnap, b: UndoSnap): boolean =>
  a.text === b.text && a.images.length === b.images.length && a.images.every((p, i) => p === b.images[i]);

const clipboardImages = (e: React.ClipboardEvent): Blob[] => {
  const out: Blob[] = [];
  const items = e.clipboardData?.items;
  if (!items) return out;
  for (const item of Array.from(items)) {
    if (item.kind === 'file' && item.type.startsWith('image/')) {
      const file = item.getAsFile();
      if (file) out.push(file);
    }
  }
  return out;
};

const AgentBar = ({ tileId, active, send, getLines, getStructured, focusTerminal, onClaudeActive }: AgentBarProps) => {
  const [claudeActive, setClaudeActive] = React.useState(false);
  const [draft, setDraft] = React.useState(EMPTY_DRAFT);
  const [history, setHistory] = React.useState<ContentPart[][]>(() => loadHistory());
  const [suggest, setSuggest] = React.useState<SuggestTrigger>(null);
  const [status, setStatus] = React.useState<ParsedStatus>({});
  const [scraped, setScraped] = React.useState<{ model: string; contextInfo?: string } | null>(null);
  const [structured, setStructured] = React.useState<ClaudeState | null>(null);
  const [questionMode, setQuestionMode] = React.useState(false);
  const [manualHide, setManualHide] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  const [modelMenu, setModelMenu] = React.useState(false);
  const [effortMenu, setEffortMenu] = React.useState(false);
  const [preview, setPreview] = React.useState<string | null>(null);

  const editorRef = React.useRef<HTMLDivElement>(null);
  const modelRef = React.useRef<HTMLDivElement>(null);
  const effortRef = React.useRef<HTMLDivElement>(null);
  const suggestRef = React.useRef<AgentSuggestHandle>(null);
  const historyRef = React.useRef<ContentPart[][]>(history);
  const histIdxRef = React.useRef<number | null>(null);
  const histDraftRef = React.useRef(cloneDraft(EMPTY_DRAFT));
  const lastSentRef = React.useRef(cloneDraft(EMPTY_DRAFT));
  const draftRef = React.useRef(cloneDraft(EMPTY_DRAFT));
  const silentRef = React.useRef(false);
  const undoRef = React.useRef<UndoSnap[]>([]);
  const redoRef = React.useRef<UndoSnap[]>([]);
  const prevSnapRef = React.useRef<UndoSnap>({ text: '', images: [], caret: 0 });
  const lastInputTypeRef = React.useRef('');
  const imgSeqRef = React.useRef(0);
  const submitSeqRef = React.useRef(0);
  const seenRef = React.useRef(false);
  const lastSeenRef = React.useRef(0);
  const barOpenRef = React.useRef<boolean | null>(null);
  const activeRef = React.useRef(active);
  historyRef.current = history;
  draftRef.current = draft;
  activeRef.current = active;

  const isEmpty = isDraftEmpty(draft);
  const hidden = questionMode || manualHide;

  const liveSnap = (): UndoSnap => {
    const d = draftRef.current;
    const el = editorRef.current;
    const caret = el ? (getCaretOffset(el) ?? d.text.length) : d.text.length;
    return { text: d.text, images: [...d.images], caret };
  };

  const checkpoint = () => {
    const prev = prevSnapRef.current;
    const stack = undoRef.current;
    const top = stack[stack.length - 1];
    if (top && sameSnap(top, prev)) return;
    stack.push(prev);
    if (stack.length > UNDO_CAP) stack.shift();
  };

  const syncFromEditor = () => {
    const el = editorRef.current;
    if (!el) return;
    const next = serializeEditor(el);
    setDraft(next);
    draftRef.current = next;
    prevSnapRef.current = liveSnap();
    lastInputTypeRef.current = '';
  };

  const renderAndFocus = (next: { text: string; images: string[] }, focus = true) => {
    const el = editorRef.current;
    if (!el) return;
    renderEditor(el, next);
    if (focus) {
      el.focus({ preventScroll: true });
      placeCaretAtEnd(el);
    }
    draftRef.current = next;
  };

  const applyDraft = (next: { text: string; images: string[] }) => {
    const el = editorRef.current;
    if (!el) return;
    silentRef.current = true;
    replaceEditor(el, next);
    silentRef.current = false;
    draftRef.current = next;
    prevSnapRef.current = { text: next.text, images: [...next.images], caret: next.text.length };
    lastInputTypeRef.current = '';
  };

  const commitDraft = (next: { text: string; images: string[] }) => {
    checkpoint();
    redoRef.current = [];
    applyDraft(next);
  };

  const restoreSnap = (snap: UndoSnap) => {
    const el = editorRef.current;
    if (!el) return;
    const next = { text: snap.text, images: [...snap.images] };
    setDraft(next);
    applyDraft(next);
    setCaretOffset(el, snap.caret);
    prevSnapRef.current = snap;
    histIdxRef.current = null;
    setSuggest(null);
  };

  const doUndo = () => {
    const live = liveSnap();
    const stack = undoRef.current;
    let snap = stack.pop();
    while (snap && sameSnap(snap, live)) snap = stack.pop();
    if (!snap) return;
    redoRef.current.push(live);
    restoreSnap(snap);
  };

  const doRedo = () => {
    const snap = redoRef.current.pop();
    if (!snap) return;
    undoRef.current.push(liveSnap());
    restoreSnap(snap);
  };

  const undoFnRef = React.useRef({ undo: doUndo, redo: doRedo });
  undoFnRef.current = { undo: doUndo, redo: doRedo };

  React.useEffect(() => {
    const SCAN_MS = 350;
    const GONE_MS = 2500;
    let qMode = false;
    let qTarget: boolean | null = null;
    let qTimer: ReturnType<typeof setTimeout> | undefined;

    const scan = () => {
      const lines = getLines();
      const present = looksLikeClaude(lines.slice(-25).join('\n'));
      const now = Date.now();
      if (present) {
        lastSeenRef.current = now;
        if (!seenRef.current) {
          seenRef.current = true;
          setClaudeActive(true);
        }
      } else if (seenRef.current && now - lastSeenRef.current > GONE_MS) {
        seenRef.current = false;
        setClaudeActive(false);
      }
      if (!seenRef.current) return;

      const footer = readFooter(lines);
      setStatus(parseStatusLines(footer.status));
      if (footer.model) setScraped(footer.model);
      setStructured(getStructured());

      const qm = footer.questionMode;
      if (qm === qMode) {
        if (qTimer) {
          clearTimeout(qTimer);
          qTimer = undefined;
          qTarget = null;
        }
      } else if (qTarget !== qm) {
        if (qTimer) clearTimeout(qTimer);
        qTarget = qm;
        qTimer = setTimeout(() => {
          qMode = qm;
          qTimer = undefined;
          qTarget = null;
          setQuestionMode(qm);
        }, 150);
      }
    };

    scan();
    const id = window.setInterval(scan, SCAN_MS);
    return () => {
      clearInterval(id);
      if (qTimer) clearTimeout(qTimer);
    };
  }, [getLines, getStructured]);

  const onClaudeActiveRef = React.useRef(onClaudeActive);
  onClaudeActiveRef.current = onClaudeActive;

  React.useEffect(() => {
    onClaudeActiveRef.current?.(claudeActive);
  }, [claudeActive]);

  React.useEffect(() => {
    if (claudeActive) {
      let restored = cloneDraft(EMPTY_DRAFT);
      try {
        const raw = localStorage.getItem(draftKey(tileId));
        const parsed = raw ? JSON.parse(raw) : null;
        if (parsed && typeof parsed.text === 'string') {
          restored = { text: parsed.text, images: Array.isArray(parsed.images) ? parsed.images : [] };
        }
      } catch {
        restored = cloneDraft(EMPTY_DRAFT);
      }
      setDraft(restored);
      setSuggest(null);
      histIdxRef.current = null;
      histDraftRef.current = cloneDraft(EMPTY_DRAFT);
      undoRef.current = [];
      redoRef.current = [];
      prevSnapRef.current = { text: restored.text, images: [...restored.images], caret: restored.text.length };
      lastInputTypeRef.current = '';
      renderAndFocus(restored, activeRef.current && (restored.text.length > 0 || restored.images.length > 0));
    } else {
      setStatus({});
      setScraped(null);
      setStructured(null);
      setQuestionMode(false);
      setSubmitting(false);
      setManualHide(false);
    }
  }, [claudeActive, tileId]);

  React.useEffect(() => {
    if (!claudeActive) return;
    const t = setTimeout(() => {
      try {
        if (isDraftEmpty(draft)) localStorage.removeItem(draftKey(tileId));
        else localStorage.setItem(draftKey(tileId), JSON.stringify({ text: draft.text, images: draft.images }));
      } catch {
        void 0;
      }
    }, 500);
    return () => clearTimeout(t);
  }, [draft, claudeActive, tileId]);

  React.useEffect(() => {
    if (!active) {
      barOpenRef.current = null;
      return;
    }
    const barOpen = claudeActive && !hidden;
    if (barOpenRef.current === barOpen) return;
    barOpenRef.current = barOpen;
    const el = editorRef.current;
    if (barOpen && el) {
      el.focus({ preventScroll: true });
      placeCaretAtEnd(el);
      return;
    }
    focusTerminal();
  }, [active, claudeActive, hidden, focusTerminal]);

  React.useEffect(() => {
    if (!claudeActive || !active) return;
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.shiftKey || e.metaKey || e.altKey) return;
      if (e.key !== 'g' && e.key !== 'G') return;
      e.preventDefault();
      e.stopPropagation();
      setManualHide((prev) => !prev);
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [claudeActive, active]);

  React.useEffect(() => {
    if (!claudeActive || !active || hidden) return;
    const onTab = (e: KeyboardEvent) => {
      if (e.key !== 'Tab' || e.shiftKey || e.ctrlKey || e.metaKey || e.altKey) return;
      if (!document.hasFocus() || document.activeElement === editorRef.current) return;
      e.preventDefault();
      e.stopPropagation();
      const el = editorRef.current;
      if (el) {
        el.focus({ preventScroll: true });
        placeCaretAtEnd(el);
      }
    };
    window.addEventListener('keydown', onTab, true);
    return () => window.removeEventListener('keydown', onTab, true);
  }, [claudeActive, active, hidden]);

  React.useEffect(() => {
    if (!claudeActive) return;
    const el = editorRef.current;
    if (!el) return;
    const onBeforeInput = (e: Event) => {
      const t = (e as InputEvent).inputType;
      if (t === 'historyUndo') {
        e.preventDefault();
        undoFnRef.current.undo();
      } else if (t === 'historyRedo') {
        e.preventDefault();
        undoFnRef.current.redo();
      }
    };
    el.addEventListener('beforeinput', onBeforeInput);
    return () => el.removeEventListener('beforeinput', onBeforeInput);
  }, [claudeActive]);

  React.useEffect(() => {
    if (!modelMenu && !effortMenu) return;
    const onDown = (e: MouseEvent) => {
      if (!modelRef.current?.contains(e.target as Node)) setModelMenu(false);
      if (!effortRef.current?.contains(e.target as Node)) setEffortMenu(false);
    };
    window.addEventListener('mousedown', onDown, true);
    return () => window.removeEventListener('mousedown', onDown, true);
  }, [modelMenu, effortMenu]);

  const persistHistory = (next: { text: string; images: string[] }) => {
    const key = historyKey(next);
    if (!key && next.images.length === 0) return;
    const parts = draftToParts(next);
    const current = loadHistory();
    const updated = [...current.filter((e) => historyKey(partsToDraft(e)) !== key), parts].slice(-50);
    try {
      localStorage.setItem(HISTORY_KEY, JSON.stringify(updated));
    } catch {
      void 0;
    }
    setHistory(updated);
  };

  const sendDraft = async (submission: { text: string; images: string[] }) => {
    const seq = ++submitSeqRef.current;
    const parts = consolidateParts(draftToSendParts(submission));
    if (parts.length === 0) return;
    if (detectExitBanner(getLines())) {
      send('\x15');
      await pause(80);
    } else {
      send('\x03');
      await pause(80);
    }
    if (submitSeqRef.current !== seq) return;
    for (const part of parts) {
      if (part.type === 'text') {
        if (part.content.length > 0) send(BPM_START + part.content + BPM_END);
      } else {
        send(`${BPM_START}${part.path} ${BPM_END}`);
      }
      await pause(part.type === 'text' ? 10 : 50);
      if (submitSeqRef.current !== seq) return;
    }
    const lineCount = Math.max(1, submission.text.split('\n').length);
    await pause(Math.min(400, 50 + lineCount * 2));
    if (submitSeqRef.current !== seq) return;
    send('\r');
  };

  const handleSend = async () => {
    const live = draftRef.current;
    if (isDraftEmpty(live) || submitting) return;
    const submission = cloneDraft(live);
    persistHistory(submission);
    lastSentRef.current = cloneDraft(submission);
    histIdxRef.current = null;
    histDraftRef.current = cloneDraft(EMPTY_DRAFT);
    setSuggest(null);
    setDraft(cloneDraft(EMPTY_DRAFT));
    commitDraft(cloneDraft(EMPTY_DRAFT));
    setSubmitting(true);
    try {
      await sendDraft(submission);
    } finally {
      setSubmitting(false);
    }
  };

  const handleInput = (e: React.FormEvent<HTMLDivElement>) => {
    const el = editorRef.current;
    if (!el || silentRef.current) return;
    const next = serializeEditor(el);
    const ev = e.nativeEvent as InputEvent;
    const inputType = ev.inputType ?? '';
    const prev = prevSnapRef.current;
    const wordBreak = inputType === 'insertText' && /\s/.test(ev.data ?? '') && !/\s$/.test(prev.text);
    const lineBreak = inputType === 'insertLineBreak' || inputType === 'insertParagraph';
    if (inputType !== lastInputTypeRef.current || wordBreak || lineBreak) checkpoint();
    lastInputTypeRef.current = inputType;
    redoRef.current = [];
    setDraft(next);
    draftRef.current = next;
    prevSnapRef.current = liveSnap();
    histIdxRef.current = null;
    if (!next.text.startsWith('/')) {
      setSuggest(null);
      return;
    }
    const caret = getCaretOffset(el) ?? next.text.length;
    setSuggest(detectSuggestTrigger(next.text, caret));
  };

  const handleEditorClick = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    const removeBtn = target.closest('[data-imgremove]');
    if (removeBtn) {
      e.preventDefault();
      e.stopPropagation();
      const chip = removeBtn.closest('[data-imgpath]');
      const el = editorRef.current;
      if (chip && el) {
        checkpoint();
        redoRef.current = [];
        removeChip(el, chip);
        syncFromEditor();
      }
      return;
    }
    const chip = target.closest('[data-imgpath]') as HTMLElement | null;
    if (chip?.dataset.imgpath) {
      e.stopPropagation();
      setPreview(chip.dataset.imgpath);
    }
  };

  const handlePaste = async (e: React.ClipboardEvent<HTMLDivElement>) => {
    const blobs = clipboardImages(e);
    const fallback = (e.clipboardData?.getData('text/plain') ?? '').trim();
    e.preventDefault();

    const saved = (() => {
      const sel = window.getSelection();
      if (!sel || sel.rangeCount === 0) return null;
      const r = sel.getRangeAt(0);
      const el = editorRef.current;
      if (!el || !el.contains(r.startContainer)) return null;
      return r.cloneRange();
    })();

    const paths: string[] = [];
    for (const blob of blobs) {
      try {
        imgSeqRef.current += 1;
        paths.push(await writeTempImage(blob, `${Date.now()}_${imgSeqRef.current}`));
      } catch {
        void 0;
      }
    }
    const text = blobs.length === 0 ? fallback : '';

    const el = editorRef.current;
    if (!el) return;
    el.focus({ preventScroll: true });
    if (saved && el.contains(saved.startContainer)) {
      const sel = window.getSelection();
      sel?.removeAllRanges();
      sel?.addRange(saved);
    } else {
      placeCaretAtEnd(el);
    }

    const existing = draftRef.current.images.length;
    if (text || paths.length > 0) {
      checkpoint();
      redoRef.current = [];
      insertPartsAtCaret(el, text, paths, existing + 1);
    }
    syncFromEditor();
    histIdxRef.current = null;

    const cur = draftRef.current;
    if (cur.text.startsWith('/')) setSuggest(detectSuggestTrigger(cur.text, getCaretOffset(el) ?? cur.text.length));
    else setSuggest(null);
  };

  const fetchSlash = React.useCallback((query: string): PromptSuggestion[] => {
    const q = query.toLowerCase();
    return CLAUDE_SLASH_COMMANDS.filter(
      (c) =>
        c.name.includes(q) ||
        c.desc.toLowerCase().includes(q) ||
        c.aliases?.some((a) => a.toLowerCase().includes(q))
    ).map((c) => ({
      id: c.name,
      display: c.name,
      subtext: c.aliases?.length ? `${c.desc} (${c.aliases.join(', ')})` : c.desc,
      icon: 'cmd',
      takesArg: c.takesArg
    }));
  }, []);

  const fetchModels = React.useCallback((query: string): PromptSuggestion[] => {
    const q = query.toLowerCase();
    return CLAUDE_MODELS.filter(
      (m) => m.name.toLowerCase().includes(q) || m.desc.toLowerCase().includes(q)
    ).map((m) => ({ id: m.name, display: m.name, subtext: m.desc, icon: 'model' }));
  }, []);

  const fetchEfforts = React.useCallback((query: string): PromptSuggestion[] => {
    const q = query.toLowerCase();
    return EFFORT_LEVELS.filter((l) => l.id.includes(q) || l.desc.toLowerCase().includes(q)).map((l) => ({
      id: l.id,
      color: l.color,
      display: l.id,
      subtext: l.desc,
      icon: 'effort'
    }));
  }, []);

  const onSlashSelect = (item: PromptSuggestion, submit?: boolean) => {
    const name = item.display;
    const noSubmit = name === '/model' || item.takesArg === true;
    const doSubmit = submit && !noSubmit;
    const next = { text: name + (doSubmit ? '' : ' '), images: [] };
    setDraft(next);
    commitDraft(next);
    if (name === '/model') setSuggest({ kind: 'model', query: '' });
    else if (name === '/effort') setSuggest({ kind: 'effort', query: '' });
    else setSuggest(null);
    if (doSubmit) void handleSend();
  };

  const onEffortSelect = (item: PromptSuggestion, submit?: boolean) => {
    const next = { text: `/effort ${item.display}`, images: [] };
    setDraft(next);
    commitDraft(next);
    setSuggest(null);
    if (submit) void handleSend();
  };

  const onModelSelect = (item: PromptSuggestion, submit?: boolean) => {
    const next = { text: `/model ${item.display}`, images: [] };
    setDraft(next);
    commitDraft(next);
    setSuggest(null);
    if (submit) void handleSend();
  };

  const onModelHighlight = (item: PromptSuggestion) => {
    const next = { text: `/model ${item.display}`, images: [] };
    setDraft(next);
    applyDraft(next);
  };

  const stepHistory = (dir: -1 | 1): boolean => {
    const el = editorRef.current;
    if (!el) return false;
    const hist = historyRef.current;
    if (dir === -1) {
      if (hist.length === 0 || !isCaretOnFirstLine(el)) return false;
      if (histIdxRef.current == null) {
        histDraftRef.current = cloneDraft(draftRef.current);
        histIdxRef.current = hist.length - 1;
      } else if (histIdxRef.current > 0) {
        histIdxRef.current--;
      }
      const next = partsToDraft(hist[histIdxRef.current] ?? []);
      setDraft(next);
      commitDraft(next);
      setSuggest(null);
      return true;
    }
    if (histIdxRef.current == null || !isCaretOnLastLine(el)) return false;
    if (histIdxRef.current < hist.length - 1) {
      histIdxRef.current++;
      const next = partsToDraft(hist[histIdxRef.current] ?? []);
      setDraft(next);
      commitDraft(next);
    } else {
      histIdxRef.current = null;
      const restored = cloneDraft(histDraftRef.current);
      setDraft(restored);
      commitDraft(restored);
    }
    setSuggest(null);
    return true;
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const key = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && !e.altKey && (key === 'z' || key === 'y')) {
      e.preventDefault();
      e.stopPropagation();
      if (key === 'y' || e.shiftKey) doRedo();
      else doUndo();
      return;
    }
    if (e.key === 'Tab' && e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();
      send('\x1b[Z');
      return;
    }
    if (suggest && suggestRef.current?.handleKeyDown(e)) return;
    if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      e.stopPropagation();
      focusTerminal();
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      send('\x03');
      if (isDraftEmpty(draftRef.current) && !isDraftEmpty(lastSentRef.current)) {
        const restored = cloneDraft(lastSentRef.current);
        setDraft(restored);
        histIdxRef.current = null;
        setSuggest(null);
        commitDraft(restored);
      }
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing && e.nativeEvent.keyCode !== 229) {
      e.preventDefault();
      void handleSend();
      return;
    }
    if (e.key === 'ArrowUp' && !e.shiftKey && !e.metaKey && !e.ctrlKey && !suggest) {
      if (stepHistory(-1)) e.preventDefault();
      return;
    }
    if (e.key === 'ArrowDown' && !e.shiftKey && !e.metaKey && !e.ctrlKey && !suggest) {
      if (stepHistory(1)) e.preventDefault();
    }
  };

  const is1M = React.useMemo(() => {
    if (structured?.contextWindow) return structured.contextWindow >= 1_000_000;
    if (scraped && /1m/i.test(scraped.contextInfo ?? '')) return true;
    return Boolean(structured && /\[1m\]/i.test(`${structured.model ?? ''} ${structured.defaultModel ?? ''}`));
  }, [scraped, structured]);

  const currentModelId = React.useMemo(() => {
    if (scraped) {
      const t = scraped.model.toLowerCase();
      const oneM = /1m/i.test(scraped.contextInfo ?? '');
      const sm = t.match(/(sonnet|fable)\s*(\d+)(?:[.\s-](\d+))?/);
      if (sm) {
        const id = sm[3] ? `claude-${sm[1]}-${sm[2]}-${sm[3]}` : `claude-${sm[1]}-${sm[2]}`;
        return `${id}${oneM ? '[1m]' : ''}`;
      }
      const m = t.match(/opus\s*(\d)[.\s-]?(\d)/);
      if (m) return `claude-opus-${m[1]}-${m[2]}${oneM ? '[1m]' : ''}`;
      return '';
    }
    const base = matchQuickSwitchId(structured?.model);
    return base && is1M && !base.includes('[1m]') ? `${base}[1m]` : base;
  }, [scraped, structured, is1M]);

  const parsed = React.useMemo<ParsedStatus>(() => {
    const base: ParsedStatus = { ...status };
    if (scraped) {
      base.model = scraped.model;
      base.contextInfo = scraped.contextInfo;
    } else if (structured?.model) {
      const pm = prettyModel(structured.model);
      if (pm.model) base.model = pm.model;
      if (!base.contextInfo && pm.contextInfo) base.contextInfo = pm.contextInfo;
    }
    if (!base.mode && structured) base.mode = prettyMode(structured.permissionMode ?? structured.mode);
    if (structured?.contextPercent != null) {
      base.progress = Math.min(100, Math.round(structured.contextPercent));
      base.contextInfo = structured.contextWindow
        ? structured.contextWindow >= 1_000_000
          ? '1M'
          : `${Math.round(structured.contextWindow / 1000)}k`
        : base.contextInfo;
    } else if (base.progress == null && structured?.contextTokens != null) {
      const win = is1M ? 1_000_000 : 200_000;
      base.progress = Math.min(100, Math.round((structured.contextTokens / win) * 100));
      if (!base.contextInfo) base.contextInfo = is1M ? '1M' : '200k';
    }
    return base;
  }, [status, scraped, structured, is1M]);

  const effort = structured?.effort ?? '';
  const effortColor = EFFORT_LEVELS.find((l) => l.id === effort)?.color;

  const hasStatus = Boolean(parsed.model || parsed.mode || parsed.focused || parsed.progress != null || effort || structured?.costUsd != null);

  const suggestFetch = { slash: fetchSlash, model: fetchModels, effort: fetchEfforts };
  const suggestSelect = { slash: onSlashSelect, model: onModelSelect, effort: onEffortSelect };

  const toggleModelMenu = () => setModelMenu((o) => !o);
  const toggleEffortMenu = () => setEffortMenu((o) => !o);
  const restore = () => setManualHide(false);

  const focusEditor = (e: React.MouseEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    if (target.closest('[data-imgpath]') || target.closest(`.${styles.actions}`) || target.closest(`.${styles.editor}`)) return;
    const el = editorRef.current;
    if (el) {
      el.focus({ preventScroll: true });
      placeCaretAtEnd(el);
    }
  };

  const pickModel = (id: string) => () => {
    void sendDraft({ text: `/model ${id}`, images: [] });
    setModelMenu(false);
  };

  const pickEffort = (id: string) => () => {
    void sendDraft({ text: `/effort ${id}`, images: [] });
    setEffortMenu(false);
  };

  if (!claudeActive) return null;

  return (
    <>
      {manualHide && !questionMode && (
        <button className={styles.restore} title="Restore prompt (Ctrl+G)" onClick={restore}>
          <ClaudeLogo />
        </button>
      )}
      <div className={styles.bar} style={hidden ? { display: 'none' } : undefined} onClick={focusEditor}>
        <div className={styles.row}>
          <div className={styles.logo} aria-hidden="true">
            <ClaudeLogo />
          </div>
          <div
            ref={editorRef}
            role="textbox"
            contentEditable
            spellCheck={false}
            aria-multiline="true"
            aria-label="Prompt input"
            className={styles.editor}
            suppressContentEditableWarning
            data-placeholder="Tell Claude what to do... (/ commands, up-arrow history)"
            data-empty={isEmpty ? 'true' : undefined}
            onInput={handleInput}
            onPaste={handlePaste}
            onClick={handleEditorClick}
            onKeyDown={onKeyDown}
          />
          <div className={styles.actions}>
            <div className={styles.action} ref={effortRef}>
              <button
                type="button"
                title={effort ? `Effort: ${effort}` : 'Set effort'}
                className={styles.effort}
                style={effortColor ? { color: effortColor } : undefined}
                onClick={toggleEffortMenu}
              >
                <Brain size={14} />
              </button>
              {effortMenu && (
                <div className={styles.menu}>
                  {EFFORT_LEVELS.map((l) => (
                    <button
                      key={l.id}
                      type="button"
                      style={{ color: l.color }}
                      onClick={pickEffort(l.id)}
                      className={l.id === effort ? `${styles.menuItem} ${styles.menuActive}` : styles.menuItem}
                    >
                      {l.id}
                      <span className={styles.menuSub}>{l.desc}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
            <div className={styles.action} ref={modelRef}>
              <button type="button" className={styles.model} title="Switch model" onClick={toggleModelMenu}>
                {MODEL_QUICK_SWITCHES.find((m) => m.id === currentModelId)?.title ?? 'Model'}
                <ChevronDown size={11} />
              </button>
              {modelMenu && (
                <div className={styles.menu}>
                  {MODEL_QUICK_SWITCHES.map((m) => (
                    <button
                      key={m.id}
                      type="button"
                      onClick={pickModel(m.id)}
                      className={m.id === currentModelId ? `${styles.menuItem} ${styles.menuActive}` : styles.menuItem}
                    >
                      {m.title}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </div>
        <div className={styles.hints}>
          {submitting
            ? 'Submitting prompt...'
            : suggest
              ? 'Enter select & send - Tab complete - Esc close'
              : 'Enter send - Shift+Enter newline - Tab terminal'}
        </div>
        {hasStatus && (
          <div className={styles.footer} aria-hidden="true">
            {parsed.mode && (
              <span className={`${styles.chip} ${styles.chipMode} ${MODE_CLASS[modeKey(parsed.mode)]}`}>
                <Sparkles size={10} />
                <span className={styles.chipLabel}>{parsed.mode}</span>
              </span>
            )}
            {parsed.model && (
              <span className={styles.chip}>
                <span className={styles.chipLabel}>{parsed.model}</span>
                {parsed.contextInfo && <span className={styles.chipSub}>{parsed.contextInfo}</span>}
              </span>
            )}
            {effort && (
              <span className={styles.chip} style={effortColor ? { color: effortColor } : undefined}>
                <Brain size={9} />
                <span className={styles.chipLabel}>{effort}</span>
              </span>
            )}
            {parsed.focused && (
              <span className={`${styles.chip} ${styles.chipFocus}`}>
                <span className={styles.dot} />
                <span className={styles.chipLabel}>focus</span>
              </span>
            )}
            {(parsed.progress != null || structured?.costUsd != null) && (
              <span className={styles.meter}>
                {structured?.costUsd != null && (
                  <span className={`${styles.chip} ${styles.cost}`}>
                    <span className={styles.chipLabel}>{formatCost(structured.costUsd)}</span>
                  </span>
                )}
                {parsed.progress != null && (
                  <>
                    <span className={styles.track}>
                      <span
                        className={styles.fill}
                        style={{ width: `${Math.min(100, Math.max(0, parsed.progress))}%`, background: progressColor(parsed.progress) }}
                      />
                    </span>
                    <span className={styles.progressLabel}>{parsed.progress}%</span>
                  </>
                )}
              </span>
            )}
          </div>
        )}
        {suggest && (
          <Suggest
            ref={suggestRef}
            query={suggest.query}
            fetchFn={suggestFetch[suggest.kind]}
            onSelect={suggestSelect[suggest.kind]}
            onHighlight={suggest.kind === 'model' ? onModelHighlight : undefined}
            onClose={() => setSuggest(null)}
          />
        )}
      </div>
      {preview && <Magnifier path={preview} onClose={() => setPreview(null)} />}
    </>
  );
};

export default AgentBar;
