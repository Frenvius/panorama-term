import type { SyntaxNode } from '@lezer/common';

import React from 'react';
import { GFM, parser } from '@lezer/markdown';

import { htmlToReact } from '~/usecase/util/markdownHtml';

export interface MarkdownSlots {
  code: (lang: string, text: string, key: string) => React.ReactNode;
  image: (src: string, alt: string, key: string) => React.ReactNode;
  link: (href: string, children: React.ReactNode, key: string) => React.ReactNode;
}

interface Ctx {
  src: string;
  slots: MarkdownSlots;
  refs: Map<string, string>;
}

const gfm = parser.configure(GFM);

const MARKS = new Set([
  'HeaderMark',
  'QuoteMark',
  'ListMark',
  'EmphasisMark',
  'CodeMark',
  'LinkMark',
  'StrikethroughMark',
  'TableDelimiter',
  'TaskMarker'
]);

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ', copy: '©' };

const decodeEntity = (raw: string): string => {
  const body = raw.slice(1, -1);
  if (body.startsWith('#x') || body.startsWith('#X')) return String.fromCodePoint(parseInt(body.slice(2), 16) || 0xfffd);
  if (body.startsWith('#')) return String.fromCodePoint(parseInt(body.slice(1), 10) || 0xfffd);
  return ENTITIES[body] ?? raw;
};

const normLabel = (label: string): string => label.slice(1, -1).trim().replace(/\s+/g, ' ').toLowerCase();

const unwrapUrl = (url: string): string => (url.startsWith('<') && url.endsWith('>') ? url.slice(1, -1) : url);

const text = (ctx: Ctx, node: SyntaxNode): string => ctx.src.slice(node.from, node.to);

const kids = (node: SyntaxNode): SyntaxNode[] => {
  const out: SyntaxNode[] = [];
  for (let cur = node.firstChild; cur; cur = cur.nextSibling) out.push(cur);
  return out;
};

const collectRefs = (ctx: Ctx, top: SyntaxNode): void => {
  for (const node of kids(top)) {
    if (node.name !== 'LinkReference') continue;
    const label = node.getChild('LinkLabel');
    const url = node.getChild('URL');
    if (!label || !url) continue;
    const key = normLabel(text(ctx, label));
    if (!ctx.refs.has(key)) ctx.refs.set(key, unwrapUrl(text(ctx, url)));
  }
};

const inlineRange = (ctx: Ctx, parent: SyntaxNode, range: { from: number; to: number }): React.ReactNode[] => {
  const out: React.ReactNode[] = [];
  let pos = range.from;
  for (const child of kids(parent)) {
    if (child.to <= range.from || child.from >= range.to) continue;
    if (child.from > pos) out.push(ctx.src.slice(pos, child.from));
    out.push(renderNode(ctx, child));
    pos = child.to;
  }
  if (pos < range.to) out.push(ctx.src.slice(pos, range.to));
  return out;
};

const inline = (ctx: Ctx, node: SyntaxNode): React.ReactNode[] => inlineRange(ctx, node, node);

const trimEdges = (nodes: React.ReactNode[]): React.ReactNode[] => {
  const out = nodes.filter((n) => n !== null);
  if (typeof out[0] === 'string') out[0] = out[0].trimStart();
  const end = out.length - 1;
  if (typeof out[end] === 'string') out[end] = out[end].trimEnd();
  return out;
};

const trimmedInline = (ctx: Ctx, node: SyntaxNode): React.ReactNode[] => {
  const marks = kids(node).filter((c) => c.name === 'HeaderMark');
  const from = marks[0] && marks[0].from === node.from ? marks[0].to : node.from;
  const last = marks[marks.length - 1];
  const to = last && last.to === node.to && last.from > from ? last.from : node.to;
  return trimEdges(inlineRange(ctx, node, { from, to }));
};

const isTight = (ctx: Ctx, list: SyntaxNode): boolean => {
  const items = kids(list);
  for (let at = 1; at < items.length; at++) {
    if (/\n[ \t>]*\n/.test(ctx.src.slice(items[at - 1].to, items[at].from))) return false;
  }
  return items.every((item) => {
    const blocksIn = kids(item).filter((c) => c.name !== 'ListMark');
    return blocksIn.every((b, at) => at === 0 || !/\n[ \t>]*\n/.test(ctx.src.slice(blocksIn[at - 1].to, b.from)));
  });
};

const linkParts = (ctx: Ctx, node: SyntaxNode) => {
  const children = kids(node);
  const open = children[0];
  const close = children.find((c) => c.name === 'LinkMark' && text(ctx, c) === ']');
  const labelEnd = close ? close.from : node.to;
  const url = children.find((c) => c.name === 'URL');
  const label = node.getChild('LinkLabel');
  let href = url ? unwrapUrl(text(ctx, url)) : '';
  if (!url) {
    const key = normLabel(label ? text(ctx, label) : ctx.src.slice(open.from + (node.name === 'Image' ? 1 : 0), labelEnd + 1));
    href = ctx.refs.get(key) ?? '';
  }
  return { href, label: { from: open.to, to: labelEnd } };
};

const codeLines = (ctx: Ctx, node: SyntaxNode, fenced: boolean): string => {
  const lineStart = ctx.src.lastIndexOf('\n', node.from - 1) + 1;
  const indent = node.from - lineStart;
  const lines = ctx.src.slice(lineStart, node.to).split('\n');
  const body = fenced ? lines.slice(1) : lines;
  if (fenced && body.length && /^\s*(```|~~~)/.test(body[body.length - 1])) body.pop();
  const strip = fenced ? indent : indent + 4;
  return body.map((l) => l.replace(new RegExp(`^ {0,${strip}}`), '')).join('\n');
};

const cells = (ctx: Ctx, row: SyntaxNode, head: boolean, key: string): React.ReactNode[] =>
  kids(row)
    .filter((c) => c.name === 'TableCell')
    .map((cell, at) => {
      const body = inline(ctx, cell);
      return head ? <th key={`${key}-${at}`}>{body}</th> : <td key={`${key}-${at}`}>{body}</td>;
    });

const table = (ctx: Ctx, node: SyntaxNode, key: string): React.ReactNode => {
  const head = node.getChild('TableHeader');
  const rows = kids(node).filter((c) => c.name === 'TableRow');
  return (
    <table key={key}>
      {head && (
        <thead>
          <tr>{cells(ctx, head, true, `${key}-h`)}</tr>
        </thead>
      )}
      <tbody>
        {rows.map((row, at) => (
          <tr key={`${key}-r${at}`}>{cells(ctx, row, false, `${key}-r${at}`)}</tr>
        ))}
      </tbody>
    </table>
  );
};

const listItem = (ctx: Ctx, node: SyntaxNode, tight: boolean): React.ReactNode => {
  const task = node.getChild('Task');
  const marker = task?.getChild('TaskMarker');
  const checked = marker ? /x/i.test(text(ctx, marker)) : false;
  return (
    <li key={`li-${node.from}`}>
      {marker && <input type="checkbox" checked={checked} readOnly />}
      {blocks(ctx, node, tight)}
    </li>
  );
};

const list = (ctx: Ctx, node: SyntaxNode): React.ReactNode[] => {
  const tight = isTight(ctx, node);
  return kids(node).map((item) => listItem(ctx, item, tight));
};

const blocks = (ctx: Ctx, node: SyntaxNode, tight = false): React.ReactNode[] =>
  kids(node)
    .filter((c) => !MARKS.has(c.name))
    .map((c) =>
      tight && c.name === 'Paragraph' ? <React.Fragment key={`t-${c.from}`}>{inline(ctx, c)}</React.Fragment> : renderNode(ctx, c)
    );

const renderNode = (ctx: Ctx, node: SyntaxNode): React.ReactNode => {
  const key = `${node.name}-${node.from}`;
  const name = node.name;

  const heading = /^(?:ATX|Setext)Heading(\d)$/.exec(name);
  if (heading) return React.createElement(`h${heading[1]}`, { key }, trimmedInline(ctx, node));
  if (MARKS.has(name)) return null;

  switch (name) {
    case 'Paragraph':
      return <p key={key}>{inline(ctx, node)}</p>;
    case 'Task':
      return <React.Fragment key={key}>{trimEdges(inline(ctx, node))}</React.Fragment>;
    case 'Blockquote':
      return <blockquote key={key}>{blocks(ctx, node)}</blockquote>;
    case 'BulletList':
      return <ul key={key}>{list(ctx, node)}</ul>;
    case 'OrderedList': {
      const mark = node.firstChild?.getChild('ListMark');
      const start = mark ? parseInt(text(ctx, mark), 10) : 1;
      return (
        <ol key={key} start={start === 1 ? undefined : start}>
          {list(ctx, node)}
        </ol>
      );
    }
    case 'FencedCode': {
      const info = node.getChild('CodeInfo');
      const lang = info ? text(ctx, info).trim().split(/\s+/)[0].toLowerCase() : '';
      return ctx.slots.code(lang, codeLines(ctx, node, true), key);
    }
    case 'CodeBlock':
      return ctx.slots.code('', codeLines(ctx, node, false), key);
    case 'HorizontalRule':
      return <hr key={key} />;
    case 'Table':
      return table(ctx, node, key);
    case 'HTMLBlock':
      return <React.Fragment key={key}>{htmlToReact(text(ctx, node), key, ctx.slots)}</React.Fragment>;
    case 'LinkReference':
    case 'CommentBlock':
    case 'ProcessingInstructionBlock':
    case 'Comment':
      return null;
    case 'Emphasis':
      return <em key={key}>{inline(ctx, node)}</em>;
    case 'StrongEmphasis':
      return <strong key={key}>{inline(ctx, node)}</strong>;
    case 'Strikethrough':
      return <del key={key}>{inline(ctx, node)}</del>;
    case 'InlineCode': {
      const raw = text(ctx, node);
      const fence = /^`+/.exec(raw)?.[0].length ?? 1;
      const body = raw.slice(fence, raw.length - fence);
      return <code key={key}>{/^ .* $/.test(body) ? body.slice(1, -1) : body}</code>;
    }
    case 'Link': {
      const { href, label } = linkParts(ctx, node);
      return ctx.slots.link(href, inlineRange(ctx, node, label), key);
    }
    case 'Image': {
      const { href, label } = linkParts(ctx, node);
      return ctx.slots.image(href, ctx.src.slice(label.from, label.to), key);
    }
    case 'Autolink': {
      const url = node.getChild('URL');
      const href = url ? text(ctx, url) : '';
      return ctx.slots.link(href.includes('@') && !href.includes(':') ? `mailto:${href}` : href, href, key);
    }
    case 'URL':
      return ctx.slots.link(text(ctx, node), text(ctx, node), key);
    case 'Escape':
      return text(ctx, node).slice(1);
    case 'Entity':
      return decodeEntity(text(ctx, node));
    case 'HardBreak':
      return <br key={key} />;
    case 'HTMLTag':
      return /^<br\s*\/?>$/i.test(text(ctx, node)) ? <br key={key} /> : null;
    default:
      return node.firstChild ? <React.Fragment key={key}>{inline(ctx, node)}</React.Fragment> : text(ctx, node);
  }
};

export const isRemoteUrl = (url: string): boolean => /^(https?:|data:|blob:)/i.test(url);

export const resolveAsset = (file: string, rel: string): string => {
  const clean = decodeURI(rel.split(/[?#]/)[0]);
  if (/^([A-Za-z]:[\\/]|[\\/])/.test(clean)) return clean;
  const sep = file.includes('\\') ? '\\' : '/';
  const parts = file.split(/[\\/]/).slice(0, -1);
  for (const seg of clean.split(/[\\/]/)) {
    if (seg === '..') parts.pop();
    else if (seg && seg !== '.') parts.push(seg);
  }
  return parts.join(sep);
};

export const renderMarkdown =(src: string, slots: MarkdownSlots): React.ReactNode[] => {
  const top = gfm.parse(src).topNode;
  const ctx: Ctx = { src, slots, refs: new Map() };
  collectRefs(ctx, top);
  return blocks(ctx, top);
};
