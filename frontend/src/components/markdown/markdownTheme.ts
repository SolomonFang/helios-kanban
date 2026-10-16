export type MarkdownVariant = 'new' | 'legacy';

export interface MarkdownTheme {
  link: string;
  inlineCode: string;
  inlineCodeClickable: string;
  paragraph: string;
  headings: {
    h1: string;
    h2: string;
    h3: string;
    h4: string;
    h5: string;
    h6: string;
  };
  quote: string;
  ul: string;
  ol: string;
  li: string;
  hr: string;
  strong: string;
  tableWrapper: string;
  table: string;
  th: string;
  td: string;
  codeBlock: {
    container: string;
    header: string;
    label: string;
    copyButton: string;
    pre: string;
  };
}

/**
 * The two Tailwind configs expose different design tokens. Keep the classes
 * for each design system explicit so the shared renderer works in both the
 * new UI and the legacy UI without emitting classes that do not exist there.
 */
export const markdownThemes: Record<MarkdownVariant, MarkdownTheme> = {
  new: {
    link: 'text-brand underline underline-offset-2 decoration-brand/40 hover:decoration-brand hover:text-brand-hover cursor-pointer',
    inlineCode:
      'font-ibm-plex-mono text-[0.875em] bg-panel border border-border rounded px-1 py-px',
    inlineCodeClickable: 'cursor-pointer hover:border-brand transition-colors',
    paragraph: 'mb-2 last:mb-0 leading-relaxed',
    headings: {
      h1: 'mt-3 mb-1.5 first:mt-0 text-xl font-semibold text-high',
      h2: 'mt-3 mb-1.5 first:mt-0 text-lg font-semibold text-high',
      h3: 'mt-3 mb-1.5 first:mt-0 text-base font-semibold text-high',
      h4: 'mt-2 mb-1 first:mt-0 text-base font-medium text-high',
      h5: 'mt-2 mb-1 first:mt-0 text-sm font-medium text-high',
      h6: 'mt-2 mb-1 first:mt-0 text-sm font-medium text-low uppercase tracking-wide',
    },
    quote: 'my-3 border-l-2 border-border pl-3 text-low',
    ul: 'my-1.5 list-disc list-outside pl-5',
    ol: 'my-1.5 list-decimal list-outside pl-5',
    li: 'my-0.5 marker:text-low',
    hr: 'my-4 border-border',
    strong: 'font-semibold text-high',
    tableWrapper: 'my-2 overflow-x-auto rounded-sm border border-border',
    table: 'border-collapse w-full text-sm',
    th: 'bg-secondary text-high font-semibold border border-border px-3 py-1.5 text-left align-top',
    td: 'border border-border px-3 py-1.5 text-left align-top',
    codeBlock: {
      container:
        'my-2 rounded-md border border-border bg-secondary overflow-hidden',
      header:
        'flex items-center justify-between px-3 py-1 border-b border-border',
      label: 'text-xs text-low select-none',
      copyButton:
        'p-1 rounded text-low hover:text-normal hover:bg-panel transition-colors',
      pre: 'block font-mono text-sm px-3 py-2 whitespace-pre overflow-x-auto',
    },
  },
  legacy: {
    link: 'text-info underline underline-offset-2 cursor-pointer hover:text-info/80 transition-colors',
    inlineCode:
      'font-mono text-[0.875em] bg-muted border border-border rounded px-1 py-px',
    inlineCodeClickable: 'cursor-pointer',
    paragraph: 'mb-2 last:mb-0 leading-relaxed',
    headings: {
      h1: 'mt-3 mb-1.5 first:mt-0 text-xl font-semibold',
      h2: 'mt-3 mb-1.5 first:mt-0 text-lg font-semibold',
      h3: 'mt-3 mb-1.5 first:mt-0 text-base font-semibold',
      h4: 'mt-2 mb-1 first:mt-0 text-base font-medium',
      h5: 'mt-2 mb-1 first:mt-0 text-sm font-medium',
      h6: 'mt-2 mb-1 first:mt-0 text-xs font-medium uppercase tracking-wide text-muted-foreground',
    },
    quote: 'my-3 border-l-2 border-border pl-3 text-muted-foreground',
    ul: 'my-1.5 list-disc list-outside pl-5',
    ol: 'my-1.5 list-decimal list-outside pl-5',
    li: 'my-0.5 marker:text-muted-foreground',
    hr: 'my-4 border-border',
    strong: 'font-semibold',
    tableWrapper: 'my-2 overflow-x-auto',
    table: 'border-collapse w-full text-sm',
    th: 'bg-muted font-semibold border border-border px-3 py-1.5 text-left align-top',
    td: 'border border-border px-3 py-1.5 text-left align-top',
    codeBlock: {
      container:
        'my-2 rounded-md border border-border bg-secondary overflow-hidden',
      header:
        'flex items-center justify-between px-3 py-1 border-b border-border',
      label: 'text-xs text-muted-foreground select-none',
      copyButton:
        'p-1 rounded text-muted-foreground hover:text-foreground hover:bg-accent transition-colors',
      pre: 'block font-mono text-sm px-3 py-2 whitespace-pre overflow-x-auto',
    },
  },
};
