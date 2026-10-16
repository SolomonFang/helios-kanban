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
          // Keep className AFTER {...props}: markdown-to-jsx may pass
          // className: undefined, which would otherwise erase our theme.
          p: (props: object) => <p {...props} className={theme.paragraph} />,
          h1: (props: object) => (
            <h1 {...props} className={theme.headings.h1} />
          ),
          h2: (props: object) => (
            <h2 {...props} className={theme.headings.h2} />
          ),
          h3: (props: object) => (
            <h3 {...props} className={theme.headings.h3} />
          ),
          h4: (props: object) => (
            <h4 {...props} className={theme.headings.h4} />
          ),
          h5: (props: object) => (
            <h5 {...props} className={theme.headings.h5} />
          ),
          h6: (props: object) => (
            <h6 {...props} className={theme.headings.h6} />
          ),
          blockquote: (props: object) => (
            <blockquote {...props} className={theme.quote} />
          ),
          ul: (props: object) => <ul {...props} className={theme.ul} />,
          ol: (props: object) => <ol {...props} className={theme.ol} />,
          li: (props: object) => <li {...props} className={theme.li} />,
          hr: (props: object) => <hr {...props} className={theme.hr} />,
          strong: (props: object) => (
            <strong {...props} className={theme.strong} />
          ),
          del: (props: object) => <del {...props} className="line-through" />,
          a: link,
          code: inlineCode,
          pre: preBlock,
          img: image,
          table: (props: object) => (
            <div className={theme.tableWrapper}>
              <table {...props} className={theme.table} />
            </div>
          ),
          th: (props: object) => <th {...props} className={theme.th} />,
          td: (props: object) => <td {...props} className={theme.td} />,
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
