import React from 'react';

const INLINE = /(\*\*[^*\n]+\*\*|`[^`\n]+`)/g;

const inline = (text: string, key: string): React.ReactNode[] =>
  text.split(INLINE).map((part, at) => {
    const id = `${key}-${at}`;
    if (part.startsWith('**') && part.endsWith('**')) return <strong key={id}>{part.slice(2, -2)}</strong>;
    if (part.startsWith('`') && part.endsWith('`')) return <code key={id}>{part.slice(1, -1)}</code>;
    return part;
  });

export interface MarkdownBlock {
  kind: 'text' | 'bullet' | 'code';
  key: string;
  body: React.ReactNode;
}

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
    const bullet = line.match(/^\s*[-*]\s+(.*)$/);
    if (bullet) {
      blocks.push({ kind: 'bullet', key, body: inline(bullet[1], key) });
      continue;
    }
    if (!line.trim()) continue;
    blocks.push({ kind: 'text', key, body: inline(line, key) });
  }

  if (fence) blocks.push({ kind: 'code', key: 'tail', body: fence.join('\n') });
  return blocks;
};
