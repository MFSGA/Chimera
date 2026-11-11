import type { SnapshotDiffHunk } from '@chimera/interface';
import { cn } from '@chimera/utils';
import { useVirtualizer } from '@tanstack/react-virtual';
import { useEffect, useMemo, useRef, type CSSProperties } from 'react';
import { columns, type FieldJump } from './text-find';

const LINE_HEIGHT = 20;
const OVERSCAN = 20;

export function YamlViewer({
  code,
  jump,
  label,
}: {
  code: string;
  jump?: FieldJump;
  label: string;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const lines = useMemo(() => code.split('\n'), [code]);
  const width = useMemo(
    () => lines.reduce((size, line) => Math.max(size, columns(line)), 0),
    [lines],
  );
  const virtualizer = useVirtualizer({
    count: lines.length,
    getScrollElement: () => viewport.current,
    estimateSize: () => LINE_HEIGHT,
    overscan: OVERSCAN,
    paddingStart: 12,
    paddingEnd: 12,
  });

  useEffect(() => {
    if (jump) virtualizer.scrollToIndex(jump.start, { align: 'center' });
  }, [jump?.nonce, virtualizer]);

  return (
    <div
      ref={viewport}
      role="region"
      aria-label={label}
      data-slot="inspect-yaml-viewer"
      tabIndex={0}
      className="bg-code text-on-surface focus-visible:outline-primary min-h-0 flex-1 overflow-auto font-mono text-[12.5px] leading-5 focus-visible:outline-2"
      style={
        {
          '--gutter': `calc(${String(lines.length).length}ch + 32px)`,
        } as CSSProperties
      }
    >
      <div
        className="relative"
        style={{
          height: virtualizer.getTotalSize(),
          minWidth: `calc(var(--gutter) + 16px + ${width}ch)`,
        }}
      >
        <pre className="m-0" data-slot="inspect-yaml-lines">
          <code>
            {virtualizer.getVirtualItems().map(({ index, start }) => (
              <span
                key={index}
                className={cn(
                  'absolute inset-x-0 block h-5 whitespace-pre',
                  jump &&
                    index >= jump.start &&
                    index <= jump.end &&
                    'bg-primary/12',
                  jump &&
                    index === jump.start &&
                    'outline-primary outline-1 -outline-offset-1',
                )}
                style={{ top: start }}
                data-line={index + 1}
                data-jump={jump && index === jump.start ? '' : undefined}
              >
                <span className="text-on-surface-variant inline-block w-(--gutter) pr-4 text-right select-none">
                  {index + 1}
                </span>
                {lines[index]}
              </span>
            ))}
          </code>
        </pre>
      </div>
    </div>
  );
}

type DiffRow =
  | { kind: 'header'; hunk: SnapshotDiffHunk }
  | {
      kind: 'line';
      sign: string;
      text: string;
      oldNumber?: number;
      newNumber?: number;
    };

function diffRows(hunks: readonly SnapshotDiffHunk[]): DiffRow[] {
  const rows: DiffRow[] = [];
  for (const hunk of hunks) {
    rows.push({ kind: 'header', hunk });
    let oldNumber = hunk.old_start;
    let newNumber = hunk.new_start;
    for (const line of hunk.lines) {
      const sign = line[0] ?? ' ';
      rows.push({
        kind: 'line',
        sign,
        text: line.slice(1),
        oldNumber: sign === '+' || sign === '\\' ? undefined : oldNumber++,
        newNumber: sign === '-' || sign === '\\' ? undefined : newNumber++,
      });
    }
  }
  return rows;
}

export function DiffViewer({
  hunks,
  label,
}: {
  hunks: SnapshotDiffHunk[];
  label: string;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => diffRows(hunks), [hunks]);
  const width = useMemo(
    () =>
      rows.reduce(
        (size, row) =>
          row.kind === 'line' ? Math.max(size, columns(row.text)) : size,
        0,
      ),
    [rows],
  );
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewport.current,
    estimateSize: () => LINE_HEIGHT,
    overscan: OVERSCAN,
  });

  return (
    <div
      ref={viewport}
      role="region"
      aria-label={label}
      data-slot="inspect-diff-viewer"
      tabIndex={0}
      className="bg-code text-on-surface focus-visible:outline-primary min-h-0 flex-1 overflow-auto font-mono text-[12.5px] leading-5 focus-visible:outline-2"
    >
      <div
        className="relative"
        style={{
          height: virtualizer.getTotalSize(),
          minWidth: `calc(9ch + 9ch + 22px + 16px + ${width}ch)`,
        }}
      >
        {virtualizer.getVirtualItems().map(({ index, start }) => {
          const row = rows[index];
          if (row.kind === 'header') {
            return (
              <div
                key={index}
                data-slot="inspect-diff-hunk"
                className="bg-on-surface/8 text-primary absolute inset-x-0 h-5 px-4 text-xs whitespace-pre"
                style={{ top: start }}
              >
                {`@@ -${row.hunk.old_start},${row.hunk.old_lines} +${row.hunk.new_start},${row.hunk.new_lines} @@`}
              </div>
            );
          }

          return (
            <div
              key={index}
              data-slot="inspect-diff-line"
              data-change={
                row.sign === '+'
                  ? 'add'
                  : row.sign === '-'
                    ? 'remove'
                    : 'context'
              }
              className={cn(
                'absolute inset-x-0 grid h-5 grid-cols-[9ch_9ch_22px_1fr] whitespace-pre',
                row.sign === '+' && 'bg-ok/12',
                row.sign === '-' && 'bg-error/12',
              )}
              style={{ top: start }}
            >
              <span className="text-on-surface-variant pr-2 text-right select-none">
                {row.oldNumber}
              </span>
              <span className="text-on-surface-variant pr-2 text-right select-none">
                {row.newNumber}
              </span>
              <span
                className={cn(
                  'text-center select-none',
                  row.sign === '+' && 'text-ok',
                  row.sign === '-' && 'text-error',
                )}
              >
                {row.sign}
              </span>
              <span className="pr-4">{row.text}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
