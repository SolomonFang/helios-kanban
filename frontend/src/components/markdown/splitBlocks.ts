import { Lexer } from 'marked';

/**
 * Split markdown into top-level blocks (paragraphs, headings, code fences,
 * lists, tables...) using marked's lexer. Each block keeps its raw source so
 * blocks can be memoized individually: while streaming, only the last block
 * changes and earlier blocks skip re-parsing entirely.
 */
export function splitMarkdownBlocks(markdown: string): string[] {
  if (!markdown.trim()) return [];
  const tokens = Lexer.lex(markdown, { gfm: true });
  // Drop pure-whitespace ("space") tokens — block spacing is handled by CSS
  return tokens
    .map((token) => token.raw)
    .filter((raw) => raw.trim().length > 0);
}
