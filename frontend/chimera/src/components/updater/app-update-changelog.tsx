import {
  commands,
  unwrapResult,
  type AppUpdateChangelogEntry,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { useQuery } from '@tanstack/react-query';
import Markdown from 'react-markdown';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';

function ChangelogEntry({ entry }: { entry: AppUpdateChangelogEntry }) {
  return (
    <section data-slot="app-update-changelog-entry">
      <header className="border-outline-variant mb-2 flex items-baseline gap-2 border-b pb-1.5">
        <h3 className="text-base font-semibold">v{entry.version}</h3>
        {entry.date && (
          <time
            className="text-on-surface-variant text-xs"
            dateTime={entry.date}
          >
            {entry.date}
          </time>
        )}
      </header>
      <Markdown
        components={{
          a({ children, href, ...props }) {
            return (
              <a
                {...props}
                href={href}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  if (typeof href === 'string') void commands.openThat(href);
                }}
              >
                {children}
              </a>
            );
          },
        }}
      >
        {entry.body}
      </Markdown>
    </section>
  );
}

export default function AppUpdateChangelog({
  from,
  to,
  fallbackBody,
}: {
  from: string;
  to: string;
  fallbackBody?: string | null;
}) {
  const query = useQuery({
    queryKey: ['getAppUpdateChangelog', from, to],
    queryFn: async () =>
      unwrapResult(await commands.getAppUpdateChangelog(from, to)),
    staleTime: Infinity,
    retry: false,
  });
  const entries = query.data?.entries ?? [];

  if (query.isPending) {
    return (
      <div
        className="text-on-surface-variant flex items-center gap-2 py-6 text-sm"
        data-slot="app-update-changelog-loading"
      >
        {m.settings_about_update_changelog_loading()}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-6" data-slot="app-update-changelog">
      {query.error && (
        <p className="text-error text-sm">
          {m.settings_about_update_changelog_failed({
            error: formatError(query.error),
          })}
        </p>
      )}
      {entries.map((entry) => (
        <ChangelogEntry key={entry.version} entry={entry} />
      ))}
      {entries.length === 0 && fallbackBody && (
        <Markdown
          components={{
            a({ children, href, ...props }) {
              return (
                <a
                  {...props}
                  href={href}
                  onClick={(event) => {
                    event.preventDefault();
                    event.stopPropagation();
                    if (typeof href === 'string') void commands.openThat(href);
                  }}
                >
                  {children}
                </a>
              );
            },
          }}
        >
          {fallbackBody}
        </Markdown>
      )}
      {query.data?.compare_url && (
        <Button
          className="-ml-2 flex items-center gap-1.5 self-start"
          variant="basic"
          onClick={() => void commands.openThat(query.data.compare_url!)}
        >
          {m.settings_about_update_changelog_compare()}
        </Button>
      )}
    </div>
  );
}
