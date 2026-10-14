import { useTranslation } from 'react-i18next';
import { MoreHorizontal, Plus, Star, Trash2 } from 'lucide-react';
import type {
  BaseCodingAgent,
  ExecutorConfigs,
  ExecutorProfileId,
} from 'shared/types';
import { Badge } from '@/components/ui/badge';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { AgentIcon, getAgentName } from '@/components/agents/AgentIcon';
import { AgentAvailabilityDot } from './AgentAvailabilityDot';
import { toPrettyCase } from '@/utils/string';
import { areProfilesEqual, getVariantOptions } from '@/utils/executor';
import { cn } from '@/lib/utils';

interface AgentConfigPickerProps {
  executors: ExecutorConfigs['executors'];
  selectedExecutor: BaseCodingAgent | null;
  selectedConfiguration: string | null;
  defaultProfile: ExecutorProfileId | null | undefined;
  disabled?: boolean;
  onSelectExecutor: (executor: BaseCodingAgent) => void;
  onSelectConfiguration: (configuration: string) => void;
  onCreateConfiguration: (executor: BaseCodingAgent) => void;
  onMakeDefault: (executor: BaseCodingAgent, configuration: string) => void;
  onDeleteConfiguration: (
    executor: BaseCodingAgent,
    configuration: string
  ) => void;
}

export function AgentConfigPicker({
  executors,
  selectedExecutor,
  selectedConfiguration,
  defaultProfile,
  disabled = false,
  onSelectExecutor,
  onSelectConfiguration,
  onCreateConfiguration,
  onMakeDefault,
  onDeleteConfiguration,
}: AgentConfigPickerProps) {
  const { t } = useTranslation('settings');

  const executorKeys = (Object.keys(executors) as BaseCodingAgent[]).sort(
    (a, b) => getAgentName(a).localeCompare(getAgentName(b))
  );

  const configurations = selectedExecutor
    ? getVariantOptions(selectedExecutor, executors)
    : [];

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
      {/* Agents column */}
      <div className="rounded-md border">
        <div className="border-b px-3 py-2">
          <span className="text-xs font-medium text-muted-foreground">
            {t('settings.agents.editor.agentLabel')}
          </span>
        </div>
        <div className="max-h-64 overflow-y-auto p-1">
          {executorKeys.map((executor) => {
            const isDefault = defaultProfile?.executor === executor;
            const isSelected = selectedExecutor === executor;
            return (
              <button
                key={executor}
                type="button"
                disabled={disabled}
                onClick={() => onSelectExecutor(executor)}
                className={cn(
                  'flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm hover:bg-accent',
                  isSelected && 'bg-accent'
                )}
              >
                <AgentIcon agent={executor} className="h-4 w-4 shrink-0" />
                <span className="flex-1 truncate">
                  {getAgentName(executor)}
                </span>
                <AgentAvailabilityDot agent={executor} />
                {isDefault && (
                  <Badge variant="secondary">
                    {t('settings.agents.editor.isDefault')}
                  </Badge>
                )}
              </button>
            );
          })}
        </div>
      </div>

      {/* Configurations column */}
      <div className="rounded-md border">
        <div className="flex items-center justify-between border-b px-3 py-2">
          <span className="text-xs font-medium text-muted-foreground">
            {t('settings.agents.editor.configLabel')}
          </span>
          {selectedExecutor && (
            <button
              type="button"
              className="rounded-sm p-1 text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-50"
              disabled={disabled}
              onClick={() => onCreateConfiguration(selectedExecutor)}
              title={t('settings.agents.editor.createNew')}
            >
              <Plus className="h-4 w-4" />
            </button>
          )}
        </div>
        <div className="max-h-64 overflow-y-auto p-1">
          {selectedExecutor ? (
            configurations.map((configName) => {
              const isDefault = areProfilesEqual(defaultProfile, {
                executor: selectedExecutor,
                variant: configName === 'DEFAULT' ? null : configName,
              });
              const isSelected = selectedConfiguration === configName;
              const isLastConfig = configurations.length <= 1;
              return (
                <div
                  key={configName}
                  className={cn(
                    'group flex items-center rounded-sm hover:bg-accent',
                    isSelected && 'bg-accent'
                  )}
                >
                  <button
                    type="button"
                    disabled={disabled}
                    onClick={() => onSelectConfiguration(configName)}
                    className="flex flex-1 items-center gap-2 px-2 py-1.5 text-left text-sm"
                  >
                    <span className="flex-1 truncate">
                      {toPrettyCase(configName)}
                    </span>
                    {isDefault && (
                      <Badge variant="secondary">
                        {t('settings.agents.editor.isDefault')}
                      </Badge>
                    )}
                  </button>
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        disabled={disabled}
                        className="mr-1 rounded-sm p-1 text-muted-foreground opacity-0 transition-opacity hover:text-foreground focus:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100 disabled:opacity-50"
                      >
                        <MoreHorizontal className="h-4 w-4" />
                      </button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuItem
                        disabled={isDefault}
                        onClick={() =>
                          onMakeDefault(selectedExecutor, configName)
                        }
                      >
                        <Star className="mr-2 h-4 w-4" />
                        {t('settings.agents.editor.makeDefault')}
                      </DropdownMenuItem>
                      <DropdownMenuItem
                        disabled={isLastConfig}
                        onClick={() =>
                          onDeleteConfiguration(selectedExecutor, configName)
                        }
                        className="text-destructive focus:text-destructive"
                      >
                        <Trash2 className="mr-2 h-4 w-4" />
                        {t('settings.agents.editor.deleteText')}
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              );
            })
          ) : (
            <p className="px-2 py-6 text-center text-sm text-muted-foreground">
              {t('settings.agents.selectAgent')}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
