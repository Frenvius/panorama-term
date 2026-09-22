import React from 'react';

import styles from './styles.module.scss';

const GAP = 8;

const Tooltip = () => {
  React.useEffect(() => {
    const el = document.createElement('div');
    el.className = styles.tooltip;
    document.body.appendChild(el);
    let active: Element | null = null;

    const show = (target: Element) => {
      if (target === active) return;
      active = target;
      const label = (target as HTMLElement).dataset.tooltip;
      if (!label) return;
      const shortcut = (target as HTMLElement).dataset.shortcut;

      el.replaceChildren();
      const span = document.createElement('span');
      span.textContent = label;
      el.appendChild(span);
      if (shortcut) {
        const kbd = document.createElement('kbd');
        kbd.textContent = shortcut;
        el.appendChild(kbd);
      }

      const rect = target.getBoundingClientRect();
      const vw = window.innerWidth;
      el.classList.remove(styles.visible);
      const tw = el.offsetWidth;
      const th = el.offsetHeight;

      const forced = (target as HTMLElement).dataset.tooltipPlace;

      if (forced === 'top') {
        el.style.left = `${Math.max(GAP, Math.min(rect.left + (rect.width - tw) / 2, vw - tw - GAP))}px`;
        el.style.top = `${rect.top - th - GAP}px`;
      } else if (forced === 'bottom') {
        el.style.left = `${Math.max(GAP, Math.min(rect.left + (rect.width - tw) / 2, vw - tw - GAP))}px`;
        el.style.top = `${rect.bottom + GAP}px`;
      } else if (rect.left < vw * 0.25) {
        el.style.left = `${rect.right + GAP}px`;
        el.style.top = `${rect.top + (rect.height - th) / 2}px`;
      } else if (rect.right > vw * 0.75) {
        el.style.left = `${rect.left - tw - GAP}px`;
        el.style.top = `${rect.top + (rect.height - th) / 2}px`;
      } else {
        el.style.left = `${rect.left + (rect.width - tw) / 2}px`;
        el.style.top = `${rect.bottom + GAP}px`;
      }

      requestAnimationFrame(() => el.classList.add(styles.visible));
    };

    const hide = () => {
      active = null;
      el.classList.remove(styles.visible);
    };

    const onEnter = (e: Event) => {
      if (!(e.target instanceof Element)) return;
      const target = e.target.closest('[data-tooltip]');
      if (target) show(target);
    };

    const onLeave = (e: Event) => {
      if (!(e.target instanceof Element)) return;
      const leaving = e.target.closest('[data-tooltip]');
      if (!leaving) return;
      const related = (e as MouseEvent).relatedTarget;
      const entering = related instanceof Element ? related.closest('[data-tooltip]') : null;
      if (entering === leaving) return;
      hide();
    };

    document.addEventListener('mouseenter', onEnter, true);
    document.addEventListener('mouseleave', onLeave, true);
    document.addEventListener('mousedown', hide, true);

    return () => {
      document.removeEventListener('mouseenter', onEnter, true);
      document.removeEventListener('mouseleave', onLeave, true);
      document.removeEventListener('mousedown', hide, true);
      el.remove();
    };
  }, []);

  return null;
};

export default Tooltip;
