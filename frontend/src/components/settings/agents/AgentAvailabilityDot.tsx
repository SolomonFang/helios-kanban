import { Loader2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { BaseCodingAgent } from 'shared/types';
import { useAgentAvailability } from '@/hooks/useAgentAvailability';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import { cn } from '@/lib/utils';

interface AgentAvailabilityDotProps {
  agent: BaseCodingAgent;
}

export function AgentAvailabilityDot({ agent }: AgentAvailabilityDotProps) {
  const { t } = useTranslation('settings');
  const availability = useAgentAvailability(agent);

  if (!availability) return null;

  if (availability.status === 'checking') {
    return (
      <Loader2 className="h-3 w-3 shrink-0 animate-spin text-muted-foreground" />
    );
  }

  const isAvailable =
    availability.status === 'login_detected' ||
    availability.status === 'installation_found';

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <span
            className={cn(
              'h-2 w-2 shrink-0 rounded-full',
              isAvailable ? 'bg-success' : 'bg-muted-foreground/40'
            )}
          />
        </TooltipTrigger>
        <TooltipContent side="top">
          {isAvailable
            ? t('settings.agents.availability.available')
            : t('settings.agents.availability.notFoundSimple')}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
