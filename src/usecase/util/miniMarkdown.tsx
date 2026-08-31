import React from 'react';

const INLINE = /(\*\*[^*\n]+\*\*|`[^`\n]+`)/g;

const TOKEN =
  /(https?:\/\/\S+|[\w.-]+@[\w.-]+\.\w+|(?:[A-Za-z]:)?[\\/][\w.\-\\/]*\w|(?:[\w.-]+[\\/])+[\w.-]+\.\w+(?::\d+)?|[\w.-]+\.(?:tsx?|jsx?|rs|scss|css|json|md|toml|ya?ml)(?::\d+)?|\b[0-9a-f]{7,40}\b)/g;

const IS_TOKEN = new RegExp(`^(?:${TOKEN.source})$`);

const marked = (text: string, key: string): React.ReactNode[] =>
  text.split(TOKEN).map((part, at) => (part && IS_TOKEN.test(part) ? <em key={`${key}-t${at}`}>{part}</em> : part));

const inline = (text: string, key: string): React.ReactNode[] =>
  text.split(INLINE).map((part, at) => {
    const id = `${key}-${at}`;
    if (part.startsWith('**') && part.endsWith('**')) return <strong key={id}>{part.slice(2, -2)}</strong>;
    if (part.startsWith('`') && part.endsWith('`')) return <code key={id}>{part.slice(1, -1)}</code>;
    return <React.Fragment key={id}>{marked(part, id)}</React.Fragment>;
  });

export interface MarkdownBlock {
  kind: 'text' | 'bullet' | 'code' | 'table';
  key: string;
  body: React.ReactNode;
}

const cells = (line: string): string[] =>
  line
    .trim()
    .replace(/^\||\|$/g, '')
    .split('|')
    .map((c) => c.trim());

const isRow = (line?: string): boolean => Boolean(line?.trim().startsWith('|'));

const isDivider = (line?: string): boolean => Boolean(line && /^\s*\|[\s:|-]+\|\s*$/.test(line));

const table = (head: string[], rows: string[][], key: string): React.ReactNode => (
  <table>
    <thead>
      <tr>
        {head.map((cell, at) => (
          <th key={`${key}-h${at}`}>{inline(cell, `${key}-h${at}`)}</th>
        ))}
      </tr>
    </thead>
    <tbody>
      {rows.map((row, at) => (
        <tr key={`${key}-r${at}`}>
          {row.map((cell, col) => (
            <td key={`${key}-r${at}-${col}`}>{inline(cell, `${key}-r${at}-${col}`)}</td>
          ))}
        </tr>
      ))}
    </tbody>
  </table>
);

export const miniMarkdown = (raw: string): MarkdownBlock[] => {
  const blocks: MarkdownBlock[] = [];
  const lines = raw.split('\n');
  let fence: string[] | null = null;

  for (let at = 0; at < lines.length; at++) {
    const line = lines[at];
    const key = `b${at}`;
    if (line.trimStart().startsWith('```')) {
      if (fence) {
        blocks.push({ kind: 'code', key, body: fence.join('\n') });
        fence = null;
      } else {
        fence = [];
      }
      continue;
    }
    if (fence) {
      fence.push(line);
      continue;
    }
    if (isRow(line) && isDivider(lines[at + 1])) {
      const head = cells(line);
      const rows: string[][] = [];
      at += 2;
      while (at < lines.length && isRow(lines[at])) {
        rows.push(cells(lines[at]));
        at++;
      }
      at--;
      blocks.push({ kind: 'table', key, body: table(head, rows, key) });
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) {
      blocks.push({ kind: 'bullet', key, body: inline(bullet[1], key) });
      continue;
    }
    if (!line.trim()) continue;
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    if (heading) {
      blocks.push({ kind: 'text', key, body: <strong>{inline(heading[1], key)}</strong> });
      continue;
    }
    blocks.push({ kind: 'text', key, body: inline(line, key) });
  }

  if (fence) blocks.push({ kind: 'code', key: 'tail', body: fence.join('\n') });
  return blocks;
};
