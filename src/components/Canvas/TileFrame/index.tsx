import React from 'react';
import type { EditorView } from '@codemirror/view';
import { X, Pin, Link2, PinOff, Copy, Focus, Pencil, Trash2, ArrowUp, Maximize, Minimize, RotateCw, CopyPlus, ArrowDown, Link2Off, GitBranch, ShieldCheck, ChevronDown, FolderOpen, ClipboardCopy, ClipboardPaste, ArrowLeftRight } from 'lucide-react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import type { TabMeta } from '~/domain/interfaces/workspace.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { NotifyKind } from '~/components/commons/Notifications/bridge';
import NoteTile from '~/components/Canvas/NoteTile';
import DiffViewer from '~/components/DiffViewer';
import { noteTheme } from '~/usecase/util/note';
import { parseFrontTitle } from '~/usecase/util/noteMeta';
import ClaudeLogo from '~/components/commons/ClaudeLogo';
import ContextMenu from '~/components/commons/ContextMenu';
import BranchMenu from '~/components/Canvas/TileFrame/BranchMenu';
import GridTerminal from '~/components/Terminal/GridTerminal';
import { useBranches } from '~/usecase/hooks/useBranches';
import { useAheadBehind } from '~/usecase/hooks/useAheadBehind';
import { stripSpinner, stripStarPrefix, hasSpinnerPrefix } from '~/usecase/util/title';
import { TILE_GAP, TILE_HEADER } from '~/usecase/util/constants';
import { getBinding, formatCombo } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

interface TileFrameProps {
  tile: Tile;
  view: View;
  active: boolean;
  selected: boolean;
  alert: NotifyKind | null;
  visible: boolean;
  live: boolean;
  hidden: boolean;
  fullscreen: boolean;
  exiting: boolean;
  vpW: number;
  vpH: number;
  onClose: (id: string) => void;
  onSnap: (id: string) => void;
  onActivate: (id: string) => void;
  onFocusTile: (id: string, zoomToMax?: boolean) => void;
  onToggleFullscreen: (id: string) => void;
  onMove: (id: string, dx: number, dy: number) => void;
  onResize: (id: string, dir: string, dx: number, dy: number) => void;
  onCwd: (id: string, cwd: string, branch?: string) => void;
  onAgentState: (id: string, live: boolean, busy: boolean) => void;
  onOscTitle: (id: string, title: string) => void;
  onNoteChange: (id: string, content: string) => void;
  onNoteEditor: (id: string, editor: EditorView | null) => void;
  onNoteTitle: (id: string, title: string) => void;
  onCopyNote: (id: string) => void;
  onCopyNoteSelection: (id: string) => void;
  onPasteNote: (id: string) => void;
  onToggleRaw: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onCopyPath: (id: string) => void;
  onReveal: (id: string) => void;
  onDuplicate: (id: string) => void;
  onTogglePin: (id: string) => void;
  onToggleSelect: (id: string) => void;
  wsId: string | null;
  linkActive: boolean;
  linkTarget: { id: string; name: string } | null;
  linkedTerms: { id: string; name: string }[];
  onLink: (noteId: string, termId: string) => void;
  onUnlink: (noteId: string, termId: string) => void;
  onLinkDragStart: (noteId: string, e: React.PointerEvent) => void;
  tabs: TabMeta[];
  activeTabId: string | null;
  onMoveToTab: (id: string, targetTabId: string) => void;
}

const HANDLES = ['n', 's', 'e', 'w', 'nw', 'ne', 'sw', 'se'];

const FS_PAD = 28;

const DRAG_THRESHOLD = 4;

const devicePx = (v: number): number => {
  const dpr = window.devicePixelRatio || 1;
  return Math.round(v * dpr) / dpr;
};

const TileFrame = ({ tile, view, active, selected, alert, visible, live, hidden, fullscreen, exiting, vpW, vpH, onMove, onSnap, onClose, onResize, onActivate, onFocusTile, onToggleFullscreen, onCwd, onAgentState, onOscTitle, onNoteChange, onNoteEditor, onNoteTitle, onCopyNote, onCopyNoteSelection, onPasteNote, onToggleRaw, onRename, onCopyPath, onReveal, onDuplicate, onTogglePin, onToggleSelect, wsId, linkActive, linkTarget, linkedTerms, onLink, onUnlink, onLinkDragStart, tabs, activeTabId, onMoveToTab }: TileFrameProps) => {
  const k = view.k;
  const drag = React.useRef<{ sx: number; sy: number; ox: number; oy: number; pid: number; on: boolean } | null>(null);
  const resize = React.useRef<{ x: number; y: number; dir: string } | null>(null);
  const [claudeLive, setClaudeLive] = React.useState(false);
  const [claudeBusy, setClaudeBusy] = React.useState(false);
  const [progress, setProgress] = React.useState<{ state: number; pct: number } | null>(null);
  const [diff, setDiff] = React.useState<{ a: number; r: number } | null>(null);
  const onClaudeStatus = (s: string) => setClaudeBusy(s === 'busy');
  React.useEffect(() => {
    onAgentState(tile.id, claudeLive, claudeBusy);
  }, [tile.id, claudeLive, claudeBusy, onAgentState]);
  React.useEffect(() => () => onAgentState(tile.id, false, false), [tile.id, onAgentState]);
  const onClaudeDiff = (a: number, r: number) => setDiff(a || r ? { a, r } : null);
  const onProgress = (state: number, pct: number) => setProgress(state === 0 || state === 3 ? null : { state, pct });

  const startDrag = (e: React.PointerEvent) => {
    if (e.button !== 0 || fullscreen) return;
    e.stopPropagation();
    if (e.shiftKey) {
      onToggleSelect(tile.id);
      return;
    }
    if (!selected) onActivate(tile.id);
    if (tile.pinned) return;
    drag.current = { sx: e.clientX, sy: e.clientY, ox: tile.x, oy: tile.y, pid: e.pointerId, on: false };
  };
  const onDrag = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    if (!d.on) {
      if (Math.abs(e.clientX - d.sx) < DRAG_THRESHOLD && Math.abs(e.clientY - d.sy) < DRAG_THRESHOLD) return;
      d.on = true;
      (e.target as Element).setPointerCapture(d.pid);
    }
    onMove(tile.id, d.ox + (e.clientX - d.sx) / k, d.oy + (e.clientY - d.sy) / k);
  };
  const endDrag = () => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    if (d.on) onSnap(tile.id);
  };

  const startResize = (dir: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    onActivate(tile.id);
    (e.target as Element).setPointerCapture(e.pointerId);
    resize.current = { x: e.clientX, y: e.clientY, dir };
  };
  const onResizeMove = (e: React.PointerEvent) => {
    const r = resize.current;
    if (!r) return;
    onResize(tile.id, r.dir, e.clientX - r.x, e.clientY - r.y);
    resize.current = { ...r, x: e.clientX, y: e.clientY };
  };
  const endResize = () => {
    if (!resize.current) return;
    resize.current = null;
    onSnap(tile.id);
  };

  const [restartKey, setRestartKey] = React.useState(0);
  const [elevated, setElevated] = React.useState(false);
  const restartTile = () => setRestartKey((n) => n + 1);

  const toggleElevated = () => {
    setElevated((v) => !v);
    setRestartKey((n) => n + 1);
  };
  const closeTile = () => onClose(tile.id);
  const focusTile = () => onFocusTile(tile.id, true);
  const toggleFullscreen = () => onToggleFullscreen(tile.id);
  const oscTitle = tile.oscTitle ? stripStarPrefix(tile.oscTitle).trim() : '';
  const spinning = !tile.userTitle && claudeBusy && hasSpinnerPrefix(oscTitle);
  const label = tile.userTitle
    || (claudeLive && oscTitle && (spinning ? oscTitle : stripSpinner(oscTitle)))
    || tile.cwd
    || tile.autoTitle
    || `${tile.type} · ${tile.id}`;
  const folder = tile.cwd && label !== tile.cwd ? tile.cwd.replace(/[\\/]+$/, '').split(/[\\/]/).pop() : '';

  const note = tile.type === 'note';
  const code = tile.type === 'code';
  const tint = note ? noteTheme(tile.color) : null;
  const noteTint = tint ? { background: tint.body, color: tint.text } : null;
  const noteTitle = note ? parseFrontTitle(tile.content) : '';
  const noteLabel = note ? noteTitle || 'Note' : null;
  const copyNote = () => onCopyNote(tile.id);
  const copyNoteSelection = () => onCopyNoteSelection(tile.id);
  const pasteNote = () => onPasteNote(tile.id);
  const toggleRaw = () => onToggleRaw(tile.id);
  const stopDrag = (e: React.PointerEvent) => e.stopPropagation();
  const startLinkDrag = (e: React.PointerEvent) => onLinkDragStart(tile.id, e);

  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [menuInContent, setMenuInContent] = React.useState(false);
  const [renaming, setRenaming] = React.useState(false);
  const [draft, setDraft] = React.useState('');
  const renameRef = React.useRef<HTMLInputElement>(null);

  const closeMenu = () => setMenu(null);
  const openMenu = (e: React.MouseEvent, inContent = false) => {
    if (fullscreen) return;
    e.preventDefault();
    e.stopPropagation();
    onActivate(tile.id);
    setMenuInContent(inContent);
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const startRename = () => {
    setDraft(note ? noteTitle : tile.userTitle || tile.autoTitle || '');
    setRenaming(true);
  };
  const startTitleEdit = (e: React.MouseEvent) => {
    e.stopPropagation();
    startRename();
  };
  const commitRename = () => {
    if (!renaming) return;
    setRenaming(false);
    const next = draft.trim();
    if (note) {
      if (next !== noteTitle) onNoteTitle(tile.id, next);
      return;
    }
    if (next === (tile.userTitle || '')) return;
    onRename(tile.id, next);
  };
  const onRenameKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') setRenaming(false);
  };

  React.useEffect(() => {
    if (renaming) renameRef.current?.select();
  }, [renaming]);

  const togglePin = () => onTogglePin(tile.id);
  const duplicate = () => onDuplicate(tile.id);
  const copyPath = () => onCopyPath(tile.id);
  const reveal = () => onReveal(tile.id);

  const [branchLocal, setBranchLocal] = React.useState<{ x: number; y: number } | null>(null);
  const branches = useBranches(tile.cwd, branchLocal !== null);
  const track = useAheadBehind(tile.cwd, tile.branch);
  const snapCurrent = branches.snapshot?.current ?? null;

  React.useEffect(() => {
    if (!snapCurrent || !tile.cwd || snapCurrent === tile.branch) return;
    onCwd(tile.id, tile.cwd, snapCurrent);
  }, [snapCurrent, tile.id, tile.cwd, tile.branch, onCwd]);

  const closeBranches = () => setBranchLocal(null);

  const openBranches = (e: React.MouseEvent) => {
    e.stopPropagation();
    const btn = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const tileRect = (e.currentTarget as HTMLElement).closest('[data-tile]')?.getBoundingClientRect();
    if (!tileRect) return;
    const scale = fullscreen ? 1 : k;
    setBranchLocal({ x: (btn.left - tileRect.left) / scale, y: (btn.bottom - tileRect.top) / scale + 4 });
  };

  const fullscreenItem: ContextMenuEntry = {
    label: fullscreen ? 'Exit fullscreen' : 'Fullscreen',
    icon: fullscreen ? <Minimize size={15} strokeWidth={1.75} /> : <Maximize size={15} strokeWidth={1.75} />,
    shortcut: formatCombo(getBinding('tile.fullscreen')),
    onSelect: toggleFullscreen
  };
  const pinItem: ContextMenuEntry = {
    label: tile.pinned ? 'Unpin' : 'Pin',
    icon: tile.pinned ? <PinOff size={15} strokeWidth={1.75} /> : <Pin size={15} strokeWidth={1.75} />,
    onSelect: togglePin
  };
  const closeItem: ContextMenuEntry = {
    label: 'Close',
    icon: <Trash2 size={15} strokeWidth={1.75} />,
    shortcut: formatCombo(getBinding('tile.close')),
    danger: true,
    onSelect: closeTile
  };

  const noteContentItems: ContextMenuEntry[] = menuInContent
    ? [
        { label: 'Copy', icon: <Copy size={15} strokeWidth={1.75} />, onSelect: copyNoteSelection },
        { label: 'Paste', icon: <ClipboardPaste size={15} strokeWidth={1.75} />, onSelect: pasteNote },
        'separator'
      ]
    : [];

  const linkItems: ContextMenuEntry[] = [
    ...(linkTarget
      ? [
          {
            label: `Link to ${linkTarget.name}`,
            icon: <Link2 size={15} strokeWidth={1.75} />,
            onSelect: () => onLink(tile.id, linkTarget.id)
          }
        ]
      : []),
    ...linkedTerms.map((lt) => ({
      label: `Unlink from ${lt.name}`,
      icon: <Link2Off size={15} strokeWidth={1.75} />,
      onSelect: () => onUnlink(tile.id, lt.id)
    }))
  ];
  if (linkItems.length) linkItems.push('separator');

  const noteMenuItems: ContextMenuEntry[] = [
    ...noteContentItems,
    { label: 'Rename', icon: <Pencil size={15} strokeWidth={1.75} />, onSelect: startRename },
    { label: 'Duplicate', icon: <CopyPlus size={15} strokeWidth={1.75} />, onSelect: duplicate },
    pinItem,
    'separator',
    ...linkItems,
    { label: 'Focus', icon: <Focus size={15} strokeWidth={1.75} />, onSelect: focusTile },
    fullscreenItem,
    'separator',
    closeItem
  ];

  const otherTabs = tabs.filter((t) => t.id !== activeTabId);
  if (otherTabs.length > 0) {
    const moveItem: ContextMenuEntry = {
      label: 'Move to Tab',
      icon: <ArrowLeftRight size={15} strokeWidth={1.75} />,
      submenu: otherTabs.map((t) => ({
        label: t.name,
        onSelect: () => onMoveToTab(tile.id, t.id)
      }))
    };
    const noteCloseIndex = noteMenuItems.findIndex((item) => typeof item !== 'string' && item.label === 'Close');
    if (noteCloseIndex !== -1) {
      noteMenuItems.splice(noteCloseIndex, 0, moveItem, 'separator');
    }
  }

  const menuItems: ContextMenuEntry[] = note
    ? noteMenuItems
    : [
        { label: 'Rename', icon: <Pencil size={15} strokeWidth={1.75} />, onSelect: startRename },
        { label: 'Duplicate', icon: <CopyPlus size={15} strokeWidth={1.75} />, onSelect: duplicate },
        pinItem,
        'separator',
        { label: 'Reveal in explorer', icon: <FolderOpen size={15} strokeWidth={1.75} />, onSelect: reveal, disabled: !tile.cwd },
        { label: 'Copy path', icon: <ClipboardCopy size={15} strokeWidth={1.75} />, onSelect: copyPath, disabled: !tile.cwd },
        'separator',
        { label: 'Restart terminal', icon: <RotateCw size={15} strokeWidth={1.75} />, onSelect: restartTile },
        {
          label: elevated ? 'Restart as user' : 'Restart as administrator',
          icon: <ShieldCheck size={15} strokeWidth={1.75} />,
          onSelect: toggleElevated
        },
        { label: 'Focus', icon: <Focus size={15} strokeWidth={1.75} />, onSelect: focusTile },
        fullscreenItem,
        'separator',
        closeItem
      ];

  if (otherTabs.length > 0 && !note) {
    const moveItem: ContextMenuEntry = {
      label: 'Move to Tab',
      icon: <ArrowLeftRight size={15} strokeWidth={1.75} />,
      submenu: otherTabs.map((t) => ({
        label: t.name,
        onSelect: () => onMoveToTab(tile.id, t.id)
      }))
    };
    const termCloseIndex = menuItems.findIndex((item) => typeof item !== 'string' && item.label === 'Close');
    if (termCloseIndex !== -1) {
      menuItems.splice(termCloseIndex, 0, moveItem, 'separator');
    }
  }

  const inset = TILE_GAP / 2;
  const ek = fullscreen ? 1 : k;
  const bodyW = fullscreen ? vpW - FS_PAD * 2 : tile.width - TILE_GAP;
  const bodyH = fullscreen ? vpH - FS_PAD * 2 : tile.height - TILE_GAP;
  const sx = fullscreen ? FS_PAD : (tile.x + inset) * k + view.x;
  const sy = fullscreen ? FS_PAD : (tile.y + inset) * k + view.y;
  const z = fullscreen ? 50 : active ? 2 : 1;
  const box = fullscreen
    ? { width: bodyW, height: bodyH }
    : { width: bodyW, height: bodyH, transform: `scale(${k})`, transformOrigin: 'top left' as const };
  const term = tile.type === 'term' && live;
  const termCols = Math.max(20, Math.floor((bodyW - 8) / 7.23));
  const termRows = Math.max(2, Math.floor((bodyH - TILE_HEADER - 11) / 15));
  const anim = fullscreen ? (exiting ? styles.fsExit : styles.fsEnter) : null;
  const cls = [styles.tile, note && styles.sticky, tile.pinned && styles.pinnedTile, note && linkActive && styles.linkActive, selected && !fullscreen && styles.selected, active && !fullscreen && styles.active, anim].filter(Boolean).join(' ');
  const gone = { display: hidden ? 'none' : undefined };

  return (
    <>
      <div data-tile={tile.id} className={cls} style={{ top: sy, left: sx, zIndex: z, ...box, ...gone, ...noteTint }}>
        <div
          className={styles.header}
          onPointerUp={endDrag}
          onPointerDown={startDrag}
          onPointerMove={onDrag}
          onPointerCancel={endDrag}
          onContextMenu={openMenu}
        >
          {progress && !note && (
            <span
              className={progress.state === 2 ? `${styles.progressBar} ${styles.progressError}` : styles.progressBar}
              style={{ width: `${progress.pct}%` }}
            />
          )}
          {renaming ? (
            <input
              ref={renameRef}
              className={note ? styles.noteTitle : styles.renameInput}
              value={draft}
              placeholder={note ? 'Note' : tile.autoTitle || 'Terminal'}
              onBlur={commitRename}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={onRenameKey}
              onPointerDown={stopDrag}
              onDoubleClick={(e) => e.stopPropagation()}
              autoFocus
            />
          ) : (
            <span className={styles.title} data-empty={note && !noteTitle} onDoubleClick={startTitleEdit}>
              {claudeLive && !spinning && (
                <span className={styles.claudeMark}>
                  <ClaudeLogo size={11} />
                </span>
              )}
              <span className={styles.label}>{noteLabel ?? label}</span>
              {folder && folder !== label && (
                <span className={styles.folder} data-tooltip={tile.cwd}>
                  {folder}
                </span>
              )}
              {!note && tile.branch && (
                <button className={styles.branch} onClick={openBranches} onPointerDown={stopDrag}>
                  <GitBranch size={10} strokeWidth={2} />
                  {tile.branch}
                  {track.ahead > 0 && (
                    <span className={styles.ahead} data-tooltip={`${track.ahead} to push`}>
                      <ArrowUp size={9} strokeWidth={2.5} />
                    </span>
                  )}
                  {track.behind > 0 && (
                    <span className={styles.behind} data-tooltip={`${track.behind} to pull`}>
                      <ArrowDown size={9} strokeWidth={2.5} />
                    </span>
                  )}
                  <ChevronDown size={10} strokeWidth={2} />
                </button>
              )}
              {alert && <span className={alert === 'finished' ? `${styles.alertDot} ${styles.alertDone}` : styles.alertDot} />}
            </span>
          )}
          <div className={styles.actions}>
            {!note && diff && (
              <span className={styles.diffStat}>
                <span className={styles.diffAdd}>+{diff.a}</span>
                <span className={styles.diffDel}>-{diff.r}</span>
              </span>
            )}
            {note && (
              <button
                className={tile.renderOnly ? `${styles.action} ${styles.rawOn}` : styles.action}
                onClick={toggleRaw}
                aria-label={tile.renderOnly ? 'Show markdown on edit' : 'Rendered only'}
                data-tooltip={tile.renderOnly ? 'Rendered' : 'Live edit'}
                style={tint ? ({ ['--note-body' as string]: tint.body, ['--note-text' as string]: tint.text }) : undefined}
              >
                <span className={styles.rawGlyph}>M</span>
              </button>
            )}
            {note && (
              <button
                className={linkedTerms.length ? `${styles.action} ${styles.linked}` : styles.action}
                onPointerDown={startLinkDrag}
                aria-label="Link note to terminal"
                style={tint ? ({ ['--note-body' as string]: tint.body, ['--note-text' as string]: tint.text }) : undefined}
              >
                <span className={styles.rawGlyph}>
                  <Link2 size={12} strokeWidth={2} />
                </span>
              </button>
            )}
            {note && (
              <button className={styles.action} onClick={copyNote} aria-label="Copy note">
                <Copy size={13} strokeWidth={2} />
              </button>
            )}
            {!note && !fullscreen && (
              <button
                className={tile.pinned ? `${styles.action} ${styles.pinned}` : styles.action}
                onClick={togglePin}
                aria-label={tile.pinned ? 'Unpin tile' : 'Pin tile'}
              >
                {tile.pinned ? <PinOff size={13} strokeWidth={2} /> : <Pin size={13} strokeWidth={2} />}
              </button>
            )}
            {!note && (
              <button className={styles.action} onClick={toggleFullscreen} aria-label="Toggle fullscreen">
                {fullscreen ? <Minimize size={13} strokeWidth={2} /> : <Maximize size={13} strokeWidth={2} />}
              </button>
            )}
            {!note && !code && (
              <button className={styles.action} onClick={restartTile} aria-label="Restart terminal">
                <RotateCw size={13} strokeWidth={2} />
              </button>
            )}
            {!fullscreen && (
              <button className={`${styles.action} ${styles.close}`} onClick={closeTile} aria-label="Close tile">
                <X size={14} strokeWidth={2} />
              </button>
            )}
          </div>
        </div>
        <div className={styles.body} onContextMenu={note ? (e) => openMenu(e, true) : undefined}>
          {note && (
            <NoteTile tile={tile} wsId={wsId} active={active} onChange={onNoteChange} onActivate={onActivate} onEditor={onNoteEditor} />
          )}
          {code && tile.cwd && tile.filePath && <DiffViewer root={tile.cwd} file={tile.filePath} embedded />}
          {!note && !code && !term && <div className={styles.placeholder}>{tile.type !== 'term' ? label : ''}</div>}
        </div>
      </div>
      {term && (
        <div
          data-tile={tile.id}
          className={anim ? `${styles.termLayer} ${anim}` : styles.termLayer}
          style={{
            top: devicePx(sy + (TILE_HEADER + 4) * ek),
            left: devicePx(sx + 4 * ek),
            width: devicePx((bodyW - 8) * ek),
            height: devicePx((bodyH - TILE_HEADER - 5) * ek),
            zIndex: z,
            ...gone
          }}
        >
          <GridTerminal
            k={ek}
            cwd={tile.cwd}
            onCwd={onCwd}
            onOscTitle={onOscTitle}
            onClaudeActive={setClaudeLive}
            onClaudeStatus={onClaudeStatus}
            onClaudeDiff={onClaudeDiff}
            onProgress={onProgress}
            restartKey={restartKey}
            elevated={elevated}
            active={active}
            visible={visible && !hidden}
            tileId={tile.id}
            cols={termCols}
            rows={termRows}
            onContextMenu={openMenu}
          />
        </div>
      )}
      {!fullscreen && (
        <div data-tile={tile.id} className={styles.handles} style={{ top: sy, left: sx, zIndex: z, ...box, ...gone }}>
          {HANDLES.map((dir) => (
            <div
              key={dir}
              data-dir={dir}
              className={styles.handle}
              onPointerUp={endResize}
              onPointerMove={onResizeMove}
              onPointerCancel={endResize}
              onPointerDown={startResize(dir)}
            />
          ))}
        </div>
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems} onClose={closeMenu} />}
      {branchLocal && tile.cwd && (
        <BranchMenu
          k={ek}
          cwd={tile.cwd}
          anchor={{ x: sx + branchLocal.x * ek, y: sy + branchLocal.y * ek }}
          zIndex={z}
          snapshot={branches.snapshot}
          loading={branches.loading}
          error={branches.error}
          onClose={closeBranches}
          onSnapshot={branches.setSnapshot}
          onError={branches.setError}
        />
      )}
    </>
  );
};

export default TileFrame;
