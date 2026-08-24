import type { FileDiff } from '~/domain/interfaces/git.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { DiffViewMode, HighlightMode } from '~/domain/interfaces/diff.interface';

import React from 'react';
import { listen } from '@tauri-apps/api/event';
import {
  X,
  Info,
  Space,
  Undo2,
  Shrink,
  PenLine,
  ArrowUp,
  ArrowDown,
  LayoutGrid,
  ChevronLeft,
  ChevronRight,
  LoaderCircle
} from 'lucide-react';

import Panes from '~/components/DiffViewer/Panes';
import Picker from '~/components/DiffViewer/Picker';
import Unified from '~/components/DiffViewer/Unified';
import FileIcon from '~/components/commons/FileIcon';
import ContextMenu from '~/components/commons/ContextMenu';
import { gitDiffFile, gitRevertHunk, gitWatchFile, gitUnwatchFile, gitCommitDiffFile } from '~/adapter/git/git.client';
import { langOf, computeDiff, revertChunk, computeIntraLine } from '~/usecase/util/diff';
import { isCapturing, getBinding, formatCombo, matchCommand, type CommandId } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

export interface DiffViewerMode {
  embedded?: boolean;
  exiting?: boolean;
  keys?: boolean;
}

export interface DiffViewerHandlers {
  onClose?: () => void;
  onPrevFile?: () => void;
  onNextFile?: () => void;
  onEditFile?: () => void;
  onAddToCanvas?: () => void;
}

interface DiffViewerProps {
  root: string;
  file: string;
  commit?: string;
  mode?: DiffViewerMode;
  handlers?: DiffViewerHandlers;
}

const VIEW_LABELS: Record<DiffViewMode, string> = {
  'side-by-side': 'Side-by-side viewer',
  unified: 'Unified viewer'
};

const HIGHLIGHT_LABELS: Record<HighlightMode, string> = {
  lines: 'Highlight lines',
  words: 'Highlight words',
  characters: 'Highlight characters',
  none: 'Do not highlight'
};

const REFRESH_MS = 5000;
const VIEW_MODES: DiffViewMode[] = ['side-by-side', 'unified'];
const HIGHLIGHT_MODES: HighlightMode[] = ['lines', 'words', 'characters', 'none'];

const message = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const eolLabel = (crlf: boolean): string => (crlf ? 'CRLF' : 'LF');

const DiffViewer = ({ root, file, commit, mode, handlers }: DiffViewerProps) => {
  const { embedded, exiting, keys = !mode?.embedded } = mode ?? {};
  const { onClose, onPrevFile, onNextFile, onEditFile, onAddToCanvas } = handlers ?? {};
  const shellRef = React.useRef<HTMLDivElement>(null);
  const [diff, setDiff] = React.useState<FileDiff | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [view, setView] = React.useState<DiffViewMode>('side-by-side');
  const [highlight, setHighlight] = React.useState<HighlightMode>('words');
  const [collapse, setCollapse] = React.useState(true);
  const [ignoreWs, setIgnoreWs] = React.useState(false);
  const [chunkIndex, setChunkIndex] = React.useState(-1);
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);

  React.useEffect(() => {
    let alive = true;
    let poll = 0;
    let debounce = 0;
    let watchId: number | null = null;
    setDiff(null);
    setError(null);
    setChunkIndex(-1);

    const load = (quiet: boolean) =>
      (commit ? gitCommitDiffFile(root, commit, file) : gitDiffFile(root, file))
        .then((result) => {
          if (!alive) return;
          setDiff((prev) =>
            prev && prev.old === result.old && prev.new === result.new && prev.binary === result.binary ? prev : result
          );
          setError(null);
        })
        .catch((err: unknown) => alive && !quiet && setError(message(err)));

    void load(false);

    const bump = () => {
      window.clearTimeout(debounce);
      debounce = window.setTimeout(() => void load(true), 150);
    };

    if (commit) return () => (alive = false);

    void gitWatchFile(root, file)
      .then((id) => {
        if (!alive) return void gitUnwatchFile(id);
        watchId = id;
      })
      .catch(() => {
        if (alive) poll = window.setInterval(() => void load(true), REFRESH_MS);
      });

    const off = listen<{ root: string; file: string }>('diff:changed', (e) => {
      if (e.payload.root === root && e.payload.file === file) bump();
    });

    return () => {
      alive = false;
      window.clearInterval(poll);
      window.clearTimeout(debounce);
      if (watchId !== null) void gitUnwatchFile(watchId);
      void off.then((un) => un());
    };
  }, [root, file, commit]);

  const chunks = React.useMemo(() => (diff ? computeDiff(diff.old, diff.new, ignoreWs) : []), [diff, ignoreWs]);

  React.useEffect(() => {
    setChunkIndex((i) => Math.min(i, chunks.length - 1));
  }, [chunks]);

  React.useEffect(() => {
    if (!keys) return;

    const onKey = (e: KeyboardEvent) => {
      if (isCapturing()) return;
      const cmd = matchCommand(e);
      if (!cmd) return;

      const run = (): boolean => {
        if (cmd === 'diff.close' && onClose) {
          if (shellRef.current?.querySelector('[data-picker-open]')) return false;
          onClose();
          return true;
        }
        if (cmd === 'diff.prevChunk') {
          setChunkIndex((i) => Math.max(0, i - 1));
          return true;
        }
        if (cmd === 'diff.nextChunk') {
          setChunkIndex((i) => Math.min(chunks.length - 1, i + 1));
          return true;
        }
        if (cmd === 'diff.prevFile' && onPrevFile) {
          onPrevFile();
          return true;
        }
        if (cmd === 'diff.nextFile' && onNextFile) {
          onNextFile();
          return true;
        }
        if (cmd === 'diff.editFile' && onEditFile) {
          onEditFile();
          return true;
        }
        return false;
      };

      if (!run()) return;
      e.preventDefault();
      e.stopPropagation();
    };

    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [keys, onClose, onPrevFile, onNextFile, onEditFile, chunks.length]);
  const intra = React.useMemo(
    () => (diff ? computeIntraLine(diff.old, diff.new, chunks, highlight) : null),
    [diff, chunks, highlight]
  );

  const lang = langOf(file);
  const slash = Math.max(file.lastIndexOf('/'), file.lastIndexOf('\\'));
  const name = file.slice(slash + 1);
  const dir = file.slice(0, slash + 1);

  const prevChunk = () => setChunkIndex((i) => Math.max(0, i - 1));
  const nextChunk = () => setChunkIndex((i) => Math.min(chunks.length - 1, i + 1));
  const toggleCollapse = () => setCollapse((v) => !v);
  const toggleWs = () => setIgnoreWs((v) => !v);
  const combo = (id: CommandId) => (keys ? formatCombo(getBinding(id)) : undefined);

  const revert = () => {
    const chunk = chunks[chunkIndex];
    if (!diff || !chunk) return;
    gitDiffFile(root, file)
      .then((fresh) => {
        if (fresh.old !== diff.old || fresh.new !== diff.new) {
          setDiff(fresh);
          throw new Error('File changed on disk, diff refreshed');
        }
        return gitRevertHunk(root, file, revertChunk(diff.old, diff.new, chunk), diff.crlf);
      })
      .then(() => gitDiffFile(root, file))
      .then((result) => {
        setDiff(result);
        setError(null);
      })
      .catch((err: unknown) => setError(message(err)));
  };

  const openMenu = (e: React.MouseEvent) => {
    if (!onEditFile) return;
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const closeMenu = () => setMenu(null);

  const editFile = () => {
    closeMenu();
    onEditFile?.();
  };

  const menuItems: ContextMenuEntry[] = [
    {
      label: 'Edit file',
      icon: <PenLine size={15} strokeWidth={1.75} />,
      shortcut: combo('diff.editFile'),
      onSelect: editFile
    }
  ];

  const ready = Boolean(diff) && !error;
  const same = ready && !diff!.binary && chunks.length === 0;
  const eolOnly = same && diff!.crlf !== diff!.old_crlf;
  const banner = !same ? null : eolOnly ? 'Contents have differences only in line separators' : 'Contents are identical';
  const total = chunks.length === 1 ? '1 difference' : `${chunks.length} differences`;
  const status = chunkIndex < 0 ? total : `${chunkIndex + 1} of ${total}`;

  const body = () => {
    if (error) return <div className={styles.notice}>{error}</div>;
    if (!diff) {
      return (
        <div className={styles.notice}>
          <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
        </div>
      );
    }
    if (diff.binary) return <div className={styles.notice}>Binary file</div>;

    if (view === 'unified') {
      return <Unified old={diff.old} next={diff.new} lang={lang} chunks={chunks} collapse={collapse} />;
    }

    return (
      <Panes
        old={diff.old}
        next={diff.new}
        lang={lang}
        mode={highlight}
        chunks={chunks}
        intra={intra}
        collapse={collapse}
        chunkIndex={chunkIndex}
      />
    );
  };

  const shell = embedded ? styles.embed : exiting ? `${styles.overlay} ${styles.exit}` : styles.overlay;

  return (
    <div ref={shellRef} className={shell}>
      <div className={styles.header}>
        <FileIcon name={name} size={14} />
        <span className={styles.path}>
          <span className={styles.dir}>{dir}</span>
          {name}
        </span>
        <span className={styles.spacer} />
        {onAddToCanvas && (
          <button className={styles.action} onClick={onAddToCanvas} data-tooltip="Add to canvas" aria-label="Add to canvas">
            <LayoutGrid size={14} strokeWidth={2} />
          </button>
        )}
        {onClose && (
          <button
            className={styles.action}
            onClick={onClose}
            data-tooltip="Close"
            data-shortcut={combo('diff.close')}
            aria-label="Close"
          >
            <X size={15} strokeWidth={2} />
          </button>
        )}
      </div>

      <div className={styles.toolbar}>
        <button
          className={styles.tool}
          onClick={prevChunk}
          disabled={!ready || chunkIndex <= 0}
          data-tooltip="Previous difference"
          data-shortcut={combo('diff.prevChunk')}
          aria-label="Previous difference"
        >
          <ArrowUp size={14} strokeWidth={2} />
        </button>
        <button
          className={styles.tool}
          onClick={nextChunk}
          disabled={!ready || chunkIndex >= chunks.length - 1}
          data-tooltip="Next difference"
          data-shortcut={combo('diff.nextChunk')}
          aria-label="Next difference"
        >
          <ArrowDown size={14} strokeWidth={2} />
        </button>
        <button
          className={styles.tool}
          onClick={revert}
          disabled={!ready || Boolean(commit) || !chunks[chunkIndex]}
          data-tooltip="Revert difference"
          aria-label="Revert difference"
        >
          <Undo2 size={14} strokeWidth={2} />
        </button>
        <span className={styles.divider} />
        <button
          className={styles.tool}
          onClick={onPrevFile}
          disabled={!onPrevFile}
          data-tooltip="Previous file"
          data-shortcut={combo('diff.prevFile')}
          aria-label="Previous file"
        >
          <ChevronLeft size={14} strokeWidth={2} />
        </button>
        <button
          className={styles.tool}
          onClick={onNextFile}
          disabled={!onNextFile}
          data-tooltip="Next file"
          data-shortcut={combo('diff.nextFile')}
          aria-label="Next file"
        >
          <ChevronRight size={14} strokeWidth={2} />
        </button>
        <span className={styles.divider} />
        <Picker value={view} options={VIEW_MODES} labels={VIEW_LABELS} disabled={!ready} onChange={setView} />
        <Picker
          value={highlight}
          options={HIGHLIGHT_MODES}
          labels={HIGHLIGHT_LABELS}
          disabled={!ready || view === 'unified'}
          onChange={setHighlight}
        />
        <span className={styles.divider} />
        <button
          className={styles.tool}
          onClick={toggleCollapse}
          disabled={!ready}
          data-active={collapse}
          data-tooltip="Collapse unchanged"
          aria-label="Collapse unchanged"
        >
          <Shrink size={14} strokeWidth={2} />
        </button>
        <button
          className={styles.tool}
          onClick={toggleWs}
          disabled={!ready}
          data-active={ignoreWs}
          data-tooltip="Ignore whitespace"
          aria-label="Ignore whitespace"
        >
          <Space size={14} strokeWidth={2} />
        </button>
        <span className={styles.spacer} />
        {ready && <span className={styles.status}>{status}</span>}
      </div>

      {banner && (
        <div className={styles.banner}>
          <Info size={13} strokeWidth={2} />
          <span>{banner}</span>
          {eolOnly && (
            <span className={styles.eol}>
              {eolLabel(diff!.old_crlf)} to {eolLabel(diff!.crlf)}
            </span>
          )}
        </div>
      )}

      <div className={styles.body} onContextMenu={openMenu}>
        {body()}
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems} onClose={closeMenu} />}
    </div>
  );
};

export default DiffViewer;
