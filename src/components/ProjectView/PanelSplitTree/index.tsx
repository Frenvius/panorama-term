import type { PanelSplitTreeProps } from '../types';
import type { PanelSplitNode, PanelLayoutNode } from '~/domain/interfaces/projectView.interface';

import React from 'react';

import styles from './styles.module.scss';
import PanelGroupView from '../PanelGroupView';

const MIN_RATIO = 0.1;
const MAX_RATIO = 0.9;

interface SplitBranchProps {
  path: number[];
  node: PanelSplitNode;
  onResizeSplit: (path: number[], ratio: number) => void;
  renderNode: (node: PanelLayoutNode, path: number[]) => React.ReactNode;
}

const SplitBranch = (props: SplitBranchProps) => {
  const { node, path, onResizeSplit, renderNode } = props;
  const containerRef = React.useRef<HTMLDivElement>(null);
  const firstRef = React.useRef<HTMLDivElement>(null);
  const secondRef = React.useRef<HTMLDivElement>(null);
  const dragRef = React.useRef<{ pointerId: number; ratio: number } | null>(null);
  const [dragging, setDragging] = React.useState(false);

  const ratio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, node.ratio));

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
    const raw =
      node.direction === 'horizontal'
        ? rect.width > 0
          ? (e.clientX - rect.left) / rect.width
          : 0.5
        : rect.height > 0
          ? (e.clientY - rect.top) / rect.height
          : 0.5;
    const nextRatio = Math.min(MAX_RATIO, Math.max(MIN_RATIO, raw));
    drag.ratio = nextRatio;
    if (firstRef.current) firstRef.current.style.flex = `${nextRatio} 1 0%`;
    if (secondRef.current) secondRef.current.style.flex = `${1 - nextRatio} 1 0%`;
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
      <div ref={firstRef} className={styles.pane} style={{ flex: `${ratio} 1 0%` }}>
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

      <div ref={secondRef} className={styles.pane} style={{ flex: `${1 - ratio} 1 0%` }}>
        {renderNode(node.second, [...path, 1])}
      </div>
    </div>
  );
};

const PanelSplitTree = (props: PanelSplitTreeProps) => {
  const { node, groups, activeGroupId, maximizedGroupId, groupCount, sharedState, handlers } = props;

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
    return <SplitBranch node={currentNode} path={path} onResizeSplit={handlers.onResizeSplit} renderNode={renderNode} />;
  };

  if (maximizedGroupId && groups[maximizedGroupId]) {
    return <div className={styles.treeRoot}>{renderGroup(maximizedGroupId, true)}</div>;
  }

  return <div className={styles.treeRoot}>{renderNode(node, [])}</div>;
};

export default PanelSplitTree;
