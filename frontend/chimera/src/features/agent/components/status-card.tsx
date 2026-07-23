import type { ReactNode } from 'react';
import { Card, CardContent, CardHeader } from '@/components/ui/card';

export type AgentStatusRow = {
  label: string;
  value: ReactNode;
};

export function AgentStatusCard({
  icon,
  title,
  rows,
}: {
  icon: ReactNode;
  title: string;
  rows: AgentStatusRow[];
}) {
  return (
    <Card variant="outline">
      <CardHeader className="text-base">
        <span className="text-primary bg-primary/10 flex size-9 items-center justify-center rounded-full">
          {icon}
        </span>
        <span>{title}</span>
      </CardHeader>
      <CardContent className="gap-2 text-sm">
        {rows.map(({ label, value }) => (
          <div
            className="flex min-w-0 items-center justify-between gap-4"
            key={label}
          >
            <span className="text-on-surface-variant">{label}</span>
            <span className="truncate text-right font-medium">{value}</span>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
