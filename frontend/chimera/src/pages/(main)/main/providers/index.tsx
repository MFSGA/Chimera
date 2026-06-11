import { cn } from '@chimera/utils';
import { createFileRoute, Link } from '@tanstack/react-router';
import { PropsWithChildren } from 'react';
import { Button } from '@/components/ui/button';
import { m } from '@/paraglide/messages';

export const Route = createFileRoute('/(main)/main/providers/')({
  component: RouteComponent,
});

const Group = ({ children }: PropsWithChildren) => {
  return (
    <div className="flex flex-col gap-1" data-slot="providers-group">
      {children}
    </div>
  );
};

const GroupTitle = ({ children }: PropsWithChildren) => {
  return (
    <div
      className={cn(
        'sticky top-0 z-10 pl-1 text-lg font-semibold',
        'text-secondary bg-mixed-background flex h-16 items-center justify-between',
      )}
      data-slot="providers-group-title"
    >
      {children}
    </div>
  );
};
/* 
const GroupContent = ({ children }: PropsWithChildren) => {
  return (
    <div
      className="grid grid-cols-2 gap-2 sm:grid-cols-3 md:grid-cols-4"
      data-slot="providers-group-content"
    >
      {children}
    </div>
  );
}; */

function RouteComponent() {
  return (
    <div className="flex flex-col gap-4 p-4">
      <Group>
        <GroupTitle>
          <span>{m.providers_rules_title()}</span>

          <Button
            icon
            /* onClick={handleUpdateRules}
            loading={rulesBlockTask.isPending} */
          >
            <div>hello tood</div>
          </Button>
        </GroupTitle>
      </Group>
    </div>
  );
}
