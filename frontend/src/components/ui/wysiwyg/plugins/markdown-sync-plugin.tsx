import { useEffect, useRef } from 'react';
import { useLexicalComposerContext } from '@lexical/react/LexicalComposerContext';
import {
  $convertToMarkdownString,
  $convertFromMarkdownString,
  type Transformer,
} from '@lexical/markdown';
import { $getRoot, type EditorState } from 'lexical';

/**
 * Read-only editors render streaming content; parsing the full markdown on
 * every frame is the main streaming hotspot, so updates are throttled to one
 * parse per interval (trailing edge guarantees the final value is applied).
 */
const READ_ONLY_SYNC_INTERVAL_MS = 150;

type MarkdownSyncPluginProps = {
  value: string;
  onChange?: (markdown: string) => void;
  onEditorStateChange?: (state: EditorState) => void;
  editable: boolean;
  transformers: Transformer[];
};

/**
 * Handles bidirectional markdown synchronization between Lexical editor and external state.
 *
 * Uses an internal ref to prevent infinite update loops during bidirectional sync.
 */
export function MarkdownSyncPlugin({
  value,
  onChange,
  onEditorStateChange,
  editable,
  transformers,
}: MarkdownSyncPluginProps) {
  const [editor] = useLexicalComposerContext();
  const lastSerializedRef = useRef<string | undefined>(undefined);
  const pendingValueRef = useRef<string | null>(null);
  const throttleTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastAppliedAtRef = useRef(0);

  // Handle editable state
  useEffect(() => {
    editor.setEditable(editable);
  }, [editor, editable]);

  // Handle controlled value changes (external → editor)
  useEffect(() => {
    if (value === lastSerializedRef.current) return;

    const applyValue = (nextValue: string) => {
      try {
        editor.update(() => {
          if (nextValue.trim() === '') {
            $getRoot().clear();
          } else {
            $convertFromMarkdownString(nextValue, transformers);
          }

          // Only position cursor at end if editor already has focus (user is actively editing)
          // This prevents unwanted focus when value changes externally (e.g., panel opening)
          const rootElement = editor.getRootElement();
          if (rootElement?.contains(document.activeElement)) {
            const root = $getRoot();
            const lastNode = root.getLastChild();
            if (lastNode) {
              lastNode.selectEnd();
            }
          }
        });
        lastSerializedRef.current = nextValue;
      } catch (err) {
        console.error('Failed to parse markdown', err);
      }
    };

    if (editable) {
      applyValue(value);
      return;
    }

    // Read-only (streaming) mode: throttle full markdown re-parses.
    // Leading edge applies immediately so first paint is not delayed;
    // trailing edge guarantees the final streamed value is rendered.
    const now = Date.now();
    const elapsed = now - lastAppliedAtRef.current;
    if (elapsed >= READ_ONLY_SYNC_INTERVAL_MS) {
      lastAppliedAtRef.current = now;
      applyValue(value);
      return;
    }

    pendingValueRef.current = value;
    if (throttleTimerRef.current !== null) return;

    throttleTimerRef.current = setTimeout(() => {
      throttleTimerRef.current = null;
      const pending = pendingValueRef.current;
      pendingValueRef.current = null;
      if (pending !== null && pending !== lastSerializedRef.current) {
        lastAppliedAtRef.current = Date.now();
        applyValue(pending);
      }
    }, READ_ONLY_SYNC_INTERVAL_MS - elapsed);
  }, [editor, value, editable, transformers]);

  // Flush any pending read-only value and clear the throttle timer on unmount
  // or when switching to editable mode.
  useEffect(() => {
    return () => {
      if (throttleTimerRef.current !== null) {
        clearTimeout(throttleTimerRef.current);
        throttleTimerRef.current = null;
        pendingValueRef.current = null;
      }
    };
  }, []);

  // Handle editor changes (editor → external)
  useEffect(() => {
    return editor.registerUpdateListener(({ editorState }) => {
      onEditorStateChange?.(editorState);
      if (!onChange) return;

      const markdown = editorState.read(() =>
        $convertToMarkdownString(transformers)
      );
      if (markdown === lastSerializedRef.current) return;

      lastSerializedRef.current = markdown;
      onChange(markdown);
    });
  }, [editor, onChange, onEditorStateChange, transformers]);

  return null;
}
