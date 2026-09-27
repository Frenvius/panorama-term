import type { ThemedToken } from 'shiki';

import React from 'react';

import { highlight } from '~/usecase/service/highlight';

interface CodeBlockProps {
  lang: string;
  code: string;
}

const CodeBlock = ({ lang, code }: CodeBlockProps) => {
  const [tokens, setTokens] = React.useState<ThemedToken[][] | null>(null);

  React.useEffect(() => {
    if (!lang) return;
    let alive = true;
    void highlight(code, lang)
      .then((t) => alive && setTokens(t))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [lang, code]);

  if (!tokens) {
    return (
      <pre>
        <code>{code}</code>
      </pre>
    );
  }

  return (
    <pre>
      <code>
        {tokens.map((line, at) => (
          <React.Fragment key={at}>
            {at > 0 && '\n'}
            {line.map((t, col) => (
              <span key={col} style={{ color: t.color, fontStyle: t.fontStyle === 1 ? 'italic' : undefined }}>
                {t.content}
              </span>
            ))}
          </React.Fragment>
        ))}
      </code>
    </pre>
  );
};

export default CodeBlock;
