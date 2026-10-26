import { ChevronDown, SlidersHorizontal } from 'lucide-react';
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

export type FollowUpOption = {
  value: string;
  label: string;
};

export type FollowUpOptionGroup = {
  id: string;
  label: string;
  value: string;
  options: FollowUpOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
};

type Props = {
  triggerLabel: string;
  ariaLabel: string;
  groups: FollowUpOptionGroup[];
  disabled?: boolean;
};

export function FollowUpOptionsMenu({
  triggerLabel,
  ariaLabel,
  groups,
  disabled,
}: Props) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          variant="outline"
          size="sm"
          className="max-w-48 px-2"
          disabled={disabled}
          aria-label={ariaLabel}
        >
          <SlidersHorizontal className="h-3 w-3 mr-1 shrink-0" />
          <span className="truncate text-xs">{triggerLabel}</span>
          <ChevronDown className="h-3 w-3 ml-1 shrink-0" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="start">
        {groups.map((group) => (
          <DropdownMenuSub key={group.id}>
            <DropdownMenuSubTrigger disabled={group.disabled}>
              {group.label}
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuRadioGroup
                value={group.value}
                onValueChange={group.onChange}
              >
                {group.options.map((option) => (
                  <DropdownMenuRadioItem
                    key={option.value}
                    value={option.value}
                  >
                    {option.label}
                  </DropdownMenuRadioItem>
                ))}
              </DropdownMenuRadioGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
