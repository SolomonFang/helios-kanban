import {
  CommandExitStatus,
  ExecutionProcess,
  ExecutionProcessStatus,
  NormalizedEntry,
  PatchType,
  TokenUsageInfo,
  ToolStatus,
} from 'shared/types';
import { useExecutionProcessesContext } from '@/contexts/ExecutionProcessesContext';
import { useSetTokenUsageInfo } from '@/contexts/EntriesContext';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  extractTokenUsageFromEntries,
  streamJsonPatchEntries,
} from '@/utils/streamJsonPatchEntries';
import type {
  AddEntryType,
  ExecutionProcessStateStore,
  OnEntriesUpdated,
  PatchTypeWithKey,
  UseConversationHistoryParams,
} from '@/hooks/useConversationHistory/types';

// Result type for the new UI's conversation history hook
export interface UseConversationHistoryResult {
  /** Whether a setup script has already run in this conversation */
  hasSetupScriptRun: boolean;
  /** Whether a cleanup script has already run in this conversation */
  hasCleanupScriptRun: boolean;
  /** Whether there is currently a running process */
  hasRunningProcess: boolean;
}
import {
  makeLoadingPatch,
  MIN_INITIAL_ENTRIES,
  REMAINING_BATCH_SIZE,
} from '@/hooks/useConversationHistory/constants';

// created_at strings never change for a given process, so the parsed
// timestamp is cached to avoid re-parsing on every sort comparison.
const getCachedProcessSortKey = (
  p: ExecutionProcessStateStore[string],
  cache: Map<string, number>
): number => {
  const id = p.executionProcess.id;
  const cached = cache.get(id);
  if (cached !== undefined) return cached;
  const key = new Date(
    p.executionProcess.created_at as unknown as string
  ).getTime();
  cache.set(id, key);
  return key;
};

export type {
  AddEntryType,
  OnEntriesUpdated,
  PatchTypeWithKey,
  DisplayEntry,
  AggregatedPatchGroup,
  AggregatedDiffGroup,
  AggregatedThinkingGroup,
} from '@/hooks/useConversationHistory/types';

export {
  isAggregatedGroup,
  isAggregatedDiffGroup,
  isAggregatedThinkingGroup,
} from '@/hooks/useConversationHistory/types';

export const useConversationHistory = ({
  attempt,
  onEntriesUpdated,
}: UseConversationHistoryParams): UseConversationHistoryResult => {
  const {
    executionProcessesVisible: executionProcessesRaw,
    isLoading,
    isConnected,
  } = useExecutionProcessesContext();
  const { setTokenUsageInfo } = useSetTokenUsageInfo();
  const executionProcesses = useRef<ExecutionProcess[]>(executionProcessesRaw);
  const displayedExecutionProcesses = useRef<ExecutionProcessStateStore>({});
  const loadedInitialEntries = useRef(false);
  const streamingProcessIdsRef = useRef<Set<string>>(new Set());
  const onEntriesUpdatedRef = useRef<OnEntriesUpdated | null>(null);
  const previousStatusMapRef = useRef<Map<string, ExecutionProcessStatus>>(
    new Map()
  );
  const latestTokenUsageRef = useRef<TokenUsageInfo | null>(null);

  // Reference-stability caches: unchanged entries keep their identity across
  // emits so downstream memoized rows skip re-rendering during streaming.
  const wrappedPatchCacheRef = useRef<
    Map<string, { source: PatchType; wrapped: PatchTypeWithKey }>
  >(new Map());
  const processSortKeyCacheRef = useRef<Map<string, number>>(new Map());
  const userPatchCacheRef = useRef<
    Map<string, { prompt: string; wrapped: PatchTypeWithKey }>
  >(new Map());
  const loadingPatchCacheRef = useRef<Map<string, PatchTypeWithKey>>(new Map());
  type ProcessSliceCacheEntry = {
    entriesRef: PatchTypeWithKey[];
    processRef: ExecutionProcessStateStore[string]['executionProcess'];
    liveStatus: ExecutionProcessStatus | undefined;
    liveExitCode: number | null;
    slice: PatchTypeWithKey[];
    tokenUsageInfo: TokenUsageInfo | null;
    contributesRunning: boolean;
  };
  const processSliceCacheRef = useRef<Map<string, ProcessSliceCacheEntry>>(
    new Map()
  );

  // Track whether scripts have run in this conversation
  const [hasSetupScriptRun, setHasSetupScriptRun] = useState(false);
  const [hasCleanupScriptRun, setHasCleanupScriptRun] = useState(false);
  const [hasRunningProcess, setHasRunningProcess] = useState(false);

  const mergeIntoDisplayed = (
    mutator: (state: ExecutionProcessStateStore) => void
  ) => {
    const state = displayedExecutionProcesses.current;
    mutator(state);
  };
  useEffect(() => {
    onEntriesUpdatedRef.current = onEntriesUpdated;
  }, [onEntriesUpdated]);

  // Keep executionProcesses up to date
  useEffect(() => {
    executionProcesses.current = executionProcessesRaw.filter(
      (ep) =>
        ep.run_reason === 'setupscript' ||
        ep.run_reason === 'cleanupscript' ||
        ep.run_reason === 'archivescript' ||
        ep.run_reason === 'codingagent'
    );
  }, [executionProcessesRaw]);

  // Reset displayed state when workspace changes (must run before load/stream effects)
  useEffect(() => {
    displayedExecutionProcesses.current = {};
    loadedInitialEntries.current = false;
    latestTokenUsageRef.current = null;
    streamingProcessIdsRef.current.clear();
    previousStatusMapRef.current.clear();
    wrappedPatchCacheRef.current.clear();
    processSortKeyCacheRef.current.clear();
    userPatchCacheRef.current.clear();
    loadingPatchCacheRef.current.clear();
    processSliceCacheRef.current.clear();
    setHasSetupScriptRun(false);
    setHasCleanupScriptRun(false);
    setHasRunningProcess(false);
    // Do not emit here — empty state would only produce a non-renderable next_action row
  }, [attempt.id]);

  const loadEntriesForHistoricExecutionProcess = useCallback(
    (executionProcess: ExecutionProcess) => {
      let url = '';
      if (executionProcess.executor_action.typ.type === 'ScriptRequest') {
        url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;
      } else {
        url = `/api/execution-processes/${executionProcess.id}/normalized-logs/ws`;
      }

      return new Promise<PatchType[]>((resolve) => {
        const controller = streamJsonPatchEntries<PatchType>(url, {
          onTokenUsage: (info) => {
            latestTokenUsageRef.current = info;
            setTokenUsageInfo(info);
          },
          onFinished: (allEntries) => {
            controller.close();
            resolve(allEntries);
          },
          onError: (err) => {
            console.warn(
              `Error loading entries for historic execution process ${executionProcess.id}`,
              err
            );
            controller.close();
            resolve([]);
          },
        });
      });
    },
    [setTokenUsageInfo]
  );

  const getLiveExecutionProcess = (
    executionProcessId: string
  ): ExecutionProcess | undefined => {
    return executionProcesses?.current.find(
      (executionProcess) => executionProcess.id === executionProcessId
    );
  };

  // Wraps a patch with its display key. immer's structural sharing keeps
  // unchanged entries referentially equal across frames, so the cached wrapper
  // can be reused and downstream memoized rows skip re-rendering.
  const patchWithKey = (
    patch: PatchType,
    executionProcessId: string,
    index: number | 'user'
  ): PatchTypeWithKey => {
    const patchKey = `${executionProcessId}:${index}`;
    const cached = wrappedPatchCacheRef.current.get(patchKey);
    if (cached && cached.source === patch) {
      return cached.wrapped;
    }
    const wrapped: PatchTypeWithKey = {
      ...patch,
      patchKey,
      executionProcessId,
    };
    wrappedPatchCacheRef.current.set(patchKey, { source: patch, wrapped });
    return wrapped;
  };

  // Drop all cached wrappers/slices belonging to a removed process.
  const evictProcessCaches = (executionProcessId: string) => {
    processSliceCacheRef.current.delete(executionProcessId);
    processSortKeyCacheRef.current.delete(executionProcessId);
    userPatchCacheRef.current.delete(executionProcessId);
    loadingPatchCacheRef.current.delete(executionProcessId);
    const prefix = `${executionProcessId}:`;
    for (const key of wrappedPatchCacheRef.current.keys()) {
      if (key.startsWith(prefix)) {
        wrappedPatchCacheRef.current.delete(key);
      }
    }
  };

  const flattenEntries = (
    executionProcessState: ExecutionProcessStateStore
  ): PatchTypeWithKey[] => {
    return Object.values(executionProcessState)
      .filter(
        (p) =>
          p.executionProcess.executor_action.typ.type ===
            'CodingAgentFollowUpRequest' ||
          p.executionProcess.executor_action.typ.type ===
            'CodingAgentInitialRequest' ||
          p.executionProcess.executor_action.typ.type === 'ReviewRequest'
      )
      .sort(
        (a, b) =>
          getCachedProcessSortKey(a, processSortKeyCacheRef.current) -
          getCachedProcessSortKey(b, processSortKeyCacheRef.current)
      )
      .flatMap((p) => p.entries);
  };

  const getActiveAgentProcesses = (): ExecutionProcess[] => {
    return (
      executionProcesses?.current.filter(
        (p) =>
          p.status === ExecutionProcessStatus.running &&
          p.run_reason !== 'devserver'
      ) ?? []
    );
  };

  // Computes one process's contribution to the emitted entry list. Pure
  // except for the setup/cleanup-run tracking side effects.
  const computeProcessSlice = (
    p: ExecutionProcessStateStore[string],
    liveProcess: ExecutionProcess | undefined
  ): {
    slice: PatchTypeWithKey[];
    tokenUsageInfo: TokenUsageInfo | null;
    contributesRunning: boolean;
  } => {
    const entries: PatchTypeWithKey[] = [];
    let tokenUsageInfo: TokenUsageInfo | null = null;
    let contributesRunning = false;

    if (
      p.executionProcess.executor_action.typ.type ===
        'CodingAgentInitialRequest' ||
      p.executionProcess.executor_action.typ.type ===
        'CodingAgentFollowUpRequest' ||
      p.executionProcess.executor_action.typ.type === 'ReviewRequest'
    ) {
      // New user message (wrapper cached by prompt so the user row keeps a
      // stable reference even when this slice is rebuilt)
      const actionType = p.executionProcess.executor_action.typ;
      const userCached = userPatchCacheRef.current.get(p.executionProcess.id);
      let userPatchWithKey: PatchTypeWithKey;
      if (userCached && userCached.prompt === actionType.prompt) {
        userPatchWithKey = userCached.wrapped;
      } else {
        userPatchWithKey = {
          type: 'NORMALIZED_ENTRY',
          content: {
            entry_type: { type: 'user_message' },
            content: actionType.prompt,
            timestamp: null,
          },
          patchKey: `${p.executionProcess.id}:user`,
          executionProcessId: p.executionProcess.id,
        };
        userPatchCacheRef.current.set(p.executionProcess.id, {
          prompt: actionType.prompt,
          wrapped: userPatchWithKey,
        });
      }
      entries.push(userPatchWithKey);

      // Extract latest token usage info before filtering
      for (let i = p.entries.length - 1; i >= 0; i--) {
        const e = p.entries[i];
        if (
          e?.type === 'NORMALIZED_ENTRY' &&
          e.content.entry_type.type === 'token_usage_info'
        ) {
          const et = e.content.entry_type;
          tokenUsageInfo = {
            total_tokens: et.total_tokens,
            model_context_window: et.model_context_window,
          };
          break;
        }
      }

      // Remove user messages (replaced with custom one) and token usage info (displayed separately)
      const entriesExcludingUser = p.entries.filter(
        (e) =>
          e.type !== 'NORMALIZED_ENTRY' ||
          (e.content.entry_type.type !== 'user_message' &&
            e.content.entry_type.type !== 'token_usage_info')
      );

      entries.push(...entriesExcludingUser);

      if (liveProcess?.status === ExecutionProcessStatus.running) {
        contributesRunning = true;
        let loadingPatch = loadingPatchCacheRef.current.get(
          p.executionProcess.id
        );
        if (!loadingPatch) {
          loadingPatch = makeLoadingPatch(p.executionProcess.id);
          loadingPatchCacheRef.current.set(p.executionProcess.id, loadingPatch);
        }
        entries.push(loadingPatch);
      }
    } else if (
      p.executionProcess.executor_action.typ.type === 'ScriptRequest'
    ) {
      // Add setup and cleanup script as a tool call
      let toolName = '';
      const scriptContext = p.executionProcess.executor_action.typ.context;
      switch (scriptContext) {
        case 'SetupScript':
          toolName = 'Setup Script';
          break;
        case 'CleanupScript':
          toolName = 'Cleanup Script';
          break;
        case 'ArchiveScript':
          toolName = 'Archive Script';
          break;
        case 'ToolInstallScript':
          toolName = 'Tool Install Script';
          break;
        default:
          return { slice: [], tokenUsageInfo: null, contributesRunning };
      }

      // Track that setup/cleanup scripts have run
      if (scriptContext === 'SetupScript') {
        setHasSetupScriptRun(true);
      } else if (scriptContext === 'CleanupScript') {
        setHasCleanupScriptRun(true);
      }

      if (liveProcess?.status === ExecutionProcessStatus.running) {
        contributesRunning = true;
      }

      const exitCode = Number(liveProcess?.exit_code) || 0;
      const exit_status: CommandExitStatus | null =
        liveProcess?.status === 'running'
          ? null
          : {
              type: 'exit_code',
              code: exitCode,
            };

      const toolStatus: ToolStatus =
        liveProcess?.status === ExecutionProcessStatus.running
          ? { status: 'created' }
          : exitCode === 0
            ? { status: 'success' }
            : { status: 'failed' };

      const output = p.entries.map((line) => line.content).join('\n');

      const toolNormalizedEntry: NormalizedEntry = {
        entry_type: {
          type: 'tool_use',
          tool_name: toolName,
          action_type: {
            action: 'command_run',
            command: p.executionProcess.executor_action.typ.script,
            result: {
              output,
              exit_status,
            },
          },
          status: toolStatus,
        },
        content: toolName,
        timestamp: null,
      };
      const toolPatch: PatchType = {
        type: 'NORMALIZED_ENTRY',
        content: toolNormalizedEntry,
      };

      entries.push({
        ...toolPatch,
        patchKey: `${p.executionProcess.id}:0`,
        executionProcessId: p.executionProcess.id,
      });
    }

    return { slice: entries, tokenUsageInfo, contributesRunning };
  };

  const flattenEntriesForEmit = useCallback(
    (executionProcessState: ExecutionProcessStateStore): PatchTypeWithKey[] => {
      let hasRunningProcess = false;
      let latestTokenUsageInfo: TokenUsageInfo | null = null;

      // Create user messages + tool calls for setup/cleanup scripts.
      // Per-process slices are cached: a slice is only rebuilt when the
      // process's entries array, live status/exit code, or process object
      // actually changes, so historic processes keep stable references
      // while an unrelated process is streaming.
      const sortedProcesses = Object.values(executionProcessState).sort(
        (a, b) =>
          getCachedProcessSortKey(a, processSortKeyCacheRef.current) -
          getCachedProcessSortKey(b, processSortKeyCacheRef.current)
      );

      const allEntries: PatchTypeWithKey[] = [];
      for (const p of sortedProcesses) {
        const liveProcess = getLiveExecutionProcess(p.executionProcess.id);
        const liveStatus = liveProcess?.status;
        const liveExitCode =
          liveProcess?.exit_code != null ? Number(liveProcess.exit_code) : null;

        const cached = processSliceCacheRef.current.get(p.executionProcess.id);
        let result: {
          slice: PatchTypeWithKey[];
          tokenUsageInfo: TokenUsageInfo | null;
          contributesRunning: boolean;
        };
        if (
          cached &&
          cached.entriesRef === p.entries &&
          cached.processRef === p.executionProcess &&
          cached.liveStatus === liveStatus &&
          cached.liveExitCode === liveExitCode
        ) {
          result = cached;
        } else {
          result = computeProcessSlice(p, liveProcess);
          processSliceCacheRef.current.set(p.executionProcess.id, {
            entriesRef: p.entries,
            processRef: p.executionProcess,
            liveStatus,
            liveExitCode,
            ...result,
          });
        }

        if (result.contributesRunning) hasRunningProcess = true;
        if (result.tokenUsageInfo !== null) {
          latestTokenUsageInfo = result.tokenUsageInfo;
        }
        allEntries.push(...result.slice);
      }

      // Update running process state
      setHasRunningProcess(hasRunningProcess);

      // next_action is not rendered in the new UI (NewDisplayConversationEntry returns null)

      // Keep the latest known token usage; do not clear when entries lack it yet
      if (latestTokenUsageInfo !== null) {
        latestTokenUsageRef.current = latestTokenUsageInfo;
      }
      if (latestTokenUsageRef.current !== null) {
        setTokenUsageInfo(latestTokenUsageRef.current);
      }

      return allEntries;
    },
    [setTokenUsageInfo]
  );

  const emitEntries = useCallback(
    (
      executionProcessState: ExecutionProcessStateStore,
      addEntryType: AddEntryType,
      loading: boolean
    ) => {
      const entries = flattenEntriesForEmit(executionProcessState);
      let modifiedAddEntryType = addEntryType;

      // Modify so that if last entry is ExitPlanMode, emit special plan type
      if (entries.length > 0) {
        const lastEntry = entries[entries.length - 1];
        if (
          lastEntry.type === 'NORMALIZED_ENTRY' &&
          lastEntry.content.entry_type.type === 'tool_use' &&
          lastEntry.content.entry_type.tool_name === 'ExitPlanMode'
        ) {
          modifiedAddEntryType = 'plan';
        }
      }

      const tokenUsage =
        latestTokenUsageRef.current ??
        extractTokenUsageFromEntries(
          Object.values(executionProcessState).flatMap((p) => p.entries)
        );
      if (tokenUsage) {
        latestTokenUsageRef.current = tokenUsage;
      }

      onEntriesUpdatedRef.current?.(
        entries,
        modifiedAddEntryType,
        loading,
        tokenUsage
      );
    },
    [flattenEntriesForEmit]
  );

  // This emits its own events as they are streamed
  const loadRunningAndEmit = useCallback(
    (executionProcess: ExecutionProcess): Promise<void> => {
      return new Promise((resolve, reject) => {
        let url = '';
        if (executionProcess.executor_action.typ.type === 'ScriptRequest') {
          url = `/api/execution-processes/${executionProcess.id}/raw-logs/ws`;
        } else {
          url = `/api/execution-processes/${executionProcess.id}/normalized-logs/ws`;
        }
        const controller = streamJsonPatchEntries<PatchType>(url, {
          onTokenUsage: (info) => {
            latestTokenUsageRef.current = info;
            setTokenUsageInfo(info);
          },
          onEntries(entries) {
            const tokenFromSnapshot = extractTokenUsageFromEntries(entries);
            if (tokenFromSnapshot) {
              latestTokenUsageRef.current = tokenFromSnapshot;
              setTokenUsageInfo(tokenFromSnapshot);
            }

            const patchesWithKey: PatchTypeWithKey[] = [];
            entries.forEach((entry, index) => {
              if (entry != null) {
                patchesWithKey.push(
                  patchWithKey(entry, executionProcess.id, index)
                );
              }
            });
            mergeIntoDisplayed((state) => {
              state[executionProcess.id] = {
                executionProcess,
                entries: patchesWithKey,
              };
            });
            emitEntries(displayedExecutionProcesses.current, 'running', false);
          },
          onFinished: () => {
            emitEntries(displayedExecutionProcesses.current, 'running', false);
            controller.close();
            resolve();
          },
          onError: (err) => {
            console.warn(
              `normalized-logs stream error for process ${executionProcess.id}:`,
              err
            );
            controller.close();
            reject(err);
          },
        });
      });
    },
    [emitEntries, setTokenUsageInfo]
  );

  // Sometimes it can take a few seconds for the stream to start, wrap the loadRunningAndEmit method
  const loadRunningAndEmitWithBackoff = useCallback(
    async (executionProcess: ExecutionProcess) => {
      for (let i = 0; i < 20; i++) {
        try {
          await loadRunningAndEmit(executionProcess);
          break;
        } catch (_) {
          await new Promise((resolve) => setTimeout(resolve, 500));
        }
      }
    },
    [loadRunningAndEmit]
  );

  const loadInitialEntries =
    useCallback(async (): Promise<ExecutionProcessStateStore> => {
      const localDisplayedExecutionProcesses: ExecutionProcessStateStore = {};

      if (!executionProcesses?.current) return localDisplayedExecutionProcesses;

      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        if (executionProcess.status === ExecutionProcessStatus.running)
          continue;

        const entries =
          await loadEntriesForHistoricExecutionProcess(executionProcess);
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        localDisplayedExecutionProcesses[executionProcess.id] = {
          executionProcess,
          entries: entriesWithKey,
        };

        if (
          flattenEntries(localDisplayedExecutionProcesses).length >
          MIN_INITIAL_ENTRIES
        ) {
          break;
        }
      }

      return localDisplayedExecutionProcesses;
    }, [executionProcesses, loadEntriesForHistoricExecutionProcess]);

  const loadRemainingEntriesInBatches = useCallback(
    async (batchSize: number): Promise<boolean> => {
      if (!executionProcesses?.current) return false;

      let anyUpdated = false;
      for (const executionProcess of [
        ...executionProcesses.current,
      ].reverse()) {
        const current = displayedExecutionProcesses.current;
        if (
          current[executionProcess.id] ||
          executionProcess.status === ExecutionProcessStatus.running
        )
          continue;

        const entries =
          await loadEntriesForHistoricExecutionProcess(executionProcess);
        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, executionProcess.id, idx)
        );

        mergeIntoDisplayed((state) => {
          state[executionProcess.id] = {
            executionProcess,
            entries: entriesWithKey,
          };
        });

        if (
          flattenEntries(displayedExecutionProcesses.current).length > batchSize
        ) {
          anyUpdated = true;
          break;
        }
        anyUpdated = true;
      }
      return anyUpdated;
    },
    [executionProcesses, loadEntriesForHistoricExecutionProcess]
  );

  const ensureProcessVisible = useCallback((p: ExecutionProcess) => {
    mergeIntoDisplayed((state) => {
      if (!state[p.id]) {
        state[p.id] = {
          executionProcess: {
            id: p.id,
            created_at: p.created_at,
            updated_at: p.updated_at,
            executor_action: p.executor_action,
          },
          entries: [],
        };
      }
    });
  }, []);

  const idListKey = useMemo(
    () => executionProcessesRaw?.map((p) => p.id).join(','),
    [executionProcessesRaw]
  );

  const idStatusKey = useMemo(
    () => executionProcessesRaw?.map((p) => `${p.id}:${p.status}`).join(','),
    [executionProcessesRaw]
  );

  // Clean up entries for processes that have been removed (e.g., after reset)
  useEffect(() => {
    if (isLoading || !isConnected) return;
    const visibleProcessIds = new Set(executionProcessesRaw.map((p) => p.id));
    const displayedIds = Object.keys(displayedExecutionProcesses.current);
    let changed = false;

    for (const id of displayedIds) {
      if (!visibleProcessIds.has(id)) {
        delete displayedExecutionProcesses.current[id];
        evictProcessCaches(id);
        changed = true;
      }
    }

    if (changed) {
      emitEntries(displayedExecutionProcesses.current, 'historic', false);
    }
  }, [idListKey, executionProcessesRaw, emitEntries, isLoading, isConnected]);

  // Initial load when attempt changes
  useEffect(() => {
    let cancelled = false;
    (async () => {
      // Waiting for execution processes to load
      if (
        executionProcesses?.current.length === 0 ||
        loadedInitialEntries.current
      )
        return;

      // Initial entries
      const allInitialEntries = await loadInitialEntries();
      if (cancelled) return;
      mergeIntoDisplayed((state) => {
        Object.assign(state, allInitialEntries);
      });
      emitEntries(displayedExecutionProcesses.current, 'initial', false);
      loadedInitialEntries.current = true;

      // Then load the remaining in batches
      while (
        !cancelled &&
        (await loadRemainingEntriesInBatches(REMAINING_BATCH_SIZE))
      ) {
        if (cancelled) return;
      }
      await new Promise((resolve) => setTimeout(resolve, 100));
      emitEntries(displayedExecutionProcesses.current, 'historic', false);
    })();
    return () => {
      cancelled = true;
    };
  }, [
    attempt.id,
    idListKey,
    loadInitialEntries,
    loadRemainingEntriesInBatches,
    emitEntries,
  ]); // include idListKey so new processes trigger reload

  useEffect(() => {
    const activeProcesses = getActiveAgentProcesses();
    if (activeProcesses.length === 0) return;

    for (const activeProcess of activeProcesses) {
      if (!displayedExecutionProcesses.current[activeProcess.id]) {
        const runningOrInitial =
          Object.keys(displayedExecutionProcesses.current).length > 1
            ? 'running'
            : 'initial';
        ensureProcessVisible(activeProcess);
        emitEntries(
          displayedExecutionProcesses.current,
          runningOrInitial,
          false
        );
      }

      if (
        activeProcess.status === ExecutionProcessStatus.running &&
        !streamingProcessIdsRef.current.has(activeProcess.id)
      ) {
        streamingProcessIdsRef.current.add(activeProcess.id);
        loadRunningAndEmitWithBackoff(activeProcess).finally(() => {
          streamingProcessIdsRef.current.delete(activeProcess.id);
        });
      }
    }
  }, [
    attempt.id,
    idStatusKey,
    emitEntries,
    ensureProcessVisible,
    loadRunningAndEmitWithBackoff,
  ]);

  useEffect(() => {
    if (!executionProcessesRaw) return;

    const processesToReload: ExecutionProcess[] = [];

    for (const process of executionProcessesRaw) {
      const previousStatus = previousStatusMapRef.current.get(process.id);
      const currentStatus = process.status;

      if (
        previousStatus === ExecutionProcessStatus.running &&
        currentStatus !== ExecutionProcessStatus.running &&
        displayedExecutionProcesses.current[process.id]
      ) {
        processesToReload.push(process);
      }

      previousStatusMapRef.current.set(process.id, currentStatus);
    }

    if (processesToReload.length === 0) return;

    (async () => {
      let anyUpdated = false;

      for (const process of processesToReload) {
        const entries = await loadEntriesForHistoricExecutionProcess(process);
        if (entries.length === 0) continue;

        const entriesWithKey = entries.map((e, idx) =>
          patchWithKey(e, process.id, idx)
        );

        mergeIntoDisplayed((state) => {
          state[process.id] = {
            executionProcess: process,
            entries: entriesWithKey,
          };
        });
        anyUpdated = true;
      }

      if (anyUpdated) {
        emitEntries(displayedExecutionProcesses.current, 'running', false);
      }
    })();
  }, [
    idStatusKey,
    executionProcessesRaw,
    emitEntries,
    loadEntriesForHistoricExecutionProcess,
  ]);

  // If an execution process is removed, remove it from the state
  useEffect(() => {
    if (!executionProcessesRaw) return;

    const removedProcessIds = Object.keys(
      displayedExecutionProcesses.current
    ).filter((id) => !executionProcessesRaw.some((p) => p.id === id));

    if (removedProcessIds.length > 0) {
      mergeIntoDisplayed((state) => {
        removedProcessIds.forEach((id) => {
          delete state[id];
          evictProcessCaches(id);
        });
      });
    }
  }, [attempt.id, idListKey, executionProcessesRaw]);

  return {
    hasSetupScriptRun,
    hasCleanupScriptRun,
    hasRunningProcess,
  };
};
