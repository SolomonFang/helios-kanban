import * as React from 'react';
import * as ToggleGroupPrimitive from '@radix-ui/react-toggle-group';
import { cva, type VariantProps } from 'class-variance-authority';
import { cn } from '@/lib/utils';

const ToggleGroup = ToggleGroupPrimitive.Root;

const toggleGroupItemVariants = cva(
  'inline-flex items-center justify-center rounded-sm text-sm font-medium ring-offset-background transition-colors focus-visible:outline-none disabled:pointer-events-none disabled:opacity-50',
  {
    variants: {
      active: {
        true: 'bg-primary text-primary-foreground',
        false:
          'text-primary-foreground/70 hover:bg-accent hover:text-accent-foreground',
      },
      size: {
        icon: 'h-4 w-4',
        sm: 'h-8 px-3 text-xs',
      },
    },
    defaultVariants: {
      active: false,
      size: 'icon',
    },
  }
);

interface ToggleGroupItemProps
  extends React.ComponentPropsWithoutRef<typeof ToggleGroupPrimitive.Item>,
    VariantProps<typeof toggleGroupItemVariants> {}

const ToggleGroupItem = React.forwardRef<
  React.ElementRef<typeof ToggleGroupPrimitive.Item>,
  ToggleGroupItemProps
>(({ className, active, size, ...props }, ref) => (
  <ToggleGroupPrimitive.Item
    ref={ref}
    className={cn(toggleGroupItemVariants({ active, size }), className)}
    {...props}
  />
));
ToggleGroupItem.displayName = 'ToggleGroupItem';

export { ToggleGroup, ToggleGroupItem };
