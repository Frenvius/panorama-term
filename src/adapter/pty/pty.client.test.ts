import { describe, test, expect } from 'bun:test';

import { parseGridFrame } from '~/adapter/pty/pty.client';

const buildFrame = (rows: number, cols: number, text: string, topLine: number, alt: boolean): ArrayBuffer => {
  const textBytes = new TextEncoder().encode(text);
  const attrCount = rows * cols * 2;
  const attrBytes = attrCount * 4;
  const totalBytes = 17 + textBytes.length + attrBytes + 8;
  const buf = new ArrayBuffer(totalBytes);
  const dv = new DataView(buf);
  const u8 = new Uint8Array(buf);

  u8[0] = 1;
  dv.setUint16(1, rows, true);
  dv.setUint16(3, cols, true);
  dv.setUint32(5, 0, true);
  u8[9] = 0;
  u8[10] = 0;
  dv.setUint16(11, 0, true);
  dv.setUint32(13, textBytes.length, true);
  u8.set(textBytes, 17);

  const trailerStart = 17 + textBytes.length + attrBytes;
  dv.setUint32(trailerStart, topLine, true);
  u8[trailerStart + 4] = alt ? 1 : 0;

  return buf;
};

describe('parseGridFrame', () => {
  test('reads topLine and alt from trailer', () => {
    const rows = 2;
    const cols = 4;
    const text = 'aaaa\nbbbb';
    const frame = parseGridFrame(buildFrame(rows, cols, text, 42, true));
    expect(frame).not.toBeNull();
    expect(frame!.topLine).toBe(42);
    expect(frame!.alt).toBe(true);
  });

  test('defaults topLine to 0 and alt to false when no trailer', () => {
    const rows = 1;
    const cols = 2;
    const textBytes = new TextEncoder().encode('ab');
    const attrBytes = rows * cols * 2 * 4;
    const buf = new ArrayBuffer(17 + textBytes.length + attrBytes);
    const dv = new DataView(buf);
    const u8 = new Uint8Array(buf);
    u8[0] = 1;
    dv.setUint16(1, rows, true);
    dv.setUint16(3, cols, true);
    dv.setUint32(5, 0, true);
    dv.setUint32(13, textBytes.length, true);
    u8.set(textBytes, 17);
    const frame = parseGridFrame(buf);
    expect(frame).not.toBeNull();
    expect(frame!.topLine).toBe(0);
    expect(frame!.alt).toBe(false);
  });
});
