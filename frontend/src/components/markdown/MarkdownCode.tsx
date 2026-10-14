import { useMemo, type ReactNode } from 'react';
import Prism from 'prismjs';
import { CODE_HIGHLIGHT_CLASSES } from '@/components/ui/wysiwyg/lib/code-highlight-theme';

function renderToken(token: string | Prism.Token, key: number): ReactNode {
  if (typeof token === 'string') return token;

  const aliases = Array.isArray(token.alias)
    ? token.alias
    : token.alias
      ? [token.alias]
      : [];
  const className =
    CODE_HIGHLIGHT_CLASSES[token.type] ??
    aliases.map((a) => CODE_HIGHLIGHT_CLASSES[a]).find(Boolean);

  const content = Array.isArray(token.content)
    ? token.content.map((t, i) => renderToken(t, i))
    : typeof token.content === 'string'
      ? token.content
      : renderToken(token.content, 0);

  return (
    <span key={key} className={className}>
      {content}
    </span>
  );
}

/**
 * Highlight code with Prism using the same token → class mapping as the
 * Lexical editor (CODE_HIGHLIGHT_CLASSES → --syntax-* CSS variables), so
 * colors are identical in light and dark themes.
 * Only Prism's built-in grammars (markup, css, clike, javascript) highlight —
 * same coverage as the previous Lexical-based rendering.
 */
export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const highlighted = useMemo((): ReactNode => {
    const grammar = lang ? Prism.languages[lang] : undefined;
    if (!grammar) return code;
    try {
      return Prism.tokenize(code, grammar).map((t, i) => renderToken(t, i));
    } catch {
      return code;
    }
  }, [code, lang]);

  return (
    <pre className="block font-mono bg-secondary rounded-md px-3 py-2 my-2 whitespace-pre overflow-x-auto">
      <code>{highlighted}</code>
    </pre>
  );
}
