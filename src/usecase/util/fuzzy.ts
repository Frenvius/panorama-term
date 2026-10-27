const SEPARATORS = '/-_.';

const isWordStart = (text: string, i: number): boolean => {
  if (i === 0 || SEPARATORS.includes(text[i - 1])) return true;
  const ch = text[i];
  const before = text[i - 1];
  return ch !== ch.toLowerCase() && before === before.toLowerCase();
};

export const fuzzyMatch = (query: string, text: string): number[] | null => {
  const needle = query.replace(/\s+/g, '').toLowerCase();
  if (!needle) return [];
  const hay = text.toLowerCase();

  const walk = (qi: number, from: number, prev: number): number[] | null => {
    if (qi === needle.length) return [];
    const ch = needle[qi];
    for (let i = from; i < hay.length; i++) {
      if (hay[i] !== ch) continue;
      const allowed = qi === 0 || i === prev + 1 || SEPARATORS.includes(ch) || isWordStart(text, i);
      if (!allowed) continue;
      const rest = walk(qi + 1, i + 1, i);
      if (rest) return [i, ...rest];
    }
    return null;
  };

  return walk(0, 0, -1);
};
