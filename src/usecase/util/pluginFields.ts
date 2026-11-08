export const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export const isText = (value: unknown): value is string => typeof value === 'string' && value.trim() !== '';

export const isInside = (path: string): boolean =>
  !path.startsWith('/') && !path.includes('\\') && !path.includes(':') && path.split('/').every((p) => p && p !== '..');
