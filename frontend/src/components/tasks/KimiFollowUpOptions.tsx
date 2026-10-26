import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import type { AcpConfigOption, KimiCli } from 'shared/types';
import { kimiApi } from '@/lib/api';
import {
  FollowUpOptionsMenu,
  type FollowUpOptionGroup,
} from './FollowUpOptionsMenu';

function withCurrentValue<T extends AcpConfigOption>(
  options: T[],
  currentValue: string | null | undefined
): T[] {
  if (
    currentValue &&
    !options.some((option) => option.value === currentValue)
  ) {
    return [...options, { value: currentValue, label: currentValue }] as T[];
  }
  return options;
}

type Props = {
  config: KimiCli;
  onChange: (config: KimiCli) => void;
  disabled?: boolean;
};

export function KimiFollowUpOptions({ config, onChange, disabled }: Props) {
  const { t } = useTranslation('tasks');
  const { data } = useQuery({
    queryKey: ['kimi', 'config-options'],
    queryFn: kimiApi.configOptions,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });

  const modes = useMemo(
    () => withCurrentValue(data?.agents ?? [], config.mode),
    [data?.agents, config.mode]
  );
  const models = useMemo(
    () => withCurrentValue(data?.models ?? [], config.model),
    [data?.models, config.model]
  );
  const selectedModel = models.find(
    (model) => model.value === (config.model ?? data?.current_model)
  );
  const thinkingOptions = withCurrentValue(
    selectedModel?.variants ?? [],
    config.thinking
  );

  const groups: FollowUpOptionGroup[] = [
    {
      id: 'mode',
      label: t('followUp.agent'),
      value: config.mode ?? '',
      options: [{ value: '', label: t('followUp.defaultOption') }, ...modes],
      onChange: (mode) =>
        onChange({ ...config, mode: (mode || null) as KimiCli['mode'] }),
    },
    {
      id: 'model',
      label: t('followUp.model'),
      value: config.model ?? '',
      options: [{ value: '', label: t('followUp.defaultOption') }, ...models],
      onChange: (model) => {
        const nextModel = models.find((option) => option.value === model);
        const thinking =
          config.thinking &&
          nextModel?.variants.some((option) => option.value === config.thinking)
            ? config.thinking
            : null;
        onChange({ ...config, model: model || null, thinking });
      },
    },
    {
      id: 'thinking',
      label: t('followUp.thinking'),
      value: config.thinking ?? '',
      options: [
        { value: '', label: t('followUp.defaultOption') },
        ...thinkingOptions,
      ],
      onChange: (thinking) =>
        onChange({
          ...config,
          thinking: (thinking || null) as KimiCli['thinking'],
        }),
      disabled: !selectedModel,
    },
  ];

  if (!data) return null;

  return (
    <FollowUpOptionsMenu
      triggerLabel={config.mode ?? 'default'}
      ariaLabel={t('followUp.kimiOptions')}
      groups={groups}
      disabled={disabled}
    />
  );
}
