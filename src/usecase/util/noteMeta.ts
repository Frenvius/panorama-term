const FM = /^---\r?\n([\s\S]*?)\r?\n---[ \t]*\r?\n?/;

export const stripFrontmatter = (raw: string): string => {
  const m = FM.exec(raw);
  return m ? raw.slice(m[0].length) : raw;
};

export const withBody = (raw: string, body: string): string => {
  const m = FM.exec(raw);
  if (!m) return body;
  return m[0].endsWith('\n') ? m[0] + body : `${m[0]}\n${body}`;
};

const fieldRe = (key: string) => new RegExp(`^${key}\\s*:`);

export const parseFrontField = (raw: string | undefined, key: string): string => {
  if (!raw) return '';
  const m = FM.exec(raw);
  if (!m) return '';
  const line = m[1].split(/\r?\n/).find((l) => fieldRe(key).test(l.trim()));
  if (!line) return '';
  let v = line.trim().replace(new RegExp(`^${key}\\s*:\\s*`), '').trim();
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

export const setFrontField = (raw: string, key: string, value: string): string => {
  const m = FM.exec(raw);
  const body = m ? raw.slice(m[0].length) : raw;
  const lines = m ? m[1].split(/\r?\n/).filter((l) => l.trim()) : [];
  const v = value.trim();
  const at = lines.findIndex((l) => fieldRe(key).test(l.trim()));
  const next = v ? `${key}: ${/[:#"'\n]|^\s|\s$/.test(v) ? JSON.stringify(v) : v}` : null;
  if (at === -1 && next) lines.push(next);
  else if (at !== -1 && next) lines[at] = next;
  else if (at !== -1) lines.splice(at, 1);
  return lines.length ? `---\n${lines.join('\n')}\n---\n${body}` : body;
};

export const parseFrontTitle = (raw?: string): string => parseFrontField(raw, 'title');

export const applyFrontTitle = (raw: string, title: string): string => setFrontField(raw, 'title', title);
