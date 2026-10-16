import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
} from 'react';
import { Check, Clipboard, Pencil, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { writeClipboardViaBridge } from '@/vscode/bridge';
import { useChangesView } from '@/contexts/ChangesViewContext';
import { cn } from '@/lib/utils';
import { repairMarkdown } from './repairMarkdown';
import {
  splitMarkdownBlocksIncremental,
  type MarkdownBlocksState,
} from './splitBlocks';
import { MarkdownBlock } from './MarkdownBlock';
import type { MarkdownVariant } from './markdownTheme';

interface StreamingMarkdownProps {
  content: string;
  workspaceId?: string;
  className?: string;
  variant?: MarkdownVariant;
  onEdit?: () => void;
  onDelete?: () => void;
}

/**
 * Streaming-optimized markdown renderer for chat messages (Streamdown-style):
 *   1. repairMarkdown auto-closes incomplete syntax mid-stream
 *   2. marked splits the text into top-level blocks
 *   3. each block is a memoized component — only the block currently being
 *      written re-parses, earlier blocks render zero work
 *   4. block updates go through startTransition so fast token bursts stay
 *      interruptible instead of stacking up synchronous renders
 */
export function StreamingMarkdown({
  content,
  workspaceId,
  className,
  variant = 'new',
  onEdit,
  onDelete,
}: StreamingMarkdownProps) {
  const { viewFileInChanges, findMatchingDiffPath } = useChangesView();

  const repaired = useMemo(() => repairMarkdown(content), [content]);
  // Incremental splitting: streaming updates are append-only, so only the
  // tail (from the previous last block's offset) is re-lexed instead of the
  // whole message on every frame.
  const splitterRef = useRef<MarkdownBlocksState | null>(null);
  const blocks = useMemo(() => {
    const state = splitMarkdownBlocksIncremental(
      repaired,
      content,
      splitterRef.current
    );
    splitterRef.current = state;
    return state.blocks;
  }, [repaired, content]);

  const [displayBlocks, setDisplayBlocks] = useState(blocks);
  const [, startTransition] = useTransition();
  useEffect(() => {
    startTransition(() => setDisplayBlocks(blocks));
  }, [blocks]);

  // Copy as Markdown — mirrors the WYSIWYGEditor read-only hover button
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(async () => {
    if (!content) return;
    try {
      // Unescape markdown-escaped underscores for cleaner clipboard output
      await writeClipboardViaBridge(content.replace(/\\_/g, '_'));
      setCopied(true);
      window.setTimeout(() => setCopied(false), 400);
    } catch {
      // noop – bridge handles fallback
    }
  }, [content]);

  return (
    <div className="relative group">
      <div className="sticky top-0 right-2 z-10 pointer-events-none h-0">
        <div className="flex justify-end gap-1 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
          <Button
            type="button"
            aria-label={copied ? 'Copied!' : 'Copy as Markdown'}
            title={copied ? 'Copied!' : 'Copy as Markdown'}
            variant="icon"
            size="icon"
            onClick={handleCopy}
            className="pointer-events-auto p-2 bg-muted h-8 w-8"
          >
            {copied ? (
              <Check className="w-4 h-4 text-success" />
            ) : (
              <Clipboard className="w-4 h-4 text-muted-foreground" />
            )}
          </Button>
          {onEdit && (
            <Button
              type="button"
              aria-label="Edit"
              title="Edit"
              variant="icon"
              size="icon"
              onClick={onEdit}
              className="pointer-events-auto p-2 bg-muted h-8 w-8"
            >
              <Pencil className="w-4 h-4 text-muted-foreground" />
            </Button>
          )}
          {onDelete && (
            <Button
              type="button"
              aria-label="Delete"
              title="Delete"
              variant="icon"
              size="icon"
              onClick={onDelete}
              className="pointer-events-auto p-2 bg-muted h-8 w-8"
            >
              <Trash2 className="w-4 h-4 text-muted-foreground" />
            </Button>
          )}
        </div>
      </div>
      <div className={cn(variant === 'legacy' && 'wysiwyg text-base')}>
        <div className={cn('whitespace-pre-wrap break-words', className)}>
          {displayBlocks.map((block, index) => (
            <MarkdownBlock
              // Index keys are correct here: blocks only append or mutate the
              // last one, so identity per position is stable while streaming.
              key={index}
              content={block}
              workspaceId={workspaceId}
              variant={variant}
              findMatchingDiffPath={findMatchingDiffPath}
              onCodeClick={viewFileInChanges}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
