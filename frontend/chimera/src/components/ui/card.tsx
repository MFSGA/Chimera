/**
 * Card UI 组件
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/components/ui/card.tsx`
 * 提供 Card、CardContent、CardHeader、CardFooter 四个组件，
 * 支持 basic / raised / outline 三种变体，以及 divider 分割线选项。
 */

import { cn } from '@chimera/ui';
import { cva, type VariantProps } from 'class-variance-authority';
import {
  createContext,
  useContext,
  type HTMLAttributes,
  type PropsWithChildren,
} from 'react';

export const cardVariants = cva('rounded-3xl text-on-surface overflow-hidden', {
  variants: {
    variant: {
      basic: ['shadow-sm', 'bg-surface dark:bg-surface'],
      raised: ['shadow-sm', 'bg-primary-container dark:bg-on-primary'],
      outline: [
        'bg-surface dark:bg-surface',
        'border border-outline-variant dark:border-outline-variant',
      ],
    },
  },
  defaultVariants: {
    variant: 'basic',
  },
});

export type CardVariantsProps = VariantProps<typeof cardVariants>;

export const cardContentVariants = cva(['flex flex-col gap-4 p-4']);

export const cardHeaderVariants = cva(
  ['flex items-center gap-4 text-xl', 'px-4'],
  {
    variants: {
      variant: {
        basic: 'border-surface-variant dark:border-surface-variant',
        raised: 'border-inverse-primary dark:border-primary-container',
        outline: 'border-outline-variant dark:border-outline-variant',
      },
      divider: {
        true: 'border-b py-4 ',
        false: 'pt-4',
      },
    },
    defaultVariants: {
      divider: false,
      variant: 'basic',
    },
  },
);

export const cardFooterVariants = cva(
  ['flex flex-row-reverse items-center gap-4', 'px-2'],
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

type CardContextType = {
  variant: CardVariantsProps['variant'];
  divider: boolean | undefined;
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
  extends HTMLAttributes<HTMLDivElement>,
    CardVariantsProps,
    Partial<CardContextType> {
  asChild?: boolean;
}

export const Card = ({
  variant,
  divider,
  className,
  children,
  ...props
}: PropsWithChildren<CardProps>) => {
  return (
    <CardContext.Provider
      value={{
        variant,
        divider,
      }}
    >
      <div
        className={cn(cardVariants({ variant }), className)}
        data-slot="card-root"
        {...props}
      >
        {children}
      </div>
    </CardContext.Provider>
  );
};

export type CardContentProps = HTMLAttributes<HTMLDivElement> & {
  asChild?: boolean;
};

export const CardContent = ({
  className,
  children,
  ...props
}: PropsWithChildren<CardContentProps>) => {
  return (
    <div
      className={cn(cardContentVariants(), className)}
      data-slot="card-content"
      {...props}
    >
      {children}
    </div>
  );
};

export type CardHeaderProps = HTMLAttributes<HTMLDivElement> & {
  asChild?: boolean;
  divider?: boolean;
  variant?: CardVariantsProps['variant'];
};

export const CardHeader = ({
  divider,
  variant,
  className,
  children,
  ...props
}: PropsWithChildren<CardHeaderProps>) => {
  const context = useCardContext();

  return (
    <div
      className={cn(
        cardHeaderVariants({
          divider: context?.divider ?? divider,
          variant: context?.variant ?? variant,
        }),
        className,
      )}
      data-slot="card-header"
      {...props}
    >
      {children}
    </div>
  );
};

export interface CardFooterProps
  extends HTMLAttributes<HTMLDivElement>,
    CardVariantsProps,
    Partial<Pick<CardContextType, 'divider'>> {}

export const CardFooter = ({
  divider,
  variant,
  className,
  children,
  ...props
}: PropsWithChildren<CardFooterProps>) => {
  const context = useCardContext();

  return (
    <div
      className={cn(
        cardFooterVariants({
          divider: context?.divider ?? divider,
          variant: context?.variant ?? variant,
        }),
        className,
      )}
      data-slot="card-footer"
      {...props}
    >
      {children}
    </div>
  );
};
