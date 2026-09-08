// streamJsonPatchEntries.ts - WebSocket JSON patch streaming utility
import { produce } from 'immer';
import type { Operation } from 'rfc6902';
import type { TokenUsageInfo } from 'shared/types';
import { applyUpsertPatch } from '@/utils/jsonPatch';
import { toWsUrl } from '@/lib/ws';

type PatchContainer<E = unknown> = { entries: E[] };

/** Extract latest token usage from normalized log entries. */
export function extractTokenUsageFromEntries(
  entries: unknown[]
): TokenUsageInfo | null {
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i] as {
      type?: string;
      content?: {
        entry_type?: {
          type?: string;
          total_tokens?: number;
          model_context_window?: number;
        };
      };
    };
    if (
      entry?.type === 'NORMALIZED_ENTRY' &&
      entry.content?.entry_type?.type === 'token_usage_info' &&
      typeof entry.content.entry_type.total_tokens === 'number' &&
      typeof entry.content.entry_type.model_context_window === 'number'
    ) {
      return {
        total_tokens: entry.content.entry_type.total_tokens,
        model_context_window: entry.content.entry_type.model_context_window,
      };
    }
  }
  return null;
}

/** Extract token usage from a JsonPatch batch (e.g. `/entries/12` add). */
export function extractTokenUsageFromPatches(
  ops: Operation[]
): TokenUsageInfo | null {
  for (const op of ops) {
    if (op.op !== 'add' && op.op !== 'replace') continue;
    if (parseEntryIndex(op.path) === null) continue;
    const value = op.value as
      | {
          type?: string;
          content?: {
            entry_type?: {
              type?: string;
              total_tokens?: number;
              model_context_window?: number;
            };
          };
        }
      | undefined;
    if (
      value?.type === 'NORMALIZED_ENTRY' &&
      value.content?.entry_type?.type === 'token_usage_info' &&
      typeof value.content.entry_type.total_tokens === 'number' &&
      typeof value.content.entry_type.model_context_window === 'number'
    ) {
      return {
        total_tokens: value.content.entry_type.total_tokens,
        model_context_window: value.content.entry_type.model_context_window,
      };
    }
  }
  return null;
}

function parseEntryIndex(path: string): number | null {
  const match = path.match(/^\/entries\/(\d+)$/);
  if (!match) return null;
  const index = Number(match[1]);
  return Number.isNaN(index) ? null : index;
}

export interface StreamOptions<E = unknown> {
  initial?: PatchContainer<E>;
  /** called after each successful patch application */
  onEntries?: (entries: E[]) => void;
  /** called when a patch adds/updates token_usage_info */
  onTokenUsage?: (info: TokenUsageInfo) => void;
  onConnect?: () => void;
  onError?: (err: unknown) => void;
  /** called once when a "finished" event is received */
  onFinished?: (entries: E[]) => void;
}

interface StreamController<E = unknown> {
  /** Current entries array (immutable snapshot) */
  getEntries(): E[];
  /** Full { entries } snapshot */
  getSnapshot(): PatchContainer<E>;
  /** Best-effort connection state */
  isConnected(): boolean;
  /** Subscribe to updates; returns an unsubscribe function */
  onChange(cb: (entries: E[]) => void): () => void;
  /** Close the stream */
  close(): void;
}

/**
 * Connect to a WebSocket endpoint that emits JSON messages containing:
 *   {"JsonPatch": [{"op": "add", "path": "/entries/0", "value": {...}}, ...]}
 *   {"finished": true}
 *
 * Maintains an in-memory { entries: [] } snapshot and returns a controller.
 * JsonPatch ops are batched per animation frame (immer + structural sharing).
 */
export function streamJsonPatchEntries<E = unknown>(
  url: string,
  opts: StreamOptions<E> = {}
): StreamController<E> {
  let connected = false;
  let snapshot: PatchContainer<E> = structuredClone(
    opts.initial ?? ({ entries: [] } as PatchContainer<E>)
  );

  const subscribers = new Set<(entries: E[]) => void>();
  if (opts.onEntries) subscribers.add(opts.onEntries);

  const wsUrl = toWsUrl(url);
  const ws = new WebSocket(wsUrl);

  let pendingOps: Operation[] = [];
  let rafId: number | null = null;

  const getVisibleEntries = (): E[] =>
    snapshot.entries.filter(
      (entry): entry is E => entry != null && entry !== undefined
    );

  const notify = () => {
    // Pass the full sparse array so consumers can keep stable /entries/N patchKeys.
    const entries = snapshot.entries;
    for (const cb of subscribers) {
      try {
        cb(entries as E[]);
      } catch (err) {
        console.error('streamJsonPatchEntries subscriber error:', err);
      }
    }
  };

  const flush = () => {
    rafId = null;
    if (pendingOps.length === 0) return;

    const ops = dedupeOps(pendingOps);
    pendingOps = [];

    const tokenUsage = extractTokenUsageFromPatches(ops);
    if (tokenUsage) {
      opts.onTokenUsage?.(tokenUsage);
    }

    snapshot = produce(snapshot, (draft) => {
      applyUpsertPatch(draft, ops);
    });
    notify();
  };

  const scheduleFlush = () => {
    if (rafId === null) {
      rafId = requestAnimationFrame(flush);
    }
  };

  const handleMessage = (event: MessageEvent) => {
    try {
      const msg = JSON.parse(event.data);

      if (msg.JsonPatch) {
        const raw = msg.JsonPatch as Operation[];
        pendingOps.push(...raw);
        scheduleFlush();
      }

      if (msg.finished !== undefined) {
        if (rafId !== null) {
          cancelAnimationFrame(rafId);
          rafId = null;
        }
        flush();
        opts.onFinished?.(getVisibleEntries());
        ws.close();
      }
    } catch (err) {
      opts.onError?.(err);
    }
  };

  ws.addEventListener('open', () => {
    connected = true;
    opts.onConnect?.();
  });

  ws.addEventListener('message', handleMessage);

  ws.addEventListener('error', (err) => {
    connected = false;
    opts.onError?.(err);
  });

  ws.addEventListener('close', () => {
    connected = false;
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  });

  return {
    getEntries(): E[] {
      return getVisibleEntries();
    },
    getSnapshot(): PatchContainer<E> {
      return { entries: getVisibleEntries() };
    },
    isConnected(): boolean {
      return connected;
    },
    onChange(cb: (entries: E[]) => void): () => void {
      subscribers.add(cb);
      cb(getVisibleEntries());
      return () => subscribers.delete(cb);
    },
    close(): void {
      if (rafId !== null) {
        cancelAnimationFrame(rafId);
        rafId = null;
      }
      ws.close();
      subscribers.clear();
      connected = false;
    },
  };
}

/**
 * Dedupe multiple ops that touch the same path within a single event.
 * Last write for a path wins, while preserving the overall left-to-right
 * order of the *kept* final operations.
 */
function dedupeOps(ops: Operation[]): Operation[] {
  const lastIndexByPath = new Map<string, number>();
  ops.forEach((op, i) => lastIndexByPath.set(op.path, i));

  const keptIndices = [...lastIndexByPath.values()].sort((a, b) => a - b);
  return keptIndices.map((i) => ops[i]!);
}
