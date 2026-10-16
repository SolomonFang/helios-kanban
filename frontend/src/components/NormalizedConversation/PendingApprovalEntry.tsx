import { useCallback, useContext, useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import type { ApprovalOutcome, ToolStatus } from 'shared/types';
import { Button } from '@/components/ui/button';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { approvalsApi } from '@/lib/api';
import { Check, X } from 'lucide-react';
import WYSIWYGEditor from '@/components/ui/wysiwyg';

import { useHotkeysContext } from 'react-hotkeys-hook';
import { TabNavContext } from '@/contexts/TabNavigationContext';
import { useKeyApproveRequest, useKeyDenyApproval, Scope } from '@/keyboard';
import { useProject } from '@/contexts/ProjectContext';
import { useApprovalForm } from '@/contexts/ApprovalFormContext';
import {
  formatApprovalTime,
  useApprovalCountdown,
} from '@/hooks/useApprovalCountdown';

const DEFAULT_DENIAL_REASON = 'User denied this tool use request.';

// ---------- Types ----------
interface PendingApprovalEntryProps {
  pendingStatus: Extract<ToolStatus, { status: 'pending_approval' }>;
  executionProcessId?: string;
  children: ReactNode;
}

function ActionButtons({
  disabled,
  isResponding,
  onApprove,
  onStartDeny,
}: {
  disabled: boolean;
  isResponding: boolean;
  onApprove: () => void;
  onStartDeny: () => void;
}) {
  return (
    <div className="flex items-center gap-1.5 pr-4">
      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            onClick={onApprove}
            variant="ghost"
            className="h-8 w-8 rounded-full p-0"
            disabled={disabled}
            aria-label={isResponding ? 'Submitting approval' : 'Approve'}
            aria-busy={isResponding}
          >
            <Check className="h-5 w-5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          <p>{isResponding ? 'Submitting…' : 'Approve request'}</p>
        </TooltipContent>
      </Tooltip>

      <Tooltip>
        <TooltipTrigger asChild>
          <Button
            onClick={onStartDeny}
            variant="ghost"
            className="h-8 w-8 rounded-full p-0"
            disabled={disabled}
            aria-label={isResponding ? 'Submitting denial' : 'Deny'}
            aria-busy={isResponding}
          >
            <X className="h-5 w-5" />
          </Button>
        </TooltipTrigger>
        <TooltipContent>
          <p>{isResponding ? 'Submitting…' : 'Provide denial reason'}</p>
        </TooltipContent>
      </Tooltip>
    </div>
  );
}

function DenyReasonForm({
  isResponding,
  value,
  onChange,
  onCancel,
  onSubmit,
  projectId,
}: {
  isResponding: boolean;
  value: string;
  onChange: (v: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
  projectId?: string;
}) {
  return (
    <div className="flex flex-col gap-2 p-4">
      <WYSIWYGEditor
        value={value}
        onChange={onChange}
        placeholder="Let the agent know why this request was denied... Type @ to insert tags or search files."
        disabled={isResponding}
        className="min-h-[80px]"
        projectId={projectId}
        onCmdEnter={onSubmit}
      />
      <div className="flex flex-wrap items-center justify-end gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={onCancel}
          disabled={isResponding}
        >
          Cancel
        </Button>
        <Button size="sm" onClick={onSubmit} disabled={isResponding}>
          Deny
        </Button>
      </div>
    </div>
  );
}

// ---------- Main Component ----------
const PendingApprovalEntry = ({
  pendingStatus,
  executionProcessId,
  children,
}: PendingApprovalEntryProps) => {
  const { t } = useTranslation('common');
  const [isResponding, setIsResponding] = useState(false);
  const [hasResponded, setHasResponded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const {
    isEnteringReason,
    denyReason,
    setIsEnteringReason,
    setDenyReason,
    clear,
  } = useApprovalForm(pendingStatus.approval_id);

  const { projectId } = useProject();

  const { enableScope, disableScope, activeScopes } = useHotkeysContext();
  const tabNav = useContext(TabNavContext);
  const isLogsTabActive = tabNav ? tabNav.activeTab === 'logs' : true;
  const dialogScopeActive = activeScopes.includes(Scope.DIALOG);
  const shouldControlScopes = isLogsTabActive && !dialogScopeActive;
  const approvalsScopeEnabledRef = useRef(false);
  const dialogScopeActiveRef = useRef(dialogScopeActive);

  useEffect(() => {
    dialogScopeActiveRef.current = dialogScopeActive;
  }, [dialogScopeActive]);

  const { timeLeft } = useApprovalCountdown(
    pendingStatus.requested_at,
    pendingStatus.timeout_at,
    hasResponded
  );

  const disabled = isResponding || hasResponded || timeLeft <= 0;

  const shouldEnableApprovalsScope = shouldControlScopes && !disabled;

  useEffect(() => {
    const shouldEnable = shouldEnableApprovalsScope;

    if (shouldEnable && !approvalsScopeEnabledRef.current) {
      enableScope(Scope.APPROVALS);
      disableScope(Scope.KANBAN);
      approvalsScopeEnabledRef.current = true;
    } else if (!shouldEnable && approvalsScopeEnabledRef.current) {
      disableScope(Scope.APPROVALS);
      if (!dialogScopeActive) {
        enableScope(Scope.KANBAN);
      }
      approvalsScopeEnabledRef.current = false;
    }

    return () => {
      if (approvalsScopeEnabledRef.current) {
        disableScope(Scope.APPROVALS);
        if (!dialogScopeActiveRef.current) {
          enableScope(Scope.KANBAN);
        }
        approvalsScopeEnabledRef.current = false;
      }
    };
  }, [
    disableScope,
    enableScope,
    dialogScopeActive,
    shouldEnableApprovalsScope,
  ]);

  const respond = useCallback(
    async (approved: boolean, reason?: string) => {
      if (disabled) return;
      if (!executionProcessId) {
        setError('Missing executionProcessId');
        return;
      }

      setIsResponding(true);
      setError(null);

      const status: ApprovalOutcome = approved
        ? { status: 'approved' }
        : { status: 'denied', reason };

      try {
        await approvalsApi.respond(pendingStatus.approval_id, {
          execution_process_id: executionProcessId,
          status,
        });
        setHasResponded(true);
        clear();
      } catch (e: unknown) {
        console.error('Approval respond failed:', e);
        const errorMessage =
          e instanceof Error ? e.message : 'Failed to send response';
        setError(errorMessage);
      } finally {
        setIsResponding(false);
      }
    },
    [disabled, executionProcessId, pendingStatus.approval_id, clear]
  );

  const handleApprove = useCallback(() => respond(true), [respond]);
  const handleStartDeny = useCallback(() => {
    if (disabled) return;
    setError(null);
    setIsEnteringReason(true);
  }, [disabled, setIsEnteringReason]);

  const handleCancelDeny = useCallback(() => {
    if (isResponding) return;
    clear();
  }, [isResponding, clear]);

  const handleSubmitDeny = useCallback(() => {
    const trimmed = denyReason.trim();
    respond(false, trimmed || DEFAULT_DENIAL_REASON);
  }, [denyReason, respond]);

  const triggerDeny = useCallback(
    (event?: KeyboardEvent) => {
      if (!isEnteringReason || disabled || hasResponded) return;
      event?.preventDefault();
      handleSubmitDeny();
    },
    [isEnteringReason, disabled, hasResponded, handleSubmitDeny]
  );

  useKeyApproveRequest(handleApprove, {
    scope: Scope.APPROVALS,
    when: () => shouldEnableApprovalsScope && !isEnteringReason,
    preventDefault: true,
  });

  useKeyDenyApproval(triggerDeny, {
    scope: Scope.APPROVALS,
    when: () => shouldEnableApprovalsScope && !hasResponded,
    enableOnFormTags: ['textarea', 'TEXTAREA'],
    preventDefault: true,
  });

  return (
    <div className="relative mt-3">
      <div className="overflow-hidden">
        {children}

        <div className="bg-background px-0 py-1.5 text-xs sm:text-sm">
          <TooltipProvider>
            <div className="flex items-center justify-between gap-1.5 pl-4">
              <div className="flex items-center gap-1.5">
                {!isEnteringReason && (
                  <span className="text-muted-foreground">
                    Would you like to approve this?
                  </span>
                )}
                {!hasResponded && (
                  <span
                    className={
                      timeLeft <= 0
                        ? 'text-amber-600'
                        : 'text-muted-foreground tabular-nums'
                    }
                  >
                    {timeLeft <= 0
                      ? t('conversation.approvalStatus.timedOut')
                      : t('conversation.approvalStatus.timeRemaining', {
                          time: formatApprovalTime(timeLeft),
                        })}
                  </span>
                )}
              </div>
              {!isEnteringReason && (
                <ActionButtons
                  disabled={disabled}
                  isResponding={isResponding}
                  onApprove={handleApprove}
                  onStartDeny={handleStartDeny}
                />
              )}
            </div>

            {error && (
              <div
                className="mt-1 text-xs text-red-600"
                role="alert"
                aria-live="polite"
              >
                {error}
              </div>
            )}

            {isEnteringReason && !hasResponded && (
              <DenyReasonForm
                isResponding={isResponding}
                value={denyReason}
                onChange={setDenyReason}
                onCancel={handleCancelDeny}
                onSubmit={handleSubmitDeny}
                projectId={projectId}
              />
            )}
          </TooltipProvider>
        </div>
      </div>
    </div>
  );
};

export default PendingApprovalEntry;
