import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import DisplayConversationEntry from '../NormalizedConversation/DisplayConversationEntry';
import {
  useEntriesActions,
  useSetTokenUsageInfo,
} from '@/contexts/EntriesContext';
import {
  AddEntryType,
  PatchTypeWithKey,
  useConversationHistory,
} from '@/hooks/useConversationHistory';
import { ArrowDown, Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { TaskWithAttemptStatus, type TokenUsageInfo } from 'shared/types';
import type { WorkspaceWithSession } from '@/types/attempt';
import { ApprovalFormProvider } from '@/contexts/ApprovalFormContext';

interface VirtualizedListProps {
  attempt: WorkspaceWithSession;
  task?: TaskWithAttemptStatus;
}

interface MessageListContext {
  attempt: WorkspaceWithSession;
  task?: TaskWithAttemptStatus;
}

const LARGE_BURST = 10;

/** Entries that DisplayConversationEntry does not render (null rows). */
function filterRenderableEntries(
  entries: PatchTypeWithKey[]
): PatchTypeWithKey[] {
  return entries.filter((entry) => {
    if (entry == null) return false;
    if (entry.type !== 'NORMALIZED_ENTRY') return true;
    return entry.content.entry_type.type !== 'token_usage_info';
  });
}

const VirtualizedList = ({ attempt, task }: VirtualizedListProps) => {
  const { t } = useTranslation('common');
  const [data, setData] = useState<PatchTypeWithKey[]>([]);
  const [loading, setLoading] = useState(true);
  const [atBottom, setAtBottom] = useState(true);
  const [hasNewOutput, setHasNewOutput] = useState(false);
  const { setEntries, reset } = useEntriesActions();
  const { setTokenUsageInfo } = useSetTokenUsageInfo();
  const atBottomRef = useRef(atBottom);
  atBottomRef.current = atBottom;
  const pendingUpdateRef = useRef<{
    entries: PatchTypeWithKey[];
    addType: AddEntryType;
    loading: boolean;
  } | null>(null);
  const rafIdRef = useRef<number | null>(null);
  const loadingRef = useRef(loading);
  loadingRef.current = loading;

  /** 消费 rAF 队列里的待处理更新并写入 Virtuoso 数据源。 */
  const applyPendingUpdate = useCallback(() => {
    rafIdRef.current = null;
    const pending = pendingUpdateRef.current;
    if (!pending) return;

    const renderableEntries = filterRenderableEntries(pending.entries);
    if (!atBottomRef.current) {
      setHasNewOutput(true);
    }
    setData(renderableEntries);
    setEntries(renderableEntries);
    setLoading(pending.loading);
  }, [setEntries]);

  useEffect(() => {
    if (rafIdRef.current !== null) {
      cancelAnimationFrame(rafIdRef.current);
      rafIdRef.current = null;
    }
    pendingUpdateRef.current = null;
    setLoading(true);
    setData([]);
    setAtBottom(true);
    atBottomRef.current = true;
    setHasNewOutput(false);
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
      if (tokenUsage) {
        setTokenUsageInfo(tokenUsage);
      }

      pendingUpdateRef.current = {
        entries: newEntries,
        addType,
        loading: newLoading,
      };

      if (rafIdRef.current === null) {
        rafIdRef.current = requestAnimationFrame(applyPendingUpdate);
      }
    },
    [applyPendingUpdate, setTokenUsageInfo]
  );

  useConversationHistory({ attempt, onEntriesUpdated });

  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const didInitScrollRef = useRef(false);
  const prevLenRef = useRef(0);

  const handleAtBottomChange = useCallback((bottom: boolean) => {
    atBottomRef.current = bottom;
    setAtBottom(bottom);
    if (bottom) setHasNewOutput(false);
  }, []);

  const scrollToLatest = useCallback(() => {
    if (data.length === 0) return;
    setHasNewOutput(false);
    virtuosoRef.current?.scrollToIndex({
      index: data.length - 1,
      align: 'end',
      behavior: 'smooth',
    });
  }, [data.length]);

  // 首次出现条目：跳到底部，初始化“跟随”状态。
  useEffect(() => {
    if (!didInitScrollRef.current && data.length > 0) {
      didInitScrollRef.current = true;
      requestAnimationFrame(() => {
        virtuosoRef.current?.scrollToIndex({
          index: data.length - 1,
          align: 'end',
        });
      });
    }
  }, [data.length]);

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
          index: data.length - 1,
          align: 'end',
        });
      });
    }
  }, [data.length, atBottom, data]);

  const messageListContext = useMemo(
    () => ({ attempt, task }),
    [attempt, task]
  );

  return (
    <ApprovalFormProvider>
      <div className="relative flex flex-1 min-h-0 flex-col">
        <Virtuoso
          ref={virtuosoRef}
          className="flex-1 min-h-0"
          data={data}
          context={messageListContext}
          computeItemKey={(_index, item) => `l-${item.patchKey}`}
          itemContent={(_index, item) => (
            <ItemRow item={item} context={messageListContext} />
          )}
          atBottomStateChange={handleAtBottomChange}
          followOutput={atBottom ? 'smooth' : false}
          components={{
            Header: () => <div className="h-2" />,
            Footer: () => <div className="h-2" />,
          }}
          // 加大下方视口，避免流式追加时频繁重新测量导致卡顿。
          increaseViewportBy={{ top: 0, bottom: 600 }}
        />
        {!loading && !atBottom && data.length > 0 && (
          <Button
            type="button"
            size="sm"
            variant="outline"
            className="absolute bottom-4 right-4 z-10 shadow-sm"
            onClick={scrollToLatest}
          >
            <ArrowDown className="h-4 w-4 mr-1.5" />
            {hasNewOutput
              ? t('conversation.newOutput')
              : t('conversation.backToLatest')}
          </Button>
        )}
        {loading && (
          <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-2 bg-primary">
            <Loader2 className="h-8 w-8 animate-spin" />
            <p>{t('states.loadingHistory')}</p>
          </div>
        )}
      </div>
    </ApprovalFormProvider>
  );
};

/** 渲染单条对话/日志条目；保持在组件外避免 useCallback 依赖震荡。 */
function ItemRow({
  item,
  context,
}: {
  item: PatchTypeWithKey;
  context: MessageListContext;
}) {
  const { attempt, task } = context;

  if (item.type === 'STDOUT') {
    return <p>{item.content}</p>;
  }
  if (item.type === 'STDERR') {
    return <p>{item.content}</p>;
  }
  if (item.type === 'NORMALIZED_ENTRY' && attempt) {
    return (
      <DisplayConversationEntry
        expansionKey={item.patchKey}
        entry={item.content}
        executionProcessId={item.executionProcessId}
        taskAttempt={attempt}
        task={task}
      />
    );
  }

  return null;
}

export default VirtualizedList;
