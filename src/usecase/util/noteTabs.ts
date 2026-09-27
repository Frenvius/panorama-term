import { stripFrontmatter } from '~/usecase/util/noteMeta';

export interface NoteTab {
  name: string;
  text: string;
}

const MARKER = /^<!-- tab: (.*?) -->[ \t]*$/gm;

export const splitTabs = (body: string): NoteTab[] => {
  const marks = [...body.matchAll(MARKER)];
  if (!marks.length) return [{ name: '', text: body }];
  const tabs: NoteTab[] = [];
  const lead = body.slice(0, marks[0].index);
  if (lead.trim()) tabs.push({ name: '', text: lead.replace(/\r?\n$/, '') });
  marks.forEach((m, i) => {
    const start = m.index + m[0].length;
    const end = i + 1 < marks.length ? marks[i + 1].index : body.length;
    let text = body.slice(start, end).replace(/^\r?\n/, '');
    if (i + 1 < marks.length) text = text.replace(/\r?\n$/, '');
    tabs.push({ name: m[1], text });
  });
  return tabs;
};

export const nameTabs = (tabs: NoteTab[]): NoteTab[] => {
  const clean = tabs.map((t) => ({ ...t, name: t.name.replace(/-->|\r?\n/g, ' ').trim() }));
  if (clean.length === 1) return clean;
  const used = new Set(clean.map((t) => t.name).filter(Boolean));
  let n = 1;
  return clean.map((t) => {
    if (t.name) return t;
    while (used.has(`Tab ${n}`)) n++;
    used.add(`Tab ${n}`);
    return { ...t, name: `Tab ${n}` };
  });
};

export const joinTabs = (tabs: NoteTab[]): string => {
  if (tabs.length === 1 && !tabs[0].name) return tabs[0].text;
  return nameTabs(tabs)
    .map((t) => `<!-- tab: ${t.name} -->\n${t.text}`)
    .join('\n');
};

export const noteTabs = (raw?: string): NoteTab[] => splitTabs(stripFrontmatter(raw || ''));

export const tabText = (raw: string | undefined, index: number): string => {
  const tabs = noteTabs(raw);
  return tabs[Math.min(index, tabs.length - 1)].text;
};

export const withTabText = (body: string, index: number, text: string): string => {
  const tabs = splitTabs(body);
  if (index >= tabs.length) return body;
  return joinTabs(tabs.map((t, i) => (i === index ? { ...t, text } : t)));
};
