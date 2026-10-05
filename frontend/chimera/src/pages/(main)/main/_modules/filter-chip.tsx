import { cn } from '@chimera/ui';
import CloseRounded from '~icons/material-symbols/close-rounded';

export default function FilterChip({
  label,
  title,
  filter,
  onRemove,
  className,
}: {
  /** "Dimension: value" */
  label: string;
  title?: string;
  filter: string;
  onRemove: () => void;
  className?: string;
}) {
  return (
    <span
      className={cn(
        'border-outline-variant text-on-surface flex h-8 shrink-0 items-center gap-1 rounded-lg border pr-1 pl-3 text-sm',
        className,
      )}
      data-slot="traffic-filter-chip"
      data-filter={filter}
      title={title}
    >
      <span className="max-w-56 truncate">{label}</span>

      <button
        type="button"
        className="hover:bg-on-surface/8 grid size-6 shrink-0 cursor-pointer place-content-center rounded-md"
        aria-label={title ?? label}
        onClick={onRemove}
      >
        <CloseRounded className="size-4" />
      </button>
    </span>
  );
}
