import type { LucideIcon } from 'lucide-react';

import React from 'react';

import styles from './styles.module.scss';

export interface CardButtonProps {
  label: string;
  icon: LucideIcon;
  hint?: string;
  disabled?: boolean;
  tone?: 'play' | 'stop' | 'accept';
  onClick: (e: React.MouseEvent) => void;
}

const CardButton = ({ label, icon: Icon, hint, disabled, tone, onClick }: CardButtonProps) => (
  <button
    type="button"
    data-tone={tone}
    aria-label={label}
    disabled={disabled}
    onClick={onClick}
    title={hint ?? label}
    className={styles.button}
  >
    <Icon size={13} strokeWidth={1.75} />
  </button>
);

export default CardButton;
