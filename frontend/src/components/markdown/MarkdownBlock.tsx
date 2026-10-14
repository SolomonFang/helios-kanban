import { memo, useMemo, type ReactNode } from 'react';
import { compiler } from 'markdown-to-jsx';
import { sanitizeHref } from '@/components/ui/wysiwyg/plugins/read-only-link-plugin';
import { CodeBlock } from './MarkdownCode';
import { MarkdownImage } from './MarkdownImage';

const LINK_CLASSES =
  'text-blue-600 dark:text-blue-400 underline underline-offset-2 cursor-pointer hover:text-blue-800 dark:hover:text-blue-300';
const INLINE_CODE_CLASSES = 'font-mono bg-muted bg-panel px-1 py-0.5 rounded';

interface MarkdownBlockProps {
  content: string;
  workspaceId?: string;
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
    findMatchingDiffPath,
    onCodeClick,
  }: MarkdownBlockProps) {
    return useMemo(() => {
      const inlineCode = ({ children }: { children?: ReactNode }) => {
        const text = typeof children === 'string' ? children.trim() : '';
        const matchedPath = text && findMatchingDiffPath?.(text);
        const clickable = Boolean(matchedPath && onCodeClick);
        return (
          <code
            className={
              clickable
                ? `${INLINE_CODE_CLASSES} cursor-pointer`
                : INLINE_CODE_CLASSES
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
        return <CodeBlock code={code} lang={lang} />;
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
              className={LINK_CLASSES}
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
            className={LINK_CLASSES}
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
          p: (props: object) => <p className="mb-2 last:mb-0" {...props} />,
          h1: (props: object) => (
            <h1 className="mt-4 mb-2 text-2xl font-semibold" {...props} />
          ),
          h2: (props: object) => (
            <h2 className="mt-3 mb-2 text-xl font-semibold" {...props} />
          ),
          h3: (props: object) => (
            <h3 className="mt-3 mb-2 text-lg font-semibold" {...props} />
          ),
          h4: (props: object) => (
            <h4 className="mt-2 mb-1 text-base font-medium" {...props} />
          ),
          h5: (props: object) => (
            <h5 className="mt-2 mb-1 text-sm font-medium" {...props} />
          ),
          h6: (props: object) => (
            <h6
              className="mt-2 mb-1 text-xs font-medium uppercase tracking-wide"
              {...props}
            />
          ),
          blockquote: (props: object) => (
            <blockquote
              className="my-3 border-l-4 border-primary-foreground pl-4 text-muted-foreground"
              {...props}
            />
          ),
          ul: (props: object) => (
            <ul className="my-1 list-disc list-inside" {...props} />
          ),
          ol: (props: object) => (
            <ol className="my-1 list-decimal list-inside" {...props} />
          ),
          strong: (props: object) => (
            <strong className="font-semibold" {...props} />
          ),
          del: (props: object) => <del className="line-through" {...props} />,
          a: link,
          code: inlineCode,
          pre: preBlock,
          img: image,
          table: (props: object) => (
            <table className="border-collapse my-2 w-full text-sm" {...props} />
          ),
          th: (props: object) => (
            <th
              className="bg-muted font-semibold border border-low px-3 py-2 text-left align-top"
              {...props}
            />
          ),
          td: (props: object) => (
            <td
              className="border border-low px-3 py-2 text-left align-top"
              {...props}
            />
          ),
        },
      });
    }, [content, workspaceId, findMatchingDiffPath, onCodeClick]);
  },
  (prev, next) =>
    prev.content === next.content &&
    prev.workspaceId === next.workspaceId &&
    prev.findMatchingDiffPath === next.findMatchingDiffPath &&
    prev.onCodeClick === next.onCodeClick
);
