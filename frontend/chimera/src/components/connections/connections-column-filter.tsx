import { BaseDialog, cn, type BaseDialogProps } from '@chimera/ui';
import DragIndicatorRounded from '~icons/material-symbols/drag-indicator';
import { useAtom } from 'jotai';
import { Reorder, useDragControls } from 'motion/react';
import { useMemo, type ChangeEvent } from 'react';
import * as m from '@/paraglide/messages';
import { connectionTableColumnsAtom } from '@/store';
import { CONNECTION_COLUMNS } from './connections-table';

export type ConnectionColumnFilterDialogProps = Omit<BaseDialogProps, 'title'>;

type ColumnState = [string, boolean];

function ColItem({
  value,
  label,
  onChange,
}: {
  value: ColumnState;
  label: string;
  onChange: (event: ChangeEvent<HTMLInputElement>) => void;
}) {
  const controls = useDragControls();

  return (
    <Reorder.Item
      value={value}
      dragListener={false}
      dragControls={controls}
      className="flex items-center gap-3 rounded-xl px-2 py-1.5"
    >
      <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-3">
        <input
          type="checkbox"
          checked={value[1]}
          onChange={onChange}
          className="accent-primary size-4"
        />
        <span className="truncate text-sm">{label}</span>
      </label>

      <button
        type="button"
        aria-label={`Reorder ${label}`}
        className={cn(
          'text-on-surface-variant hover:bg-primary/10 hover:text-primary',
          'grid size-9 cursor-grab place-items-center rounded-full active:cursor-grabbing',
        )}
        onPointerDown={(event) => controls.start(event)}
      >
        <DragIndicatorRounded className="size-5" />
      </button>
    </Reorder.Item>
  );
}

export default function ConnectionColumnFilterDialog(
  props: ConnectionColumnFilterDialogProps,
) {
  const [storedColumns, setStoredColumns] = useAtom(connectionTableColumnsAtom);

  const columns = useMemo(() => {
    const byId = new Map(CONNECTION_COLUMNS.map(([id, label]) => [id, label]));
    const stored = storedColumns
      .filter(([id]) => byId.has(id as (typeof CONNECTION_COLUMNS)[number][0]))
      .map(([id, visible]) => ({
        id,
        label: byId.get(id as (typeof CONNECTION_COLUMNS)[number][0]) ?? id,
        value: [id, visible] as ColumnState,
      }));

    const missing = CONNECTION_COLUMNS.filter(
      ([id]) => !storedColumns.some(([storedId]) => storedId === id),
    ).map(([id, label]) => ({
      id,
      label,
      value: [id, true] as ColumnState,
    }));

    return [...stored, ...missing];
  }, [storedColumns]);

  const values = columns.map(({ value }) => value);

  return (
    <BaseDialog title={m.connections_column_filter_title()} {...props}>
      <Reorder.Group
        axis="y"
        values={values}
        onReorder={(next) => setStoredColumns(next)}
        className="grid grid-cols-1 gap-1"
      >
        {columns.map(({ id, label, value }) => (
          <ColItem
            key={id}
            value={value}
            label={label}
            onChange={(event) => {
              const checked = event.target.checked;
              const hasColumn = storedColumns.some(
                ([columnId]) => columnId === id,
              );
              const nextColumns = hasColumn
                ? storedColumns.map(([columnId, visible]) =>
                    columnId === id
                      ? ([columnId, checked] as ColumnState)
                      : ([columnId, visible] as ColumnState),
                  )
                : [...storedColumns, [id, checked] as ColumnState];
              setStoredColumns(nextColumns);
            }}
          />
        ))}
      </Reorder.Group>
    </BaseDialog>
  );
}
