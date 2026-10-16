import { useCallback, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import { Check, Clipboard } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { writeClipboardViaBridge } from '@/vscode/bridge';

interface Anchor {
  codeEl: HTMLElement;
  top: number;
  left: number;
}

/**
 * Read-only code block action menu (ChatGPT-style): hovering a fenced code
 * block reveals a floating copy button anchored to its top-right corner.
 * Code blocks are identified by the data-language attribute Lexical's
 * CodeNode puts on its block-level <code> element, which also excludes
 * inline code spans. The persistent language label is rendered via CSS
 * (styles/legacy/index.css ::before on code[data-language]).
 */
export function CodeActionMenuPlugin() {
  const [editor] = useLexicalComposerContext();
  const { t } = useTranslation('common');
  const [anchor, setAnchor] = useState<Anchor | null>(null);
  const [copied, setCopied] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const hideTimerRef = useRef<number | null>(null);

  const clearHideTimer = useCallback(() => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  }, []);

  const scheduleHide = useCallback(() => {
    clearHideTimer();
    hideTimerRef.current = window.setTimeout(() => setAnchor(null), 120);
  }, [clearHideTimer]);

  useEffect(() => {
    const root = editor.getRootElement();
    if (!root) return;

    const findCodeBlock = (node: EventTarget | null): HTMLElement | null =>
      node instanceof HTMLElement ? node.closest('code[data-language]') : null;

    const onMouseOver = (event: MouseEvent) => {
      const codeEl = findCodeBlock(event.target);
      if (!codeEl) return;
      clearHideTimer();
      setCopied(false);
      const rect = codeEl.getBoundingClientRect();
      setAnchor({ codeEl, top: rect.top + 4, left: rect.right - 4 });
    };

    const onMouseOut = (event: MouseEvent) => {
      const codeEl = findCodeBlock(event.target);
      if (!codeEl) return;
      const next = event.relatedTarget;
      if (
        next instanceof Node &&
        (codeEl.contains(next) || (menuRef.current?.contains(next) ?? false))
      ) {
        return;
      }
      scheduleHide();
    };

    // Hide on any scroll — the anchor is viewport-fixed and would drift.
    const onScroll = () => setAnchor(null);

    root.addEventListener('mouseover', onMouseOver);
    root.addEventListener('mouseout', onMouseOut);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      root.removeEventListener('mouseover', onMouseOver);
      root.removeEventListener('mouseout', onMouseOut);
      window.removeEventListener('scroll', onScroll, true);
      clearHideTimer();
    };
  }, [editor, clearHideTimer, scheduleHide]);

  const handleCopy = useCallback(async () => {
    if (!anchor) return;
    try {
      await writeClipboardViaBridge(anchor.codeEl.innerText);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 400);
    } catch {
      // noop – bridge handles fallback
    }
  }, [anchor]);

  if (!anchor) return null;

  return createPortal(
    <div
      ref={menuRef}
      className="fixed z-50 rounded border border-border bg-muted p-0.5 shadow-sm"
      style={{
        top: anchor.top,
        left: anchor.left,
        transform: 'translateX(-100%)',
      }}
      onMouseEnter={clearHideTimer}
      onMouseLeave={scheduleHide}
    >
      <button
        type="button"
        aria-label={copied ? t('actions.copied') : t('actions.copyCode')}
        title={copied ? t('actions.copied') : t('actions.copyCode')}
        onClick={handleCopy}
        className="p-1 rounded text-muted-foreground hover:text-foreground hover:bg-accent transition-colors"
      >
        {copied ? (
          <Check className="w-3.5 h-3.5 text-success" />
        ) : (
          <Clipboard className="w-3.5 h-3.5" />
        )}
      </button>
    </div>,
    document.body
  );
}
