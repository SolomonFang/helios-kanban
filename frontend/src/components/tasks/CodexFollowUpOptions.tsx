import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { Codex, ExecutorConfig } from 'shared/types';
import {
  FollowUpOptionsMenu,
  type FollowUpOption,
  type FollowUpOptionGroup,
} from './FollowUpOptionsMenu';

const EFFORTS = ['low', 'medium', 'high', 'xhigh'];
const SANDBOXES = [
  'auto',
  'read-only',
  'workspace-write',
  'danger-full-access',
];
const APPROVALS = ['unless-trusted', 'on-failure', 'on-request', 'never'];

function options(values: string[]): FollowUpOption[] {
  return values.map((value) => ({ value, label: value }));
}

type Props = {
  config: Codex;
  currentProfile: ExecutorConfig | null;
  onChange: (config: Codex) => void;
  disabled?: boolean;
};

export function CodexFollowUpOptions({
  config,
  currentProfile,
  onChange,
  disabled,
}: Props) {
  const { t } = useTranslation('tasks');
  const models = useMemo(() => {
    const values = new Set<string>();
    Object.values(currentProfile ?? {}).forEach((profile) => {
      if (profile && 'CODEX' in profile && profile.CODEX.model) {
        values.add(profile.CODEX.model);
      }
    });
    if (config.model) values.add(config.model);
    return options([...values]);
  }, [currentProfile, config.model]);

  const withDefault = (items: FollowUpOption[]) => [
    { value: '', label: t('followUp.defaultOption') },
    ...items,
  ];
  const groups: FollowUpOptionGroup[] = [
    {
      id: 'model',
      label: t('followUp.model'),
      value: config.model ?? '',
      options: withDefault(models),
      onChange: (model) => onChange({ ...config, model: model || null }),
    },
    {
      id: 'effort',
      label: t('followUp.reasoningEffort'),
      value: config.model_reasoning_effort ?? '',
      options: withDefault(options(EFFORTS)),
      onChange: (effort) =>
        onChange({
          ...config,
          model_reasoning_effort: (effort ||
            null) as Codex['model_reasoning_effort'],
        }),
    },
    {
      id: 'sandbox',
      label: t('followUp.sandbox'),
      value: config.sandbox ?? '',
      options: withDefault(options(SANDBOXES)),
      onChange: (sandbox) =>
        onChange({ ...config, sandbox: (sandbox || null) as Codex['sandbox'] }),
    },
    {
      id: 'approval',
      label: t('followUp.approvalMode'),
      value: config.ask_for_approval ?? '',
      options: withDefault(options(APPROVALS)),
      onChange: (approval) =>
        onChange({
          ...config,
          ask_for_approval: (approval || null) as Codex['ask_for_approval'],
        }),
    },
  ];

  return (
    <FollowUpOptionsMenu
      triggerLabel={config.model ?? 'Codex'}
      ariaLabel={t('followUp.codexOptions')}
      groups={groups}
      disabled={disabled}
    />
  );
}
