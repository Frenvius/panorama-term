import React from 'react';
import { RotateCcw } from 'lucide-react';

import { getBinding, setBinding, formatCombo, resetBinding, setCapturing, comboFromEvent, type CommandId } from '~/usecase/util/keybindings';

import styles from '../styles.module.scss';

interface ShortcutRowProps {
  id: CommandId;
  label: string;
  defaultCombo: string;
}

const ShortcutRow = ({ id, label, defaultCombo }: ShortcutRowProps) => {
  const [combo, setCombo] = React.useState(() => getBinding(id));
  const [listening, setListening] = React.useState(false);

  React.useEffect(() => {
    if (!listening) return;
    setCapturing(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setListening(false);
        return;
      }
      const next = comboFromEvent(e);
      if (!next) return;
      setCombo(next);
      void setBinding(id, next);
      setListening(false);
    };
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      setCapturing(false);
    };
  }, [listening, id]);

  const startListening = () => setListening(true);

  const reset = () => {
    void resetBinding(id);
    setCombo(defaultCombo);
  };

  return (
    <div className={styles.shortcutRow}>
      <span className={styles.shortcutLabel}>{label}</span>
      <div className={styles.shortcutKeys}>
        {combo !== defaultCombo && (
          <button className={styles.shortcutReset} onClick={reset} aria-label="Reset to default">
            <RotateCcw size={13} strokeWidth={2} />
          </button>
        )}
        <button
          onClick={startListening}
          className={`${styles.shortcutCombo} ${listening ? styles.listening : ''}`}
        >
          {listening ? 'Press keys...' : formatCombo(combo)}
        </button>
      </div>
    </div>
  );
};

export default ShortcutRow;
