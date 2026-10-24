import React from 'react';
import { Zap, Check, ChevronRight, TriangleAlert } from 'lucide-react';

import { insideSafeTriangle } from '~/usecase/util/submenuAim';
import { MODEL_CONTEXT_VARIANTS } from '~/components/Terminal/AgentBar/constants';
import { findModel, baseModelId, contextLabel, hasOptional1M } from '~/components/Terminal/AgentBar/model';

import type { CatalogModel } from '~/domain/interfaces/pty.interface';

import styles from '~/components/Terminal/AgentBar/styles.module.scss';

interface ModelMenuProps {
  current: string;
  catalog: CatalogModel[];
  onFast: () => void;
  onPick: (id: string) => void;
}

const AIM_GRACE_MS = 400;

const ModelMenu = ({ current, catalog, onFast, onPick }: ModelMenuProps) => {
  const [moreOpen, setMoreOpen] = React.useState(false);
  const flyoutRef = React.useRef<HTMLDivElement>(null);
  const stopAim = React.useRef<(() => void) | null>(null);

  const cancelAim = () => {
    stopAim.current?.();
    stopAim.current = null;
  };

  React.useEffect(() => cancelAim, []);

  const base = baseModelId(current);
  const suffix = current.includes('[1m]') ? '[1m]' : '';
  const entry = findModel(catalog, base);
  const main = catalog.filter((m) => m.section === 'main');
  const more = catalog.filter((m) => m.section === 'more');
  const currentInMore = more.some((m) => m.id === base);
  const fixedContext = hasOptional1M(base) ? undefined : contextLabel(entry, false);

  const openMore = () => {
    cancelAim();
    setMoreOpen(true);
  };

  const closeMore = () => {
    cancelAim();
    setMoreOpen(false);
  };

  const leaveMore = (e: React.MouseEvent) => {
    const rect = flyoutRef.current?.getBoundingClientRect();
    if (!rect) return closeMore();
    const origin = { x: e.clientX, y: e.clientY };
    let timer = setTimeout(closeMore, AIM_GRACE_MS);
    const onMove = (ev: MouseEvent) => {
      if (!insideSafeTriangle(origin, { x: ev.clientX, y: ev.clientY }, rect)) return closeMore();
      clearTimeout(timer);
      timer = setTimeout(closeMore, AIM_GRACE_MS);
    };
    cancelAim();
    document.addEventListener('mousemove', onMove);
    stopAim.current = () => {
      clearTimeout(timer);
      document.removeEventListener('mousemove', onMove);
    };
  };

  const toggleMore = () => setMoreOpen((o) => !o);

  const renderModel = (m: CatalogModel) => (
    <button
      key={m.id}
      type="button"
      data-tooltip={m.notice}
      className={styles.menuItem}
      onClick={() => onPick(hasOptional1M(m.id) ? `${m.id}${suffix}` : m.id)}
    >
      <span className={styles.menuTick}>{m.id === base && <Check size={10} />}</span>
      {m.name}
      {m.notice && <TriangleAlert size={10} className={styles.menuNotice} />}
    </button>
  );

  return (
    <div className={styles.menu}>
      {main.map(renderModel)}
      {more.length > 0 && (
        <div className={styles.menuMore} onMouseEnter={openMore} onMouseLeave={leaveMore}>
          <button type="button" aria-expanded={moreOpen} className={styles.menuItem} onClick={toggleMore}>
            <span className={styles.menuTick}>{currentInMore && <Check size={10} />}</span>
            More models
            <ChevronRight size={11} className={styles.menuChevron} />
          </button>
          {moreOpen && (
            <div ref={flyoutRef} className={`${styles.menu} ${styles.menuFlyout}`}>
              {more.map(renderModel)}
            </div>
          )}
        </div>
      )}
      {(hasOptional1M(base) || fixedContext || entry?.fast) && <div className={styles.menuDivider} />}
      {hasOptional1M(base) &&
        MODEL_CONTEXT_VARIANTS.map((v) => (
          <button key={v.title} type="button" className={styles.menuItem} onClick={() => onPick(`${base}${v.suffix}`)}>
            <span className={styles.menuTick}>{v.suffix === suffix && <Check size={10} />}</span>
            {v.title}
          </button>
        ))}
      {fixedContext && (
        <div className={`${styles.menuItem} ${styles.menuInfo}`}>
          <span className={styles.menuTick} />
          {fixedContext} context
        </div>
      )}
      {entry?.fast && (
        <button type="button" className={styles.menuItem} onClick={onFast}>
          <span className={styles.menuTick}>
            <Zap size={10} />
          </span>
          Toggle fast mode
        </button>
      )}
    </div>
  );
};

export default ModelMenu;
