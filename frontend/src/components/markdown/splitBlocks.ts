import { Lexer } from 'marked';

/**
 * Split markdown into top-level blocks (paragraphs, headings, code fences,
 * lists, tables...) using marked's lexer. Each block keeps its raw source so
 * blocks can be memoized individually: while streaming, only the last block
 * changes and earlier blocks skip re-parsing entirely.
 */
export function splitMarkdownBlocks(markdown: string): string[] {
  return lexBlocks(markdown).blocks;
}

export interface MarkdownBlocksState {
  /** Raw content (before repair) that produced this state */
  content: string;
  /** The repaired text that was lexed */
  repaired: string;
  blocks: string[];
  /** Start offset of each block within `repaired` (token.raw concatenates to the input) */
  offsets: number[];
}

function lexBlocks(
  markdown: string,
  baseOffset = 0
): { blocks: string[]; offsets: number[] } {
  const tokens = Lexer.lex(markdown, { gfm: true });
  // Drop pure-whitespace ("space") tokens — block spacing is handled by CSS.
  // Their raw still counts toward offsets so positions stay exact.
  const blocks: string[] = [];
  const offsets: number[] = [];
  let offset = baseOffset;
  for (const token of tokens) {
    if (token.raw.trim().length > 0) {
      blocks.push(token.raw);
      offsets.push(offset);
    }
    offset += token.raw.length;
  }
  return { blocks, offsets };
}

/**
 * Incremental variant for streaming: when the new content is append-only
 * relative to the previous state, only the tail starting at the previous
 * last block is re-lexed and earlier blocks are reused unchanged. Any merge
 * between old and new text (lazy paragraph continuation, setext headings,
 * list/table growth) always involves the last block, so re-lexing from its
 * start offset stays correct. Falls back to a full lex on edits, non-append
 * changes, or when the offset invariants do not hold.
 */
export function splitMarkdownBlocksIncremental(
  repaired: string,
  content: string,
  prev: MarkdownBlocksState | null
): MarkdownBlocksState {
  if (!repaired.trim()) {
    return { content, repaired, blocks: [], offsets: [] };
  }

  const canReUsePrefix =
    prev !== null &&
    prev.blocks.length > 0 &&
    content.startsWith(prev.content) &&
    // The last block must start inside the raw content, not inside a
    // repair-appended suffix, so the same offset is valid in `repaired`.
    prev.offsets[prev.offsets.length - 1] <= prev.content.length &&
    repaired.startsWith(
      prev.repaired.slice(0, prev.offsets[prev.offsets.length - 1])
    );

  if (!canReUsePrefix || !prev) {
    const { blocks, offsets } = lexBlocks(repaired);
    return { content, repaired, blocks, offsets };
  }

  const lastBlockStart = prev.offsets[prev.offsets.length - 1];
  const tail = lexBlocks(repaired.slice(lastBlockStart), lastBlockStart);
  return {
    content,
    repaired,
    blocks: [...prev.blocks.slice(0, -1), ...tail.blocks],
    offsets: [...prev.offsets.slice(0, -1), ...tail.offsets],
  };
}
