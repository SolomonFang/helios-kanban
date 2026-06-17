import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Virtuoso, VirtuosoHandle } from 'react-virtuoso';
import { WarningCircleIcon } from '@phosphor-icons/react/dist/ssr';
import RawLogText from '@/components/common/RawLogText';
import type { PatchType } from 'shared/types';

export type LogEntry = Extract<
  PatchType,
  { type: 'STDOUT' } | { type: 'STDERR' }
>;

export interface VirtualizedProcessLogsProps {
  logs: LogEntry[];
  error: string | null;
  searchQuery: string;
  matchIndices: number[];
  currentMatchIndex: number;
}

type LogEntryWithKey = LogEntry & { key: string; originalIndex: number };

interface SearchContext {
  searchQuery: string;
  matchIndices: number[];
  currentMatchIndex: number;
}

const LARGE_BURST = 10;

export function VirtualizedProcessLogs({
  logs,
  error,
  searchQuery,
  matchIndices,
  currentMatchIndex,
}: VirtualizedProcessLogsProps) {
  const { t } = useTranslation('tasks');
  const [data, setData] = useState<LogEntryWithKey[]>([]);
  const [atBottom, setAtBottom] = useState(true);
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const didInitScrollRef = useRef(false);
  const prevLenRef = useRef(0);
  const prevCurrentMatchRef = useRef<number | undefined>(undefined);

  // 使用 100ms 去抖，避免每次 WS 推送都重建带 key 的数组。
  useEffect(() => {
    const timeoutId = setTimeout(() => {
      const logsWithKeys: LogEntryWithKey[] = logs.map((entry, index) => ({
        ...entry,
        key: `log-${index}`,
        originalIndex: index,
      }));
      setData(logsWithKeys);
    }, 100);

    return () => clearTimeout(timeoutId);
  }, [logs]);

  // 首次出现条目：跳到底部。
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

  // 大量追加且用户在底部：强制贴底（LARGE_BURST 为经验阈值）。
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

  // 当前匹配项变化时，居中跳转到对应日志。
  useEffect(() => {
    if (
      matchIndices.length > 0 &&
      currentMatchIndex >= 0 &&
      currentMatchIndex !== prevCurrentMatchRef.current
    ) {
      const logIndex = matchIndices[currentMatchIndex];
      virtuosoRef.current?.scrollToIndex({
        index: logIndex,
        align: 'center',
        behavior: 'smooth',
      });
      prevCurrentMatchRef.current = currentMatchIndex;
    }
  }, [currentMatchIndex, matchIndices]);

  if (logs.length === 0 && !error) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-center text-muted-foreground text-sm">
          {t('processes.noLogsAvailable')}
        </p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-center text-destructive text-sm">
          <WarningCircleIcon className="size-icon-base inline mr-2" />
          {error}
        </p>
      </div>
    );
  }

  const context: SearchContext = {
    searchQuery,
    matchIndices,
    currentMatchIndex,
  };

  return (
    <div className="h-full">
      <Virtuoso
        ref={virtuosoRef}
        className="h-full"
        data={data}
        context={context}
        computeItemKey={(_index, item) => item.key}
        itemContent={(_index, item) => (
          <LogRow item={item} context={context} />
        )}
        atBottomStateChange={setAtBottom}
        followOutput={atBottom ? 'smooth' : false}
        // 加大下方视口，避免流式追加时频繁重新测量导致卡顿。
        increaseViewportBy={{ top: 0, bottom: 600 }}
      />
    </div>
  );
}

/** 渲染单条日志行，根据上下文判断是否高亮命中。 */
function LogRow({
  item,
  context,
}: {
  item: LogEntryWithKey;
  context: SearchContext;
}) {
  const isMatch = context.matchIndices.includes(item.originalIndex);
  const isCurrentMatch =
    context.matchIndices[context.currentMatchIndex] === item.originalIndex;

  return (
    <RawLogText
      content={item.content}
      channel={item.type === 'STDERR' ? 'stderr' : 'stdout'}
      className="text-sm px-4 py-1"
      linkifyUrls
      searchQuery={isMatch ? context.searchQuery : undefined}
      isCurrentMatch={isCurrentMatch}
    />
  );
}
