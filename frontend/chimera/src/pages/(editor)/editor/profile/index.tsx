import { useProfile } from '@chimera/interface';
import { createFileRoute } from '@tanstack/react-router';
import TextMarquee from '@/components/ui/text-marquee';

export const Route = createFileRoute('/(editor)/editor/profile/')({
  component: RouteComponent,
  validateSearch: (search): { uid: string } => ({
    uid: typeof search.uid === 'string' ? search.uid : '',
  }),
});

function RouteComponent() {
  const { uid } = Route.useSearch();
  const { query } = useProfile();
  const profile = query.data?.items.find((item) => item.uid === uid);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="bg-primary-container dark:bg-on-primary flex h-12 shrink-0 items-center px-3">
        <TextMarquee className="min-w-0 flex-1 text-sm font-medium">
          {profile?.name ?? uid}
        </TextMarquee>
      </div>
      <div className="text-on-surface-variant grid min-h-0 flex-1 place-items-center text-sm">
        Loading editor…
      </div>
    </div>
  );
}
