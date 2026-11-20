import type { LucideIcon } from 'lucide-react';

import React from 'react';

import styles from './styles.module.scss';

interface ButtonProps {
  children: React.ReactNode;
  onClick?: () => void;
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'md' | 'sm';
  icon?: LucideIcon;
  disabled?: boolean;
  busy?: boolean;
  busyLabel?: string;
  type?: 'button' | 'submit';
}

const Button = ({
  children,
  onClick,
  type = 'button',
  size = 'md',
  variant = 'secondary',
  icon: Icon,
  disabled,
  busy,
  busyLabel
}: ButtonProps) => (
  <button
    type={type}
    onClick={onClick}
    disabled={disabled || busy}
    className={`${styles.btn} ${styles[variant]} ${size === 'sm' ? styles.sm : ''}`}
  >
    {Icon && <Icon size={size === 'sm' ? 12 : 14} strokeWidth={1.75} />}
    {busy && busyLabel ? busyLabel : children}
  </button>
);

export default Button;
