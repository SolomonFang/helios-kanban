import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { produce } from 'immer';
import type { Operation } from 'rfc6902';
import type { ApprovalInfo } from 'shared/types';
import { applyUpsertPatch } from '@/utils/jsonPatch';

export interface UseApprovalsResult {
  pendingApprovals: ApprovalInfo[];
  getPendingForProcess: (executionProcessId: string) => ApprovalInfo | null;
  getPendingById: (approvalId: string) => ApprovalInfo | null;
  isConnected: boolean;
  error: string | null;
}

type ApprovalState = {
  pending: Record<string, ApprovalInfo>;
};

type WsMsg =
  | { JsonPatch: Operation[] }
  | { Ready: true }
  | { finished?: boolean };

const ApprovalsContext = createContext<UseApprovalsResult | null>(null);

function approvalsWsUrl(): string {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}/api/approvals/stream/ws`;
}

export function ApprovalsProvider({ children }: { children: ReactNode }) {
  const [data, setData] = useState<ApprovalState>({ pending: {} });
  const [isConnected, setIsConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const dataRef = useRef(data);
  dataRef.current = data;

  useEffect(() => {
    let cancelled = false;
    let ws: WebSocket | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let retryAttempt = 0;

    const scheduleReconnect = () => {
      if (cancelled || retryTimer) return;
      const delay = Math.min(8000, 1000 * Math.pow(2, retryAttempt));
      retryTimer = setTimeout(() => {
        retryTimer = null;
        connect();
      }, delay);
    };

    const connect = () => {
      if (cancelled) return;

      ws = new WebSocket(approvalsWsUrl());

      ws.onopen = () => {
        if (cancelled) return;
        retryAttempt = 0;
        setError(null);
        setIsConnected(true);
      };

      ws.onmessage = (event) => {
        if (cancelled) return;
        try {
          const msg = JSON.parse(event.data) as WsMsg;
          if ('JsonPatch' in msg && msg.JsonPatch?.length) {
            const next = produce(dataRef.current, (draft) => {
              applyUpsertPatch(draft, msg.JsonPatch);
            });
            dataRef.current = next;
            setData(next);
          }
        } catch (err) {
          console.error('Failed to process approvals WS message:', err);
        }
      };

      ws.onerror = () => {
        if (cancelled) return;
        setError('Approvals stream connection failed');
      };

      ws.onclose = () => {
        if (cancelled) return;
        setIsConnected(false);
        ws = null;
        retryAttempt += 1;
        scheduleReconnect();
      };
    };

    connect();

    return () => {
      cancelled = true;
      if (retryTimer) clearTimeout(retryTimer);
      if (ws) {
        ws.onopen = null;
        ws.onmessage = null;
        ws.onerror = null;
        ws.onclose = null;
        ws.close();
      }
    };
  }, []);

  const pendingById = data.pending;
  const pendingApprovals = useMemo(
    () => Object.values(pendingById),
    [pendingById]
  );

  const getPendingForProcess = useCallback(
    (executionProcessId: string): ApprovalInfo | null => {
      for (const info of pendingApprovals) {
        if (info.execution_process_id === executionProcessId) {
          return info;
        }
      }
      return null;
    },
    [pendingApprovals]
  );

  const getPendingById = useCallback(
    (approvalId: string): ApprovalInfo | null => {
      return pendingById[approvalId] ?? null;
    },
    [pendingById]
  );

  const value = useMemo<UseApprovalsResult>(
    () => ({
      pendingApprovals,
      getPendingForProcess,
      getPendingById,
      isConnected,
      error,
    }),
    [pendingApprovals, getPendingForProcess, getPendingById, isConnected, error]
  );

  return (
    <ApprovalsContext.Provider value={value}>
      {children}
    </ApprovalsContext.Provider>
  );
}

export function useApprovals(): UseApprovalsResult {
  const context = useContext(ApprovalsContext);
  if (!context) {
    throw new Error('useApprovals must be used within ApprovalsProvider');
  }
  return context;
}
