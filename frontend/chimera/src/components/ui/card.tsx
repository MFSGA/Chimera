import { cn } from '@chimera/ui';
import { cva, type VariantProps } from 'class-variance-authority';
import { Slot } from 'radix-ui';
import { createContext, useContext, type HTMLAttributes } from 'react';

export const cardVariants = cva('overflow-hidden rounded-3xl text-on-surface', {
  variants: {
    variant: {
      basic: ['bg-surface shadow-sm dark:bg-surface'],
      raised: ['bg-primary-container shadow-sm dark:bg-on-primary'],
      outline: [
        'border border-outline-variant bg-surface dark:border-outline-variant dark:bg-surface',
      ],
    },
  },
  defaultVariants: {
    variant: 'basic',
  },
});

export type CardVariantsProps = VariantProps<typeof cardVariants>;

export const cardContentVariants = cva(['flex flex-col gap-4 p-4']);
export type CardContentVariantsProps = VariantProps<typeof cardContentVariants>;

export const cardHeaderVariants = cva(
  ['flex items-center gap-4 px-4 text-xl'],
  {
    variants: {
      variant: {
        basic: 'border-surface-variant dark:border-surface-variant',
        raised: 'border-inverse-primary dark:border-primary-container',
        outline: 'border-outline-variant dark:border-outline-variant',
      },
      divider: {
        true: 'border-b py-4',
        false: 'pt-4',
      },
    },
    defaultVariants: {
      divider: false,
      variant: 'basic',
    },
  },
);
export type CardHeaderVariantsProps = VariantProps<typeof cardHeaderVariants>;

export const cardFooterVariants = cva(
  ['flex flex-row-reverse items-center gap-4 px-2'],
  {
    variants: {
      variant: {
        basic: 'border-surface-variant dark:border-surface-variant',
        raised: 'border-inverse-primary dark:border-primary-container',
        outline: 'border-outline-variant dark:border-outline-variant',
      },
      divider: {
        true: 'border-t py-2',
        false: 'pb-2',
      },
    },
    defaultVariants: {
      divider: false,
      variant: 'basic',
    },
  },
);
export type CardFooterVariantsProps = VariantProps<typeof cardFooterVariants>;

type CardContextType = {
  variant: CardVariantsProps['variant'];
  divider: CardHeaderVariantsProps['divider'] &
    CardFooterVariantsProps['divider'];
};

const CardContext = createContext<CardContextType | null>(null);

const useCardContext = () => {
  const context = useContext(CardContext);

  if (!context) {
    throw new Error('useCardContext must be used within a CardProvider');
  }

  return context;
};

export interface CardProps
  extends
    HTMLAttributes<HTMLDivElement>,
    CardVariantsProps,
    Partial<CardContextType> {
  asChild?: boolean;
}

export const Card = ({
  variant,
  divider,
  asChild,
  className,
  ...props
}: CardProps) => {
  const Comp = asChild ? Slot.Root : 'div';

  return (
    <CardContext.Provider value={{ variant, divider }}>
      <Comp
        className={cn(cardVariants({ variant }), className)}
        data-slot="card-root"
        {...props}
      />
    </CardContext.Provider>
  );
};

export type CardContentProps = HTMLAttributes<HTMLDivElement> &
  CardContentVariantsProps & {
    asChild?: boolean;
  };

export const CardContent = ({
  className,
  asChild,
  ...props
}: CardContentProps) => {
  const Comp = asChild ? Slot.Root : 'div';

  return (
    <Comp
      className={cn(cardContentVariants(), className)}
      data-slot="card-content"
      {...props}
    />
  );
};

export type CardHeaderProps = HTMLAttributes<HTMLDivElement> &
  CardHeaderVariantsProps & {
    asChild?: boolean;
  };

export const CardHeader = ({
  divider,
  variant,
  className,
  asChild,
  ...props
}: CardHeaderProps) => {
  const context = useCardContext();
  const Comp = asChild ? Slot.Root : 'div';

  return (
    <Comp
      className={cn(
        cardHeaderVariants({
          divider: context.divider ?? divider,
          variant: context.variant ?? variant,
        }),
        className,
      )}
      data-slot="card-header"
      {...props}
    />
  );
};

export interface CardFooterProps
  extends HTMLAttributes<HTMLDivElement>, CardFooterVariantsProps {
  asChild?: boolean;
}

export const CardFooter = ({
  divider,
  variant,
  className,
  asChild,
  ...props
}: CardFooterProps) => {
  const context = useCardContext();
  const Comp = asChild ? Slot.Root : 'div';

  return (
    <Comp
      className={cn(
        cardFooterVariants({
          divider: context.divider ?? divider,
          variant: context.variant ?? variant,
        }),
        className,
      )}
      data-slot="card-footer"
      {...props}
    />
  );
};
