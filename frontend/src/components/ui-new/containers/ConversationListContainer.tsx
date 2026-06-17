import {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from 'react';
import { Virtuoso, VirtuosoHandle, ListRange } from 'react-virtuoso';

import NewDisplayConversationEntry from './NewDisplayConversationEntry';
import { ApprovalFormProvider } from '@/contexts/ApprovalFormContext';
import { useEntries } from '@/contexts/EntriesContext';
import {
  useResetProcess,
  type UseResetProcessResult,
} from '@/components/ui-new/hooks/useResetProcess';
import {
  AddEntryType,
  PatchTypeWithKey,
  DisplayEntry,
  isAggregatedGroup,
  isAggregatedDiffGroup,
  isAggregatedThinkingGroup,
  useConversationHistory,
} from '@/components/ui-new/hooks/useConversationHistory';
import { aggregateConsecutiveEntries } from '@/utils/aggregateEntries';
import { extractTokenUsageFromEntries } from '@/utils/streamJsonPatchEntries';
import type { TokenUsageInfo } from 'shared/types';
import type { WorkspaceWithSession } from '@/types/attempt';
import type { RepoWithTargetBranch } from 'shared/types';
import { useWorkspaceContext } from '@/contexts/WorkspaceContext';
import { ChatScriptPlaceholder } from '../primitives/conversation/ChatScriptPlaceholder';
import { ScriptFixerDialog } from '@/components/dialogs/scripts/ScriptFixerDialog';

interface ConversationListProps {
  attempt: WorkspaceWithSession;
}

export interface ConversationListHandle {
  scrollToPreviousUserMessage: () => void;
  scrollToBottom: () => void;
}

interface MessageListContext {
  attempt: WorkspaceWithSession;
  onConfigureSetup: (() => void) | undefined;
  onConfigureCleanup: (() => void) | undefined;
  showSetupPlaceholder: boolean;
  showCleanupPlaceholder: boolean;
  resetAction: UseResetProcessResult;
}

const LARGE_BURST = 10;

/** Entries that NewDisplayConversationEntry intentionally does not render. */
function filterRenderableEntries(
  entries: PatchTypeWithKey[]
): PatchTypeWithKey[] {
  return entries.filter((entry) => {
    if (entry.type !== 'NORMALIZED_ENTRY') return true;
    const entryType = entry.content.entry_type.type;
    return entryType !== 'next_action' && entryType !== 'token_usage_info';
  });
}

export const ConversationList = forwardRef<
  ConversationListHandle,
  ConversationListProps
>(function ConversationList({ attempt }, ref) {
  const resetAction = useResetProcess();
  const [data, setData] = useState<DisplayEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [atBottom, setAtBottom] = useState(true);
  const { setEntries, reset, setTokenUsageInfo } = useEntries();
  const pendingUpdateRef = useRef<{
    entries: PatchTypeWithKey[];
    addType: AddEntryType;
    loading: boolean;
    tokenUsage?: TokenUsageInfo | null;
  } | null>(null);
  // rAF throttle: 100ms debounce never fires during continuous streaming because
  // each WS patch resets the timer (~16ms with upstream-style rAF batching).
  const rafIdRef = useRef<number | null>(null);
  const loadingRef = useRef(loading);
  loadingRef.current = loading;
  // 记录最近一次推送类型，供 effect 决定滚动手势（plan 跳顶，其他交由 followOutput）。
  const lastAddTypeRef = useRef<AddEntryType | null>(null);
  // 跟踪当前可见首项索引，供 scrollToPreviousUserMessage 使用。
  const firstVisibleIndexRef = useRef<number>(0);
  // 标记首次出现条目，触发跳到底部。
  const didInitScrollRef = useRef(false);
  const prevLenRef = useRef(0);

  /** 消费 rAF 队列里的待处理更新并写入 Virtuoso 数据源。 */
  const applyPendingUpdate = useCallback(() => {
    rafIdRef.current = null;
    const pending = pendingUpdateRef.current;
    if (!pending) return;

    lastAddTypeRef.current = pending.addType;

    const renderableEntries = filterRenderableEntries(pending.entries);
    const aggregatedEntries = aggregateConsecutiveEntries(renderableEntries);

    setData(aggregatedEntries);
    setEntries(renderableEntries);

    const tokenUsage =
      pending.tokenUsage ?? extractTokenUsageFromEntries(pending.entries);
    if (tokenUsage) {
      setTokenUsageInfo(tokenUsage);
    }

    setLoading(pending.loading);
  }, [setEntries, setTokenUsageInfo]);

  // Get repos from workspace context to check if scripts are configured
  let repos: RepoWithTargetBranch[] = [];
  try {
    const workspaceContext = useWorkspaceContext();
    repos = workspaceContext.repos;
  } catch {
    // Context not available
  }

  // Use ref to access current repos without causing callback recreation
  const reposRef = useRef(repos);
  reposRef.current = repos;

  // Check if any repo has setup or cleanup scripts configured
  const hasSetupScript = repos.some((repo) => repo.setup_script);
  const hasCleanupScript = repos.some((repo) => repo.cleanup_script);

  // Handlers to open script fixer dialog for setup/cleanup scripts
  const handleConfigureSetup = useCallback(() => {
    const currentRepos = reposRef.current;
    if (currentRepos.length === 0) return;

    ScriptFixerDialog.show({
      scriptType: 'setup',
      repos: currentRepos,
      workspaceId: attempt.id,
      sessionId: attempt.session?.id,
    });
  }, [attempt.id, attempt.session?.id]);

  const handleConfigureCleanup = useCallback(() => {
    const currentRepos = reposRef.current;
    if (currentRepos.length === 0) return;

    ScriptFixerDialog.show({
      scriptType: 'cleanup',
      repos: currentRepos,
      workspaceId: attempt.id,
      sessionId: attempt.session?.id,
    });
  }, [attempt.id, attempt.session?.id]);

  // Determine if configure buttons should be shown
  const canConfigure = repos.length > 0;

  useEffect(() => {
    if (rafIdRef.current !== null) {
      cancelAnimationFrame(rafIdRef.current);
      rafIdRef.current = null;
    }
    pendingUpdateRef.current = null;
    lastAddTypeRef.current = null;
    didInitScrollRef.current = false;
    prevLenRef.current = 0;
    firstVisibleIndexRef.current = 0;
    setLoading(true);
    setData([]);
    setAtBottom(true);
    reset();
  }, [attempt.id, reset]);

  useEffect(() => {
    return () => {
      if (rafIdRef.current !== null) {
        cancelAnimationFrame(rafIdRef.current);
      }
    };
  }, []);

  const onEntriesUpdated = useCallback(
    (
      newEntries: PatchTypeWithKey[],
      addType: AddEntryType,
      newLoading: boolean,
      tokenUsage?: TokenUsageInfo | null
    ) => {
      pendingUpdateRef.current = {
        entries: newEntries,
        addType,
        loading: newLoading,
        tokenUsage,
      };

      if (rafIdRef.current === null) {
        rafIdRef.current = requestAnimationFrame(applyPendingUpdate);
      }
    },
    [applyPendingUpdate]
  );

  const { hasSetupScriptRun, hasCleanupScriptRun, hasRunningProcess } =
    useConversationHistory({ attempt, onEntriesUpdated });

  // Determine if there are entries to show placeholders
  const hasEntries = data.length > 0;

  // Show placeholders only if script not configured AND not already run
  const showSetupPlaceholder =
    !hasSetupScript && !hasSetupScriptRun && hasEntries;
  const showCleanupPlaceholder =
    !hasCleanupScript &&
    !hasCleanupScriptRun &&
    !hasRunningProcess &&
    hasEntries;

  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const messageListContext = useMemo(
    () => ({
      attempt,
      onConfigureSetup: canConfigure ? handleConfigureSetup : undefined,
      onConfigureCleanup: canConfigure ? handleConfigureCleanup : undefined,
      showSetupPlaceholder,
      showCleanupPlaceholder,
      resetAction,
    }),
    [
      attempt,
      canConfigure,
      handleConfigureSetup,
      handleConfigureCleanup,
      showSetupPlaceholder,
      showCleanupPlaceholder,
      resetAction,
    ]
  );

  // 首次出现条目：跳到底部以查看最新内容。
  useEffect(() => {
    if (!didInitScrollRef.current && data.length > 0) {
      didInitScrollRef.current = true;
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: 'LAST',
          align: 'end',
        });
      });
    }
  }, [data.length]);

  // 计划类条目到达时，跳到末项顶部，使用户能从计划开始处阅读。
  useEffect(() => {
    if (
      didInitScrollRef.current &&
      lastAddTypeRef.current === 'plan' &&
      data.length > 0
    ) {
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: 'LAST',
          align: 'start',
        });
      });
    }
  }, [data.length, data]);

  // 大量追加且用户在底部：强制贴底，避免大流量时跟不上（LARGE_BURST 为经验阈值）。
  useEffect(() => {
    const prev = prevLenRef.current;
    const grewBy = data.length - prev;
    prevLenRef.current = data.length;
    if (
      grewBy >= LARGE_BURST &&
      atBottom &&
      data.length > 0 &&
      didInitScrollRef.current
    ) {
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: 'LAST',
          align: 'end',
        });
      });
    }
  }, [data.length, atBottom, data]);

  /** 记录 Virtuoso 渲染范围的首项索引，供 scrollToPreviousUserMessage 使用。 */
  const handleRangeChanged = useCallback((range: ListRange) => {
    firstVisibleIndexRef.current = range.startIndex;
  }, []);

  // Expose scroll to previous user message functionality via ref
  useImperativeHandle(
    ref,
    () => ({
      scrollToPreviousUserMessage: () => {
        if (data.length === 0 || !virtuosoRef.current) return;

        const firstVisibleIndex = firstVisibleIndexRef.current;

        // Find all user message indices
        const userMessageIndices: number[] = [];
        data.forEach((item, index) => {
          if (
            item.type === 'NORMALIZED_ENTRY' &&
            item.content.entry_type.type === 'user_message'
          ) {
            userMessageIndices.push(index);
          }
        });

        // Find the user message before the first visible item
        const targetIndex = [...userMessageIndices]
          .reverse()
          .find((idx) => idx < firstVisibleIndex);

        if (targetIndex !== undefined) {
          virtuosoRef.current.scrollToIndex({
            index: targetIndex,
            align: 'start',
            behavior: 'smooth',
          });
        }
      },
      scrollToBottom: () => {
        virtuosoRef.current?.scrollToIndex({
          index: 'LAST',
          align: 'end',
          behavior: 'smooth',
        });
      },
    }),
    [data]
  );

  const showEmptyState = !loading && data.length === 0;

  return (
    <ApprovalFormProvider>
      <div className="h-full">
        {showEmptyState ? (
          <div className="h-full flex items-center justify-center px-double">
            <p className="text-sm text-low text-center">
              No messages yet. Send a prompt to start the conversation.
            </p>
          </div>
        ) : (
          <Virtuoso
            ref={virtuosoRef}
            className="h-full scrollbar-none"
            data={data}
            context={messageListContext}
            computeItemKey={(_index, item) => `conv-${item.patchKey}`}
            itemContent={(_index, item) => (
              <ItemRow item={item} context={messageListContext} />
            )}
            atBottomStateChange={setAtBottom}
            followOutput={atBottom ? 'smooth' : false}
            rangeChanged={handleRangeChanged}
            // 加大下方视口，避免流式追加时频繁重新测量导致卡顿。
            increaseViewportBy={{ top: 0, bottom: 600 }}
            components={{
              Header: ({ context }) => (
                <div className="pt-2">
                  {context?.showSetupPlaceholder && (
                    <div className="my-base px-double">
                      <ChatScriptPlaceholder
                        type="setup"
                        onConfigure={context.onConfigureSetup}
                      />
                    </div>
                  )}
                </div>
              ),
              Footer: ({ context }) => (
                <div className="pb-2">
                  {context?.showCleanupPlaceholder && (
                    <div className="my-base px-double">
                      <ChatScriptPlaceholder
                        type="cleanup"
                        onConfigure={context.onConfigureCleanup}
                      />
                    </div>
                  )}
                </div>
              ),
            }}
          />
        )}
      </div>
    </ApprovalFormProvider>
  );
});

/** 渲染单条对话项，分发到聚合/普通/原始日志三种渲染分支。 */
function ItemRow({
  item,
  context,
}: {
  item: DisplayEntry;
  context: MessageListContext;
}) {
  const attempt = context.attempt;
  const resetAction = context.resetAction;

  if (isAggregatedGroup(item)) {
    return (
      <NewDisplayConversationEntry
        expansionKey={item.patchKey}
        aggregatedGroup={item}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={null}
        entry={null}
        executionProcessId={item.executionProcessId}
        taskAttempt={attempt}
        resetAction={resetAction}
      />
    );
  }

  if (isAggregatedDiffGroup(item)) {
    return (
      <NewDisplayConversationEntry
        expansionKey={item.patchKey}
        aggregatedGroup={null}
        aggregatedDiffGroup={item}
        aggregatedThinkingGroup={null}
        entry={null}
        executionProcessId={item.executionProcessId}
        taskAttempt={attempt}
        resetAction={resetAction}
      />
    );
  }

  if (isAggregatedThinkingGroup(item)) {
    return (
      <NewDisplayConversationEntry
        expansionKey={item.patchKey}
        aggregatedGroup={null}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={item}
        entry={null}
        executionProcessId={item.executionProcessId}
        taskAttempt={attempt}
        resetAction={resetAction}
      />
    );
  }

  if (item.type === 'STDOUT') {
    return <p>{item.content}</p>;
  }
  if (item.type === 'STDERR') {
    return <p>{item.content}</p>;
  }
  if (item.type === 'NORMALIZED_ENTRY' && attempt) {
    return (
      <NewDisplayConversationEntry
        expansionKey={item.patchKey}
        entry={item.content}
        aggregatedGroup={null}
        aggregatedDiffGroup={null}
        aggregatedThinkingGroup={null}
        executionProcessId={item.executionProcessId}
        taskAttempt={attempt}
        resetAction={resetAction}
      />
    );
  }

  return null;
}

export default ConversationList;
