export interface NoteColor {
  body: string;
  text: string;
}

export const NOTE_PALETTE: NoteColor[] = [
  { body: '#fef8c4', text: '#5c4036' },
  { body: '#f8bad0', text: '#880e4e' },
  { body: '#badefa', text: '#0c46a0' },
  { body: '#c8e6c8', text: '#1a5e20' },
  { body: '#fee0b2', text: '#e44400' },
  { body: '#e0bee6', text: '#4a148c' },
  { body: '#2c2c2c', text: '#fefefe' },
  { body: '#36464e', text: '#fefefe' }
];

export const NOTE_DEFAULT_COLOR = NOTE_PALETTE[0].body;

export const noteTextColor = (hex: string): string => {
  const found = NOTE_PALETTE.find((p) => p.body.toLowerCase() === hex.toLowerCase());
  if (found) return found.text;
  const c = hex.replace('#', '');
  if (c.length < 6) return '#1a1a1a';
  const r = parseInt(c.slice(0, 2), 16);
  const g = parseInt(c.slice(2, 4), 16);
  const b = parseInt(c.slice(4, 6), 16);
  const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  return lum > 0.55 ? '#1a1a1a' : '#f5f5f5';
};
