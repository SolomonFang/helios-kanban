import { memo, useMemo, type ReactNode } from 'react';
import { compiler } from 'markdown-to-jsx';
import { sanitizeHref } from '@/components/ui/wysiwyg/plugins/read-only-link-plugin';
import { CodeBlock } from './MarkdownCode';
import { MarkdownImage } from './MarkdownImage';
import { markdownThemes, type MarkdownVariant } from './markdownTheme';

interface MarkdownBlockProps {
  content: string;
  workspaceId?: string;
  variant?: MarkdownVariant;
  /** Returns the matching diff path when an inline code span names a changed file */
  findMatchingDiffPath?: (text: string) => string | null;
  onCodeClick?: (filePath: string) => void;
}

/**
 * Renders one markdown block via markdown-to-jsx with overrides that mirror
 * the Lexical read-only theme (wysiwyg.tsx) and preserve its behaviors:
 * sanitized links, Prism-highlighted code blocks, clickable file-path inline
 * code, and .vibe-images thumbnails.
 */
export const MarkdownBlock = memo(
  function MarkdownBlock({
    content,
    workspaceId,
    variant = 'new',
    findMatchingDiffPath,
    onCodeClick,
  }: MarkdownBlockProps) {
    const theme = markdownThemes[variant];
    return useMemo(() => {
      const inlineCode = ({ children }: { children?: ReactNode }) => {
        const text = typeof children === 'string' ? children.trim() : '';
        const matchedPath = text && findMatchingDiffPath?.(text);
        const clickable = Boolean(matchedPath && onCodeClick);
        return (
          <code
            className={
              clickable
                ? `${theme.inlineCode} ${theme.inlineCodeClickable}`
                : theme.inlineCode
            }
            onClick={
              clickable
                ? (e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    onCodeClick!(matchedPath!);
                  }
                : undefined
            }
          >
            {children}
          </code>
        );
      };

      const preBlock = ({ children }: { children?: ReactNode }) => {
        // markdown-to-jsx renders fences as <pre><code className="lang-x">…</code></pre>
        const codeEl =
          children && typeof children === 'object' && 'props' in children
            ? (children as {
                props?: { className?: string; children?: ReactNode };
              })
            : null;
        const lang =
          codeEl?.props?.className?.replace(/^lang-/, '') || undefined;
        const code =
          typeof codeEl?.props?.children === 'string'
            ? codeEl.props.children
            : '';
        return <CodeBlock code={code} lang={lang} variant={variant} />;
      };

      const link = ({
        href,
        children,
      }: {
        href?: string;
        children?: ReactNode;
      }) => {
        const safe = sanitizeHref(href);
        if (!safe || !/^https:\/\//i.test(safe)) {
          // Dangerous protocol or relative/anchor link: render but not clickable
          return (
            <a
              className={theme.link}
              style={{ cursor: 'not-allowed', pointerEvents: 'none' }}
              role="link"
              aria-disabled="true"
              title={href}
            >
              {children}
            </a>
          );
        }
        return (
          <a
            href={safe}
            className={theme.link}
            target="_blank"
            rel="noopener noreferrer"
            onClick={(e) => e.stopPropagation()}
          >
            {children}
          </a>
        );
      };

      const image = ({ src, alt }: { src?: string; alt?: string }) => (
        <MarkdownImage src={src} alt={alt} workspaceId={workspaceId} />
      );

      return compiler(content, {
        overrides: {
          p: (props: object) => <p className={theme.paragraph} {...props} />,
          h1: (props: object) => (
            <h1 className={theme.headings.h1} {...props} />
          ),
          h2: (props: object) => (
            <h2 className={theme.headings.h2} {...props} />
          ),
          h3: (props: object) => (
            <h3 className={theme.headings.h3} {...props} />
          ),
          h4: (props: object) => (
            <h4 className={theme.headings.h4} {...props} />
          ),
          h5: (props: object) => (
            <h5 className={theme.headings.h5} {...props} />
          ),
          h6: (props: object) => (
            <h6 className={theme.headings.h6} {...props} />
          ),
          blockquote: (props: object) => (
            <blockquote className={theme.quote} {...props} />
          ),
          ul: (props: object) => <ul className={theme.ul} {...props} />,
          ol: (props: object) => <ol className={theme.ol} {...props} />,
          li: (props: object) => <li className={theme.li} {...props} />,
          hr: (props: object) => <hr className={theme.hr} {...props} />,
          strong: (props: object) => (
            <strong className={theme.strong} {...props} />
          ),
          del: (props: object) => <del className="line-through" {...props} />,
          a: link,
          code: inlineCode,
          pre: preBlock,
          img: image,
          table: (props: object) => (
            <div className={theme.tableWrapper}>
              <table className={theme.table} {...props} />
            </div>
          ),
          th: (props: object) => <th className={theme.th} {...props} />,
          td: (props: object) => <td className={theme.td} {...props} />,
        },
      });
    }, [
      content,
      workspaceId,
      variant,
      theme,
      findMatchingDiffPath,
      onCodeClick,
    ]);
  },
  (prev, next) =>
    prev.content === next.content &&
    prev.workspaceId === next.workspaceId &&
    prev.variant === next.variant &&
    prev.findMatchingDiffPath === next.findMatchingDiffPath &&
    prev.onCodeClick === next.onCodeClick
);
