import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { ClaudeCode, ExecutorConfig } from 'shared/types';
import {
  FollowUpOptionsMenu,
  type FollowUpOption,
  type FollowUpOptionGroup,
} from './FollowUpOptionsMenu';

const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'];

function options(values: string[]): FollowUpOption[] {
  return values.map((value) => ({ value, label: value }));
}

type Props = {
  config: ClaudeCode;
  currentProfile: ExecutorConfig | null;
  onChange: (config: ClaudeCode) => void;
  disabled?: boolean;
};

export function ClaudeFollowUpOptions({
  config,
  currentProfile,
  onChange,
  disabled,
}: Props) {
  const { t } = useTranslation('tasks');
  const models = useMemo(() => {
    const values = new Set(['sonnet', 'opus', 'haiku']);
    Object.values(currentProfile ?? {}).forEach((profile) => {
      if (profile && 'CLAUDE_CODE' in profile && profile.CLAUDE_CODE.model) {
        values.add(profile.CLAUDE_CODE.model);
      }
    });
    if (config.model) values.add(config.model);
    return options([...values]);
  }, [currentProfile, config.model]);

  const permissionMode = config.plan
    ? 'plan'
    : config.approvals
      ? 'manual'
      : 'auto';
  const groups: FollowUpOptionGroup[] = [
    {
      id: 'permission',
      label: t('followUp.approvalMode'),
      value: permissionMode,
      options: [
        { value: 'auto', label: t('followUp.autoApprove') },
        { value: 'manual', label: t('followUp.manualApprove') },
        { value: 'plan', label: 'Plan' },
      ],
      onChange: (mode) =>
        onChange({
          ...config,
          plan: mode === 'plan',
          approvals: mode === 'manual',
          dangerously_skip_permissions: mode === 'auto',
        }),
    },
    {
      id: 'model',
      label: t('followUp.model'),
      value: config.model ?? '',
      options: [{ value: '', label: t('followUp.defaultOption') }, ...models],
      onChange: (model) => onChange({ ...config, model: model || null }),
    },
    {
      id: 'effort',
      label: t('followUp.reasoningEffort'),
      value: config.effort ?? '',
      options: [
        { value: '', label: t('followUp.defaultOption') },
        ...options(EFFORTS),
      ],
      onChange: (effort) =>
        onChange({
          ...config,
          effort: (effort || null) as ClaudeCode['effort'],
        }),
    },
  ];

  return (
    <FollowUpOptionsMenu
      triggerLabel={
        permissionMode === 'plan' ? 'Plan' : (config.model ?? permissionMode)
      }
      ariaLabel={t('followUp.claudeOptions')}
      groups={groups}
      disabled={disabled}
    />
  );
}
