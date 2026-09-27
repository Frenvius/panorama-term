import type { MarkdownSlots } from '~/usecase/util/markdownRender';

import React from 'react';

const TAGS = new Set([
  'p',
  'div',
  'span',
  'br',
  'hr',
  'b',
  'i',
  'u',
  's',
  'em',
  'strong',
  'del',
  'code',
  'pre',
  'kbd',
  'sub',
  'sup',
  'small',
  'mark',
  'blockquote',
  'details',
  'summary',
  'ul',
  'ol',
  'li',
  'table',
  'thead',
  'tbody',
  'tr',
  'th',
  'td',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'picture',
  'center'
]);

const DROP = new Set(['script', 'style', 'iframe', 'object', 'embed', 'form', 'input', 'button', 'textarea', 'select', 'source']);

const ALIGN = new Set(['left', 'right', 'center']);

const attrsOf = (el: Element, key: string): Record<string, unknown> => {
  const props: Record<string, unknown> = { key };
  const align = el.getAttribute('align');
  if (align && ALIGN.has(align)) props.style = { textAlign: align };
  const title = el.getAttribute('title');
  if (title) props.title = title;
  if (el.tagName === 'DETAILS' && el.hasAttribute('open')) props.open = true;
  return props;
};

const nodeToReact = (node: Node, key: string, slots: MarkdownSlots): React.ReactNode => {
  if (node.nodeType === Node.TEXT_NODE) return node.textContent;
  if (node.nodeType !== Node.ELEMENT_NODE) return null;

  const el = node as Element;
  const tag = el.tagName.toLowerCase();
  if (DROP.has(tag)) return null;

  const children = Array.from(el.childNodes).map((child, at) => nodeToReact(child, `${key}-${at}`, slots));
  if (tag === 'img') return slots.image(el.getAttribute('src') ?? '', el.getAttribute('alt') ?? '', key);
  if (tag === 'a') return slots.link(el.getAttribute('href') ?? '', children, key);
  if (tag === 'center') return React.createElement('div', { key, style: { textAlign: 'center' } }, children);
  if (!TAGS.has(tag)) return <React.Fragment key={key}>{children}</React.Fragment>;
  if (tag === 'br' || tag === 'hr') return React.createElement(tag, { key });
  return React.createElement(tag, attrsOf(el, key), children);
};

export const htmlToReact = (html: string, key: string, slots: MarkdownSlots): React.ReactNode => {
  if (typeof DOMParser === 'undefined') return null;
  const body = new DOMParser().parseFromString(html, 'text/html').body;
  return Array.from(body.childNodes).map((child, at) => nodeToReact(child, `${key}-${at}`, slots));
};
