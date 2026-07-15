import { cn } from '@chimera/ui';
import { cva, type VariantProps } from 'class-variance-authority';
import {
  useEffect,
  useState,
  type ChangeEvent,
  type ComponentProps,
} from 'react';

export const inputContainerVariants = cva(
  [
    'group relative box-border inline-flex h-14 w-full flex-auto cursor-pointer items-center justify-between px-4 py-4 outline-hidden',
    'dark:text-on-surface',
  ],
  {
    variants: {
      variant: {
        filled: 'rounded-t bg-surface-variant/30 dark:bg-surface',
        outlined: '',
      },
    },
    defaultVariants: { variant: 'filled' },
  },
);

export const inputVariants = cva(
  'peer w-full border-none bg-transparent p-0 placeholder-transparent outline-hidden transition-[margin] duration-200',
  {
    variants: {
      variant: { filled: '', outlined: '' },
      haveValue: { true: '', false: '' },
      haveLabel: { true: '', false: '' },
    },
    compoundVariants: [
      {
        variant: 'filled',
        haveValue: true,
        haveLabel: true,
        className: 'mt-3!',
      },
    ],
  },
);

export type InputProps = ComponentProps<'input'> &
  VariantProps<typeof inputContainerVariants> & { label?: string };

export const Input = ({
  variant = 'filled',
  className,
  label,
  onChange,
  value,
  defaultValue,
  ...props
}: InputProps) => {
  const [haveValue, setHaveValue] = useState(
    Boolean(value ?? defaultValue ?? ''),
  );

  useEffect(() => {
    setHaveValue(Boolean(value ?? defaultValue ?? ''));
  }, [value, defaultValue]);

  const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
    setHaveValue(event.target.value.length > 0);
    onChange?.(event);
  };
  const open = haveValue;

  return (
    <div
      className={inputContainerVariants({ variant })}
      data-state={open ? 'open' : 'closed'}
    >
      <input
        className={cn(
          inputVariants({ variant, haveValue, haveLabel: Boolean(label) }),
          className,
        )}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
        value={value}
        defaultValue={defaultValue}
        onChange={handleChange}
        {...props}
      />

      {label && (
        <>
          <fieldset
            className={cn(
              'pointer-events-none absolute inset-0 rounded text-left transition-all duration-200',
              variant === 'filled' && 'hidden',
              variant === 'outlined' && [
                'group-data-[state=closed]:border group-data-[state=open]:border-2',
                'group-data-[state=closed]:border-outline-variant group-data-[state=open]:border-primary',
                'peer-focus:border-primary dark:group-data-[state=open]:border-primary-container',
              ],
            )}
          >
            <legend
              className={cn(
                'invisible ml-2 h-0 px-2 text-sm',
                variant === 'filled' && 'hidden',
                variant === 'outlined' && !haveValue && 'hidden',
              )}
            >
              {label}
            </legend>
          </fieldset>

          <label
            className={cn(
              'pointer-events-none absolute top-4 left-4 text-base transition-all duration-200 select-none',
              open && 'text-primary text-xs',
              variant === 'filled' && open && 'top-2',
              variant === 'outlined' && open && '-top-2 text-sm',
            )}
          >
            {label}
          </label>
        </>
      )}

      {variant === 'filled' && (
        <div
          className={cn(
            'border-b-outline-variant absolute inset-x-0 bottom-0 w-full border-b transition-all',
            'after:border-primary after:absolute after:inset-x-0 after:bottom-0 after:scale-x-0 after:border-b-2 after:opacity-0 after:transition-all after:content-[""]',
            'peer-focus:after:scale-x-100 peer-focus:after:opacity-100',
          )}
        />
      )}
    </div>
  );
};

Input.displayName = 'Input';

export type NumericInputProps = Omit<
  ComponentProps<'input'>,
  'onChange' | 'value' | 'defaultValue' | 'type'
> & {
  value?: number | null;
  defaultValue?: number | null;
  onChange?: (value: number | null) => void;
  label?: string;
  min?: number;
  max?: number;
  decimalScale?: number;
  allowNegative?: boolean;
} & VariantProps<typeof inputContainerVariants>;

export const NumericInput = ({
  value,
  defaultValue,
  onChange,
  min,
  max,
  decimalScale,
  allowNegative = true,
  ...props
}: NumericInputProps) => {
  const [inputValue, setInputValue] = useState(() => {
    const initialValue = value ?? defaultValue;
    return initialValue == null ? '' : String(initialValue);
  });

  useEffect(() => {
    setInputValue(value == null ? '' : String(value));
  }, [value]);

  const normalize = (number: number) => {
    let next = allowNegative ? number : Math.max(0, number);
    if (min != null) next = Math.max(min, next);
    if (max != null) next = Math.min(max, next);
    return decimalScale == null ? next : Number(next.toFixed(decimalScale));
  };

  const commit = (rawValue: string) => {
    if (!rawValue || rawValue === '-') {
      setInputValue('');
      onChange?.(null);
      return;
    }
    const parsed = Number(rawValue);
    if (!Number.isNaN(parsed)) {
      const next = normalize(parsed);
      setInputValue(String(next));
      onChange?.(next);
    }
  };

  return (
    <Input
      {...props}
      type="text"
      inputMode="decimal"
      value={inputValue}
      onChange={(event) => {
        const rawValue = event.target.value;
        if (
          rawValue === '' ||
          (rawValue === '-' && allowNegative) ||
          rawValue.endsWith('.') ||
          !Number.isNaN(Number(rawValue))
        ) {
          setInputValue(rawValue);
          if (rawValue && rawValue !== '-' && !rawValue.endsWith('.')) {
            onChange?.(normalize(Number(rawValue)));
          } else if (!rawValue) {
            onChange?.(null);
          }
        }
      }}
      onBlur={() => commit(inputValue)}
    />
  );
};

NumericInput.displayName = 'NumericInput';
