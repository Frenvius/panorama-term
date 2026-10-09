import React from 'react';
import { Check, ChevronRight } from 'lucide-react';

import { MODEL_CONTEXT_VARIANTS } from '~/components/Terminal/AgentBar/constants';

import type { CatalogModel } from '~/domain/interfaces/pty.interface';

import styles from '~/components/Terminal/AgentBar/styles.module.scss';

interface ModelMenuProps {
  current: string;
  catalog: CatalogModel[];
  onPick: (id: string) => void;
}

const baseOf = (id: string): string => id.replace(/\[[^\]]*\]/, '');

const ModelMenu = ({ current, catalog, onPick }: ModelMenuProps) => {
  const [moreOpen, setMoreOpen] = React.useState(false);

  const base = baseOf(current);
  const suffix = current.includes('[1m]') ? '[1m]' : '';
  const main = catalog.filter((m) => m.section === 'main');
  const more = catalog.filter((m) => m.section === 'more');
  const currentInMore = more.some((m) => m.id === base);

  const openMore = () => setMoreOpen(true);
  const closeMore = () => setMoreOpen(false);
  const toggleMore = () => setMoreOpen((o) => !o);

  const renderModel = (m: CatalogModel) => (
    <button key={m.id} type="button" className={styles.menuItem} onClick={() => onPick(`${m.id}${suffix}`)}>
      <span className={styles.menuTick}>{m.id === base && <Check size={10} />}</span>
      {m.name}
    </button>
  );

  return (
    <div className={styles.menu}>
      {main.map(renderModel)}
      {more.length > 0 && (
        <div className={styles.menuMore} onMouseEnter={openMore} onMouseLeave={closeMore}>
          <button type="button" aria-expanded={moreOpen} className={styles.menuItem} onClick={toggleMore}>
            <span className={styles.menuTick}>{currentInMore && <Check size={10} />}</span>
            More models
            <ChevronRight size={11} className={styles.menuChevron} />
          </button>
          {moreOpen && <div className={`${styles.menu} ${styles.menuFlyout}`}>{more.map(renderModel)}</div>}
        </div>
      )}
      <div className={styles.menuDivider} />
      {MODEL_CONTEXT_VARIANTS.map((v) => (
        <button key={v.title} type="button" className={styles.menuItem} onClick={() => onPick(`${base}${v.suffix}`)}>
          <span className={styles.menuTick}>{v.suffix === suffix && <Check size={10} />}</span>
          {v.title}
        </button>
      ))}
    </div>
  );
};

export default ModelMenu;
