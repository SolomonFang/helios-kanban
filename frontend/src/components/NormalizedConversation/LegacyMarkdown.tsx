import { StreamingMarkdown } from '@/components/markdown/StreamingMarkdown';

interface LegacyMarkdownProps {
  value: string;
  className?: string;
  taskAttemptId?: string;
  onEdit?: () => void;
  onDelete?: () => void;
  /** Accepted for WYSIWYGEditor call-site compatibility; always read-only. */
  disabled?: boolean;
}

/**
 * Read-only legacy conversation markdown renderer. Keeps the existing
 * WYSIWYGEditor prop shape while using the streaming-optimized renderer.
 */
export function LegacyMarkdown({
  value,
  className,
  taskAttemptId,
  onEdit,
  onDelete,
}: LegacyMarkdownProps) {
  return (
    <StreamingMarkdown
      content={value}
      className={className}
      workspaceId={taskAttemptId}
      variant="legacy"
      onEdit={onEdit}
      onDelete={onDelete}
    />
  );
}
