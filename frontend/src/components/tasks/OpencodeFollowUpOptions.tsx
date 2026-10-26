import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ChevronDown, SlidersHorizontal } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { Opencode, OpencodeConfigOption } from 'shared/types';
import { opencodeApi } from '@/lib/api';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

type Props = {
  config: Opencode;
  onChange: (config: Opencode) => void;
  disabled?: boolean;
};

function withCurrentValue<T extends OpencodeConfigOption>(
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

export function OpencodeFollowUpOptions({ config, onChange, disabled }: Props) {
  const { t } = useTranslation('tasks');
  const { data } = useQuery({
    queryKey: ['opencode', 'config-options'],
    queryFn: opencodeApi.configOptions,
    staleTime: 5 * 60 * 1000,
    retry: false,
  });

  const agents = useMemo(
    () => withCurrentValue(data?.agents ?? [], config.agent),
    [data?.agents, config.agent]
  );
  const models = useMemo(
    () => withCurrentValue(data?.models ?? [], config.model),
    [data?.models, config.model]
  );
  const selectedModel = models.find((model) => model.value === config.model);
  const variants = useMemo(
    () => withCurrentValue(selectedModel?.variants ?? [], config.variant),
    [selectedModel?.variants, config.variant]
  );
  const agentLabel =
    agents.find((agent) => agent.value === config.agent)?.label ??
    config.agent ??
    'build';

  if (!data) return null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="max-w-48 px-2"
          disabled={disabled}
          aria-label={t('followUp.opencodeOptions')}
        >
          <SlidersHorizontal className="h-3 w-3 mr-1 shrink-0" />
          <span className="truncate text-xs">{agentLabel}</span>
          <ChevronDown className="h-3 w-3 ml-1 shrink-0" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>{t('followUp.agent')}</DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuRadioGroup
              value={config.agent ?? ''}
              onValueChange={(agent) =>
                onChange({ ...config, agent: agent || null })
              }
            >
              <DropdownMenuRadioItem value="">
                {t('followUp.defaultOption')}
              </DropdownMenuRadioItem>
              {agents.map((agent) => (
                <DropdownMenuRadioItem key={agent.value} value={agent.value}>
                  {agent.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSub>
          <DropdownMenuSubTrigger>{t('followUp.model')}</DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuRadioGroup
              value={config.model ?? ''}
              onValueChange={(model) => {
                const nextModel = models.find(
                  (option) => option.value === model
                );
                const variant =
                  config.variant &&
                  nextModel?.variants.some(
                    (option) => option.value === config.variant
                  )
                    ? config.variant
                    : null;
                onChange({ ...config, model: model || null, variant });
              }}
            >
              <DropdownMenuRadioItem value="">
                {t('followUp.defaultOption')}
              </DropdownMenuRadioItem>
              {models.map((model) => (
                <DropdownMenuRadioItem key={model.value} value={model.value}>
                  {model.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSub>
          <DropdownMenuSubTrigger disabled={!selectedModel}>
            {t('followUp.variant')}
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            <DropdownMenuRadioGroup
              value={config.variant ?? ''}
              onValueChange={(variant) =>
                onChange({ ...config, variant: variant || null })
              }
            >
              <DropdownMenuRadioItem value="">
                {t('followUp.defaultOption')}
              </DropdownMenuRadioItem>
              {variants.map((variant) => (
                <DropdownMenuRadioItem
                  key={variant.value}
                  value={variant.value}
                >
                  {variant.label}
                </DropdownMenuRadioItem>
              ))}
            </DropdownMenuRadioGroup>
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
