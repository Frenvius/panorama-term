export interface KeyEvent {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}

const CSI_FINAL: Record<string, string> = {
  ArrowUp: 'A',
  ArrowDown: 'B',
  ArrowRight: 'C',
  ArrowLeft: 'D',
  Home: 'H',
  End: 'F'
};

const TILDE: Record<string, string> = {
  Insert: '2',
  Delete: '3',
  PageUp: '5',
  PageDown: '6'
};

export const keyToBytes = (e: KeyEvent): string | null => {
  const { key, ctrlKey, altKey, metaKey, shiftKey } = e;
  if (metaKey) return null;
  const mod = 1 + (shiftKey ? 1 : 0) + (altKey ? 2 : 0) + (ctrlKey ? 4 : 0);

  if (key === 'Backspace') {
    if (ctrlKey) return '\x08';
    if (altKey) return '\x1b\x7f';
    return '\x7f';
  }
  if (key === 'Delete' && ctrlKey) return '\x1bd';
  if (key === 'Tab') return shiftKey ? '\x1b[Z' : '\t';
  if (key === 'Enter') return shiftKey && !ctrlKey && !altKey ? '\x1b\r' : '\r';
  if (key === 'Escape') return '\x1b';

  const fin = CSI_FINAL[key];
  if (fin) return mod === 1 ? `\x1b[${fin}` : `\x1b[1;${mod}${fin}`;

  const tilde = TILDE[key];
  if (tilde) return mod === 1 ? `\x1b[${tilde}~` : `\x1b[${tilde};${mod}~`;

  if (ctrlKey && !altKey && key.length === 1) {
    const u = key.toUpperCase();
    if (u >= 'A' && u <= 'Z') return String.fromCharCode(u.charCodeAt(0) - 64);
    if (u === ' ') return '\x00';
  }

  if (key.length === 1 && !ctrlKey) return altKey ? '\x1b' + key : key;
  return null;
};
