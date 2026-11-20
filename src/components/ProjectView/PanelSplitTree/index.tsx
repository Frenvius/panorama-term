import type { PanelSplitTreeProps } from '../types';
import type { PanelSplitNode, PanelLayoutNode } from '~/domain/interfaces/projectView.interface';

import React from 'react';

import styles from './styles.module.scss';
import PanelGroupView from '../PanelGroupView';
import { fitCells, TERM_ROW_PX, PANEL_CHROME_W, PANEL_CHROME_H, PANEL_TOOLBAR_H, layoutSplit } from '~/usecase/util/termGrid';
import { useTermCellWidth } from '~/usecase/hooks/useTermCellWidth';

const MIN_RATIO = 0.1;
const MAX_RATIO = 0.9;

interface SplitBranchProps {
  path: number[];
  node: PanelSplitNode;
  cellW: number;
  hasToolbar: (groupId: string) => boolean;
  onResizeSplit: (path: number[], ratio: number) => void;
  renderNode: (node: PanelLayoutNode, path: number[]) => React.ReactNode;
}

const SplitBranch = (props: SplitBranchProps) => {
  const { node, path, cellW, hasToolbar, onResizeSplit, renderNode } = props;
  const containerRef = React.useRef<HTMLDivElement>(null);
  const firstRef = React.useRef<HTMLDivElement>(null);
  const secondRef = React.useRef<HTMLDivElement>(null);
  const dragRef = React.useRef<{ pointerId: number; ratio: number } | null>(null);
  const [dragging, setDragging] = React.useState(false);
  const [containerPx, setContainerPx] = React.useState<number | null>(null);

  const ratio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, node.ratio));

  React.useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const size = node.direction === 'horizontal' ? entry.contentRect.width : entry.contentRect.height;
      if (size > 0) setContainerPx(size);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [node.direction]);

  const step = node.direction === 'horizontal' ? cellW : TERM_ROW_PX;

  const chromeOf = (child: PanelLayoutNode): number => {
    if (node.direction === 'horizontal') return PANEL_CHROME_W;
    if (child.type === 'leaf') return PANEL_CHROME_H + (hasToolbar(child.groupId) ? PANEL_TOOLBAR_H : 0);
    return PANEL_CHROME_H;
  };

  const layout = React.useMemo(() => {
    if (containerPx === null) return null;
    return layoutSplit({
      ratio,
      available: containerPx,
      firstChrome: chromeOf(node.first),
      secondChrome: chromeOf(node.second),
      step
    });
  }, [containerPx, ratio, cellW, node.direction, node.first, node.second, hasToolbar]);

  const firstStyle = layout ? { flex: `0 0 ${layout.first}px` } : { flex: `${ratio} 1 0%` };
  const secondStyle = layout ? { flex: `0 0 ${layout.second}px` } : { flex: `${1 - ratio} 1 0%` };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    dragRef.current = { pointerId: e.pointerId, ratio };
    setDragging(true);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    const container = containerRef.current;
    if (!drag || !container) return;
    const rect = container.getBoundingClientRect();
    const containerSize = node.direction === 'horizontal' ? rect.width : rect.height;
    if (containerSize <= 0) return;
    const rawFirst = node.direction === 'horizontal' ? e.clientX - rect.left : e.clientY - rect.top;
    const firstChrome = chromeOf(node.first);
    const secondChrome = chromeOf(node.second);
    const clampedRatio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, rawFirst / (containerSize - 8)));
    const next = layoutSplit({ ratio: clampedRatio, available: containerSize, firstChrome, secondChrome, step });
    drag.ratio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, next.first / (containerSize - 8)));
    if (firstRef.current) firstRef.current.style.flex = `0 0 ${next.first}px`;
    if (secondRef.current) secondRef.current.style.flex = `0 0 ${next.second}px`;
  };

  const endDrag = () => {
    const drag = dragRef.current;
    if (!drag) return;
    dragRef.current = null;
    setDragging(false);
    onResizeSplit(path, drag.ratio);
  };

  const resetSplitRatio = () => {
    onResizeSplit(path, 0.5);
  };

  return (
    <div ref={containerRef} className={styles.splitContainer} data-direction={node.direction}>
      <div ref={firstRef} className={styles.pane} style={firstStyle}>
        {renderNode(node.first, [...path, 0])}
      </div>

      <div
        role="separator"
        aria-orientation={node.direction === 'horizontal' ? 'vertical' : 'horizontal'}
        className={dragging ? `${styles.divider} ${styles.dividerDragging}` : styles.divider}
        data-direction={node.direction}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onDoubleClick={resetSplitRatio}
      />

      <div ref={secondRef} className={styles.pane} style={secondStyle}>
        {renderNode(node.second, [...path, 1])}
      </div>
    </div>
  );
};

const PanelSplitTree = (props: PanelSplitTreeProps) => {
  const { node, groups, activeGroupId, maximizedGroupId, groupCount, sharedState, handlers } = props;
  const cellW = useTermCellWidth();

  const hasToolbar = React.useCallback(
    (groupId: string): boolean => {
      const group = groups[groupId];
      if (!group) return false;
      return group.tileIds.some((id) => sharedState.tileMap.has(id));
    },
    [groups, sharedState.tileMap]
  );

  const rootRef = React.useRef<HTMLDivElement>(null);
  const [rootPx, setRootPx] = React.useState<{ w: number; h: number } | null>(null);

  React.useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      if (rect && rect.width > 0 && rect.height > 0) setRootPx({ w: rect.width, h: rect.height });
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const firstLeafOf = (n: PanelLayoutNode): string => (n.type === 'leaf' ? n.groupId : firstLeafOf(n.first));

  const exactWidth = (w: number) => PANEL_CHROME_W + Math.max(2, fitCells(w, PANEL_CHROME_W, cellW)) * cellW;

  const exactHeight = (h: number, groupId: string) => {
    const chrome = PANEL_CHROME_H + (hasToolbar(groupId) ? PANEL_TOOLBAR_H : 0);
    return chrome + Math.max(2, fitCells(h, chrome, TERM_ROW_PX)) * TERM_ROW_PX;
  };

  const rootStyle = (root: PanelLayoutNode): React.CSSProperties | undefined => {
    if (!rootPx) return undefined;
    const leaf = firstLeafOf(root);
    const sizeW = root.type === 'leaf' || root.direction === 'vertical';
    const sizeH = root.type === 'leaf' || root.direction === 'horizontal';
    return {
      flex: '0 0 auto',
      width: sizeW ? exactWidth(rootPx.w) : '100%',
      height: sizeH ? exactHeight(rootPx.h, leaf) : '100%'
    };
  };

  const renderGroup = (groupId: string, isMaximized: boolean) => {
    const group = groups[groupId];
    if (!group) return null;
    return (
      <PanelGroupView
        state={{
          ...sharedState,
          group,
          isFocused: group.id === activeGroupId,
          isMaximized,
          canCloseGroup: groupCount > 1
        }}
        handlers={handlers}
      />
    );
  };

  const renderNode = (currentNode: PanelLayoutNode, path: number[]): React.ReactNode => {
    if (currentNode.type === 'leaf') {
      return renderGroup(currentNode.groupId, false);
    }
    return (
      <SplitBranch
        node={currentNode}
        path={path}
        cellW={cellW}
        hasToolbar={hasToolbar}
        onResizeSplit={handlers.onResizeSplit}
        renderNode={renderNode}
      />
    );
  };

  const maximized = maximizedGroupId && groups[maximizedGroupId] ? maximizedGroupId : null;
  const root: PanelLayoutNode = maximized ? { type: 'leaf', groupId: maximized } : node;

  return (
    <div ref={rootRef} className={styles.treeRoot}>
      <div className={styles.exactRoot} style={rootStyle(root)}>
        {maximized ? renderGroup(maximized, true) : renderNode(node, [])}
      </div>
    </div>
  );
};

export default PanelSplitTree;
