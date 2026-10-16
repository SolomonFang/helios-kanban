import { useTokenUsage } from '@/contexts/EntriesContext';
import { useTranslation } from 'react-i18next';
import { cn } from '@/lib/utils';

function formatTokens(value: number): string {
  if (value >= 1_000_000) {
    const millions = value / 1_000_000;
    return millions % 1 === 0 ? `${millions}M` : `${millions.toFixed(1)}M`;
  }
  if (value >= 1_000) return `${Math.round(value / 1_000)}K`;
  return value.toString();
}

/** Compact context-usage indicator for the legacy follow-up action bar. */
export function LegacyContextUsage() {
  const { t } = useTranslation('common');
  const tokenUsageInfo = useTokenUsage();

  if (!tokenUsageInfo || tokenUsageInfo.model_context_window <= 0) {
    return null;
  }

  const percentage = Math.min(
    100,
    (tokenUsageInfo.total_tokens / tokenUsageInfo.model_context_window) * 100
  );
  const roundedPercentage = Math.round(percentage);
  const used = formatTokens(tokenUsageInfo.total_tokens);
  const total = formatTokens(tokenUsageInfo.model_context_window);

  const radius = 7;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - percentage / 100);

  return (
    <div
      className="flex items-center gap-1 px-1 text-xs text-muted-foreground cursor-help"
      title={t('contextUsage.tooltip', {
        percentage: roundedPercentage,
        used,
        total,
      })}
      aria-label={t('contextUsage.ariaLabel', {
        percentage: roundedPercentage,
      })}
      role="img"
    >
      <svg viewBox="0 0 18 18" className="h-4 w-4 -rotate-90" aria-hidden>
        <circle
          cx="9"
          cy="9"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          className="text-border"
        />
        <circle
          cx="9"
          cy="9"
          r={radius}
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeDasharray={`${circumference} ${circumference}`}
          strokeDashoffset={dashOffset}
          className={cn(
            'transition-all duration-500 ease-out',
            percentage >= 90 ? 'text-destructive' : 'text-muted-foreground'
          )}
        />
      </svg>
      <span className="tabular-nums">{roundedPercentage}%</span>
    </div>
  );
}
