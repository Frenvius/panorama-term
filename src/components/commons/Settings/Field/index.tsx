import React from 'react';

import styles from './styles.module.scss';

interface InputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  type?: string;
  disabled?: boolean;
  readOnly?: boolean;
  ariaLabel?: string;
  onFocus?: React.FocusEventHandler<HTMLInputElement>;
}

interface TextareaProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  readOnly?: boolean;
  ariaLabel?: string;
  onFocus?: React.FocusEventHandler<HTMLTextAreaElement>;
}

interface FieldProps {
  label?: string;
  hint?: string;
  error?: string;
  children: React.ReactNode;
}

const Field = ({ label, hint, error, children }: FieldProps) => (
  <div className={styles.field}>
    {label && <span className={styles.label}>{label}</span>}
    {children}
    {error && <span className={styles.error}>{error}</span>}
    {!error && hint && <span className={styles.hint}>{hint}</span>}
  </div>
);

const Input = ({ value, onChange, placeholder, type = 'text', disabled, readOnly, ariaLabel, onFocus }: InputProps) => {
  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => onChange(e.target.value);
  return (
    <input
      type={type}
      value={value}
      onChange={handleChange}
      placeholder={placeholder}
      disabled={disabled}
      readOnly={readOnly}
      aria-label={ariaLabel}
      className={styles.input}
      onFocus={onFocus}
    />
  );
};

const Textarea = ({ value, onChange, placeholder, disabled, readOnly, ariaLabel, onFocus }: TextareaProps) => {
  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => onChange(e.target.value);
  return (
    <textarea
      value={value}
      onChange={handleChange}
      placeholder={placeholder}
      disabled={disabled}
      readOnly={readOnly}
      aria-label={ariaLabel}
      className={styles.textarea}
      onFocus={onFocus}
    />
  );
};

export { Input, Textarea };
export default Field;
