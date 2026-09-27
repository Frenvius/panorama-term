import { test, expect } from 'bun:test';
import { renderToStaticMarkup } from 'react-dom/server';

import type { MarkdownSlots } from '~/usecase/util/markdownRender';
import { renderMarkdown, resolveAsset } from '~/usecase/util/markdownRender';

const slots: MarkdownSlots = {
  code: (lang, text, key) => (
    <pre key={key} data-lang={lang}>
      {text}
    </pre>
  ),
  image: (src, alt, key) => <img key={key} src={src} alt={alt} />,
  link: (href, children, key) => (
    <a key={key} href={href}>
      {children}
    </a>
  )
};

const html = (src: string): string =>
  renderToStaticMarkup(<>{renderMarkdown(src, slots)}</>).replace(/<link rel="preload"[^>]*>/g, '');

test('headings, emphasis and inline code', () => {
  expect(html('## Change *log* `x`')).toBe('<h2>Change <em>log</em> <code>x</code></h2>');
  expect(html('Title\n===')).toBe('<h1>Title</h1>');
});

test('nested lists keep their structure', () => {
  const out = html(' - 1.0.0 - Release\n - 1.1.2 - Fixed\n   - Removed spam\n   - Added option');
  expect(out).toBe('<ul><li>1.0.0 - Release</li><li>1.1.2 - Fixed<ul><li>Removed spam</li><li>Added option</li></ul></li></ul>');
});

test('fenced code strips fences and keeps language', () => {
  expect(html('```ts\nconst a = 1;\n```')).toBe('<pre data-lang="ts">const a = 1;</pre>');
});

test('links, reference links, autolinks and images', () => {
  expect(html('[site](https://a.dev)')).toBe('<p><a href="https://a.dev">site</a></p>');
  expect(html('[x][r]\n\n[r]: https://r.dev')).toBe('<p><a href="https://r.dev">x</a></p>');
  expect(html('Discord: https://discord.gg/abc')).toBe('<p>Discord: <a href="https://discord.gg/abc">https://discord.gg/abc</a></p>');
  expect(html('![logo](img/a.png)')).toBe('<p><img src="img/a.png" alt="logo"/></p>');
});

test('gfm tables, tasks and strikethrough', () => {
  expect(html('| a | b |\n|---|---|\n| 1 | 2 |')).toBe(
    '<table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>'
  );
  expect(html('- [x] done')).toBe('<ul><li><input type="checkbox" readOnly="" checked=""/>done</li></ul>');
  expect(html('~~old~~')).toBe('<p><del>old</del></p>');
});

test('loose lists keep paragraphs', () => {
  expect(html('- a\n\n- b')).toBe('<ul><li><p>a</p></li><li><p>b</p></li></ul>');
});

test('escapes and entities', () => {
  expect(html('\\*not em\\* &amp; &#65;')).toBe('<p>*not em* &amp; A</p>');
});

test('raw html is never passed through as markup', () => {
  expect(html('<script>alert(1)</script>')).not.toContain('<script');
});

test('resolves relative assets against the file', () => {
  expect(resolveAsset('D:\\repo\\docs\\README.md', '../img/a.png')).toBe('D:\\repo\\img\\a.png');
  expect(resolveAsset('/repo/README.md', './a%20b.png?raw=1')).toBe('/repo/a b.png');
});
