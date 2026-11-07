import type { BackgroundShell } from '~/domain/interfaces/pty.interface';

import React from 'react';

import { formatElapsed } from '~/usecase/util/elapsed';
import { useFileTail } from '~/usecase/hooks/useFileTail';

import styles from './styles.module.scss';

interface ShellsProps {
  open: boolean;
  shells: BackgroundShell[];
}

const STICK_PX = 8;

const Shells = ({ open, shells }: ShellsProps) => {
  const [now, setNow] = React.useState(Date.now);
  const [pickedId, setPickedId] = React.useState('');
  const outRef = React.useRef<HTMLPreElement>(null);
  const stickRef = React.useRef(true);

  const selected = shells.find((s) => s.id === pickedId) ?? shells[0];
  const { text, error } = useFileTail(open ? selected?.outputFile : undefined);

  React.useEffect(() => {
    if (!open) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [open]);

  React.useLayoutEffect(() => {
    const el = outRef.current;
    if (el && stickRef.current) el.scrollTop = el.scrollHeight;
  }, [text]);

  const handleScroll = () => {
    const el = outRef.current;
    if (el) stickRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_PX;
  };

  const pick = (id: string) => () => {
    stickRef.current = true;
    setPickedId(id);
  };

  return (
    <div className={styles.collapse} data-open={open} inert={!open}>
      <div className={styles.inner}>
        <div className={styles.panel} aria-label="Background shells" data-shells>
          {shells.map((shell) => (
            <button
              type="button"
              key={shell.id}
              className={styles.row}
              onClick={pick(shell.id)}
              aria-pressed={shell.id === selected?.id}
              title={shell.description ?? shell.command}
            >
              <span className={styles.dot} />
              <span className={styles.command}>{shell.command}</span>
              <span className={styles.elapsed}>{formatElapsed(now - shell.startedAt)}</span>
            </button>
          ))}
          <pre ref={outRef} className={styles.output} onScroll={handleScroll}>
            {selected?.outputFile ? error ?? text : 'Output file unknown for this shell.'}
          </pre>
          {selected?.outputFile && <div className={styles.path}>{selected.outputFile}</div>}
        </div>
      </div>
    </div>
  );
};

export default Shells;
