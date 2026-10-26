import { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { DeepseekHarness, ExecutorConfig } from 'shared/types';
import {
  FollowUpOptionsMenu,
  type FollowUpOptionGroup,
} from './FollowUpOptionsMenu';

type Props = {
  config: DeepseekHarness;
  currentProfile: ExecutorConfig | null;
  onChange: (config: DeepseekHarness) => void;
  disabled?: boolean;
};

export function DeepseekFollowUpOptions({
  config,
  currentProfile,
  onChange,
  disabled,
}: Props) {
  const { t } = useTranslation('tasks');
  const models = useMemo(() => {
    const values = new Set<string>();
    Object.values(currentProfile ?? {}).forEach((profile) => {
      if (
        profile &&
        'DEEPSEEK_HARNESS' in profile &&
        profile.DEEPSEEK_HARNESS.model
      ) {
        values.add(profile.DEEPSEEK_HARNESS.model);
      }
    });
    if (config.model) values.add(config.model);
    return [...values].map((value) => ({ value, label: value }));
  }, [currentProfile, config.model]);

  const groups: FollowUpOptionGroup[] = [
    {
      id: 'model',
      label: t('followUp.model'),
      value: config.model ?? '',
      options: [{ value: '', label: t('followUp.defaultOption') }, ...models],
      onChange: (model) => onChange({ ...config, model: model || null }),
    },
    {
      id: 'approval',
      label: t('followUp.approvalMode'),
      value: config.dangerously_skip_permissions ? 'auto' : 'manual',
      options: [
        { value: 'auto', label: t('followUp.autoApprove') },
        { value: 'manual', label: t('followUp.manualApprove') },
      ],
      onChange: (mode) =>
        onChange({
          ...config,
          dangerously_skip_permissions: mode === 'auto',
        }),
    },
  ];

  return (
    <FollowUpOptionsMenu
      triggerLabel={config.model ?? 'DeepSeek'}
      ariaLabel={t('followUp.deepseekOptions')}
      groups={groups}
      disabled={disabled}
    />
  );
}
