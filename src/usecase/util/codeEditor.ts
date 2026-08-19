import type { Extension } from '@codemirror/state';

import { tags } from '@lezer/highlight';
import { languages } from '@codemirror/language-data';
import { HighlightStyle, LanguageDescription, syntaxHighlighting } from '@codemirror/language';

const style = HighlightStyle.define([
  { tag: tags.comment, color: 'var(--shiki-token-comment)', fontStyle: 'italic' },
  { tag: tags.lineComment, color: 'var(--shiki-token-comment)', fontStyle: 'italic' },
  { tag: tags.blockComment, color: 'var(--shiki-token-comment)', fontStyle: 'italic' },
  { tag: tags.docComment, color: 'var(--shiki-token-comment)', fontStyle: 'italic' },
  { tag: tags.keyword, color: 'var(--shiki-token-keyword)' },
  { tag: tags.controlKeyword, color: 'var(--shiki-token-keyword)' },
  { tag: tags.moduleKeyword, color: 'var(--shiki-token-keyword)' },
  { tag: tags.operatorKeyword, color: 'var(--shiki-token-keyword)' },
  { tag: tags.definitionKeyword, color: 'var(--shiki-token-keyword)' },
  { tag: tags.modifier, color: 'var(--shiki-token-keyword)' },
  { tag: tags.self, color: 'var(--shiki-token-keyword)' },
  { tag: tags.null, color: 'var(--shiki-token-constant)' },
  { tag: tags.bool, color: 'var(--shiki-token-constant)' },
  { tag: tags.number, color: 'var(--shiki-token-constant)' },
  { tag: tags.atom, color: 'var(--shiki-token-constant)' },
  { tag: tags.string, color: 'var(--shiki-token-string)' },
  { tag: tags.special(tags.string), color: 'var(--shiki-token-string-expression)' },
  { tag: tags.regexp, color: 'var(--shiki-token-string-expression)' },
  { tag: tags.escape, color: 'var(--shiki-token-string-expression)' },
  { tag: tags.function(tags.variableName), color: 'var(--shiki-token-function)' },
  { tag: tags.function(tags.propertyName), color: 'var(--shiki-token-function)' },
  { tag: tags.definition(tags.function(tags.variableName)), color: 'var(--shiki-token-function)' },
  { tag: tags.className, color: 'var(--shiki-token-function)' },
  { tag: tags.typeName, color: 'var(--shiki-token-function)' },
  { tag: tags.namespace, color: 'var(--shiki-token-function)' },
  { tag: tags.propertyName, color: 'var(--shiki-token-parameter)' },
  { tag: tags.attributeName, color: 'var(--shiki-token-parameter)' },
  { tag: tags.tagName, color: 'var(--shiki-token-keyword)' },
  { tag: tags.labelName, color: 'var(--shiki-token-parameter)' },
  { tag: tags.link, color: 'var(--shiki-token-link)', textDecoration: 'underline' },
  { tag: tags.url, color: 'var(--shiki-token-link)' },
  { tag: tags.heading, color: 'var(--shiki-token-keyword)', fontWeight: 'bold' },
  { tag: tags.emphasis, fontStyle: 'italic' },
  { tag: tags.strong, fontWeight: 'bold' },
  { tag: tags.punctuation, color: 'var(--shiki-token-punctuation)' },
  { tag: tags.bracket, color: 'var(--shiki-token-punctuation)' },
  { tag: tags.separator, color: 'var(--shiki-token-punctuation)' },
  { tag: tags.meta, color: 'var(--shiki-token-comment)' },
  { tag: tags.invalid, color: 'var(--danger)' }
]);

export const codeHighlight = (): Extension => syntaxHighlighting(style);

export const fileName = (path: string): string => path.split(/[\\/]/).pop() ?? path;

export const languageFor = async (path: string): Promise<Extension | null> => {
  const desc = LanguageDescription.matchFilename(languages, fileName(path));
  if (!desc) return null;
  const support = await desc.load().catch(() => null);
  return support ?? null;
};
