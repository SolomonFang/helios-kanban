import { StreamingMarkdown } from '@/components/markdown/StreamingMarkdown';
import { cn } from '@/lib/utils';

interface ChatMarkdownProps {
  content: string;
  maxWidth?: string;
  className?: string;
  workspaceId?: string;
}

export function ChatMarkdown({
  content,
  maxWidth = '800px',
  className,
  workspaceId,
}: ChatMarkdownProps) {
  return (
    <div className="text-sm" style={{ maxWidth }}>
      <StreamingMarkdown
        content={content}
        workspaceId={workspaceId}
        className={cn('whitespace-pre-wrap break-words', className)}
      />
    </div>
  );
}
