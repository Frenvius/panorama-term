import type { MarkdownSlots } from '~/usecase/util/markdownRender';

import React from 'react';

import { openUrl } from '~/adapter/shell/shell.client';
import { renderMarkdown } from '~/usecase/util/markdownRender';

import MdImage from './MdImage';
import CodeBlock from './CodeBlock';
import styles from './styles.module.scss';

interface MarkdownViewProps {
  path: string;
  source: string;
}

const EXTERNAL = /^(https?:|mailto:)/i;

const followLink = (e: React.MouseEvent<HTMLAnchorElement>) => {
  e.preventDefault();
  const href = e.currentTarget.getAttribute('href') ?? '';
  if (EXTERNAL.test(href)) openUrl(href);
};

const MarkdownView = ({ path, source }: MarkdownViewProps) => {
  const content = React.useMemo(() => {
    const slots: MarkdownSlots = {
      code: (lang, text, key) => <CodeBlock key={key} lang={lang} code={text} />,
      image: (src, alt, key) => <MdImage key={key} src={src} alt={alt} file={path} />,
      link: (href, children, key) => (
        <a key={key} href={href} title={href} onClick={followLink}>
          {children}
        </a>
      )
    };
    return renderMarkdown(source, slots);
  }, [source, path]);

  return (
    <div className={styles.view}>
      <article className={styles.doc}>{content}</article>
    </div>
  );
};

export default MarkdownView;
