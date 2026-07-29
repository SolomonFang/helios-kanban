import { useTranslation } from 'react-i18next';
import { Badge } from '@/components/ui/badge';
import { cn } from '@/lib/utils';
import type { TaskPriority } from 'shared/types';

const priorityBadgeStyles: Record<TaskPriority, string> = {
  urgent: 'border-red-500/50 bg-red-500/10 text-red-600 dark:text-red-400',
  high: 'border-orange-500/50 bg-orange-500/10 text-orange-600 dark:text-orange-400',
  medium: 'border-blue-500/50 bg-blue-500/10 text-blue-600 dark:text-blue-400',
  low: 'border-gray-500/50 bg-gray-500/10 text-gray-500 dark:text-gray-400',
};

export const priorityDotStyles: Record<TaskPriority, string> = {
  urgent: 'bg-red-500',
  high: 'bg-orange-500',
  medium: 'bg-blue-500',
  low: 'bg-gray-400',
};

interface PriorityBadgeProps {
  priority: TaskPriority;
  className?: string;
}

export function PriorityBadge({ priority, className }: PriorityBadgeProps) {
  const { t } = useTranslation('tasks');
  return (
    <Badge
      variant="outline"
      className={cn(priorityBadgeStyles[priority], className)}
    >
      {t(`taskFormDialog.priorityOptions.${priority}`)}
    </Badge>
  );
}
