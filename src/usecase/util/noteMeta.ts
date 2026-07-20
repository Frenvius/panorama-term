const FM = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*\r?\n?/;

export const stripFrontmatter = (raw: string): string => {
  const m = FM.exec(raw);
  return m ? raw.slice(m[0].length) : raw;
};

export const parseFrontTitle = (raw?: string): string => {
  if (!raw) return '';
  const m = FM.exec(raw);
  if (!m) return '';
  const line = m[1].split(/\r?\n/).find((l) => /^title\s*:/.test(l.trim()));
  if (!line) return '';
  let v = line.trim().replace(/^title\s*:\s*/, '').trim();
  if (v.startsWith('"')) {
    try {
      v = JSON.parse(v) as string;
    } catch {
      /* keep raw */
    }
  } else if (v.startsWith("'") && v.endsWith("'")) {
    v = v.slice(1, -1).replace(/''/g, "'");
  }
  return v;
};

export const applyFrontTitle = (raw: string, title: string): string => {
  const body = stripFrontmatter(raw);
  const t = title.trim();
  if (!t) return body;
  const needQuote = /[:#"'\n]|^\s|\s$/.test(t);
  const val = needQuote ? JSON.stringify(t) : t;
  return `---\ntitle: ${val}\n---\n${body}`;
};
