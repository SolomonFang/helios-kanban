import { useCallback } from 'react';
import { HelpCircle, Loader2 } from 'lucide-react';
import { useImageMetadata } from '@/hooks/useImageMetadata';
import { ImagePreviewDialog } from '@/components/dialogs/wysiwyg/ImagePreviewDialog';
import { formatFileSize } from '@/lib/utils';

function truncatePath(path: string, maxLength = 24): string {
  const filename = path.split('/').pop() || path;
  if (filename.length <= maxLength) return filename;
  return filename.slice(0, maxLength - 3) + '...';
}

interface MarkdownImageProps {
  src?: string;
  alt?: string;
  workspaceId?: string;
}

/**
 * Read-only image renderer for chat markdown. Mirrors the Lexical ImageNode's
 * chip UI: `.vibe-images/` paths resolve to a proxied thumbnail that opens
 * ImagePreviewDialog on click; other sources render as a plain <img>.
 */
export function MarkdownImage({ src, alt, workspaceId }: MarkdownImageProps) {
  const safeSrc = src ?? '';
  const isVibeImage = safeSrc.startsWith('.vibe-images/');
  const { data: metadata, isLoading } = useImageMetadata(
    workspaceId,
    safeSrc,
    undefined,
    []
  );

  const handleClick = useCallback(
    (event: React.MouseEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (metadata?.exists && metadata.proxy_url) {
        ImagePreviewDialog.show({
          imageUrl: metadata.proxy_url,
          altText: alt ?? '',
          fileName: metadata.file_name ?? undefined,
          format: metadata.format ?? undefined,
          sizeBytes: metadata.size_bytes,
        });
      }
    },
    [metadata, alt]
  );

  if (!isVibeImage) {
    return (
      <img
        src={safeSrc}
        alt={alt ?? ''}
        className="max-w-full rounded my-1"
        loading="lazy"
      />
    );
  }

  let thumbnail: React.ReactNode;
  if (isLoading) {
    thumbnail = (
      <span className="w-10 h-10 flex items-center justify-center bg-muted rounded shrink-0">
        <Loader2 className="w-5 h-5 text-muted-foreground animate-spin" />
      </span>
    );
  } else if (metadata?.exists && metadata.proxy_url) {
    thumbnail = (
      <img
        src={metadata.proxy_url}
        alt={alt ?? ''}
        className="w-10 h-10 object-cover rounded shrink-0"
        draggable={false}
      />
    );
  } else {
    thumbnail = (
      <span className="w-10 h-10 flex items-center justify-center bg-muted rounded shrink-0">
        <HelpCircle className="w-5 h-5 text-muted-foreground" />
      </span>
    );
  }

  const parts: string[] = [];
  if (metadata?.format) parts.push(metadata.format.toUpperCase());
  const sizeStr = formatFileSize(metadata?.size_bytes);
  if (sizeStr) parts.push(sizeStr);

  return (
    <span
      className="inline-flex items-center gap-1.5 px-1.5 py-1 ml-0.5 mr-0.5 bg-muted rounded border cursor-pointer border-border hover:border-muted-foreground transition-colors align-bottom"
      onClick={handleClick}
      role="button"
      tabIndex={0}
    >
      {thumbnail}
      <span className="flex flex-col min-w-0">
        <span className="text-xs text-muted-foreground truncate max-w-[120px]">
          {truncatePath(metadata?.file_name || alt || safeSrc)}
        </span>
        {parts.length > 0 && (
          <span className="text-[10px] text-muted-foreground/70 truncate max-w-[120px]">
            {parts.join(' · ')}
          </span>
        )}
      </span>
    </span>
  );
}
