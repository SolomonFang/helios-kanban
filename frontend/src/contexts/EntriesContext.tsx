import {
  createContext,
  useContext,
  useState,
  useMemo,
  useCallback,
  ReactNode,
} from 'react';
import type { PatchTypeWithKey } from '@/hooks/useConversationHistory';
import { TokenUsageInfo } from 'shared/types';

// Split into four contexts so that streaming updates only re-render the
// components that actually consume the changed slice:
// - entries state changes every frame while streaming; action contexts hold
//   stable references and never trigger re-renders on their own.
const EntriesStateContext = createContext<PatchTypeWithKey[] | null>(null);

interface EntriesActionsContextType {
  setEntries: (entries: PatchTypeWithKey[]) => void;
  reset: () => void;
}
const EntriesActionsContext = createContext<EntriesActionsContextType | null>(
  null
);

const TokenUsageStateContext = createContext<TokenUsageInfo | null | undefined>(
  undefined
);

interface TokenUsageActionsContextType {
  setTokenUsageInfo: (info: TokenUsageInfo | null) => void;
}
const TokenUsageActionsContext =
  createContext<TokenUsageActionsContextType | null>(null);

interface EntriesProviderProps {
  children: ReactNode;
}

export const EntriesProvider = ({ children }: EntriesProviderProps) => {
  const [entries, setEntriesState] = useState<PatchTypeWithKey[]>([]);
  const [tokenUsageInfo, setTokenUsageInfoState] =
    useState<TokenUsageInfo | null>(null);

  const setEntries = useCallback((newEntries: PatchTypeWithKey[]) => {
    setEntriesState(newEntries);
  }, []);

  const setTokenUsageInfo = useCallback((info: TokenUsageInfo | null) => {
    setTokenUsageInfoState((prev) => {
      // Skip broadcasting when the values are unchanged — token usage is
      // re-emitted on every stream flush and would otherwise re-render all
      // consumers each frame.
      if (
        prev === info ||
        (prev !== null &&
          info !== null &&
          prev.total_tokens === info.total_tokens &&
          prev.model_context_window === info.model_context_window)
      ) {
        return prev;
      }
      return info;
    });
  }, []);

  const reset = useCallback(() => {
    setEntriesState([]);
    setTokenUsageInfoState(null);
  }, []);

  const entriesActions = useMemo(
    () => ({ setEntries, reset }),
    [setEntries, reset]
  );
  const tokenUsageActions = useMemo(
    () => ({ setTokenUsageInfo }),
    [setTokenUsageInfo]
  );

  return (
    <EntriesActionsContext.Provider value={entriesActions}>
      <EntriesStateContext.Provider value={entries}>
        <TokenUsageActionsContext.Provider value={tokenUsageActions}>
          <TokenUsageStateContext.Provider value={tokenUsageInfo}>
            {children}
          </TokenUsageStateContext.Provider>
        </TokenUsageActionsContext.Provider>
      </EntriesStateContext.Provider>
    </EntriesActionsContext.Provider>
  );
};

export const useEntriesState = (): PatchTypeWithKey[] => {
  const context = useContext(EntriesStateContext);
  if (context === null) {
    throw new Error('useEntriesState must be used within an EntriesProvider');
  }
  return context;
};

export const useEntriesActions = (): EntriesActionsContextType => {
  const context = useContext(EntriesActionsContext);
  if (context === null) {
    throw new Error('useEntriesActions must be used within an EntriesProvider');
  }
  return context;
};

export const useSetTokenUsageInfo = (): TokenUsageActionsContextType => {
  const context = useContext(TokenUsageActionsContext);
  if (context === null) {
    throw new Error(
      'useSetTokenUsageInfo must be used within an EntriesProvider'
    );
  }
  return context;
};

export const useTokenUsage = () => {
  const context = useContext(TokenUsageStateContext);
  if (context === undefined) {
    throw new Error('useTokenUsage must be used within an EntriesProvider');
  }
  return context;
};
