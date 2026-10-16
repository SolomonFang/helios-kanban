import { useCallback, useMemo, useState, type ReactNode } from 'react';
import Prism from 'prismjs';
import { Check, Clipboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { CODE_HIGHLIGHT_CLASSES } from '@/components/ui/wysiwyg/lib/code-highlight-theme';
import { markdownThemes, type MarkdownVariant } from './markdownTheme';
import { writeClipboardViaBridge } from '@/vscode/bridge';

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
export function CodeBlock({
  code,
  lang,
  variant = 'new',
}: {
  code: string;
  lang?: string;
  variant?: MarkdownVariant;
}) {
  const { t } = useTranslation('common');
  const [copied, setCopied] = useState(false);
  const theme = markdownThemes[variant].codeBlock;

  const highlighted = useMemo((): ReactNode => {
    const grammar = lang ? Prism.languages[lang] : undefined;
    if (!grammar) return code;
    try {
      return Prism.tokenize(code, grammar).map((t, i) => renderToken(t, i));
    } catch {
      return code;
    }
  }, [code, lang]);

  const handleCopy = useCallback(async () => {
    if (!code) return;
    try {
      await writeClipboardViaBridge(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 400);
    } catch {
      // noop – bridge handles fallback
    }
  }, [code]);

  return (
    <div className={theme.container}>
      <div className={theme.header}>
        <span className={theme.label}>{lang || 'text'}</span>
        <button
          type="button"
          aria-label={copied ? t('actions.copied') : t('actions.copyCode')}
          title={copied ? t('actions.copied') : t('actions.copyCode')}
          onClick={handleCopy}
          className={theme.copyButton}
        >
          {copied ? (
            <Check className="w-3.5 h-3.5 text-success" />
          ) : (
            <Clipboard className="w-3.5 h-3.5" />
          )}
        </button>
      </div>
      <pre className={theme.pre}>
        <code>{highlighted}</code>
      </pre>
    </div>
  );
}
