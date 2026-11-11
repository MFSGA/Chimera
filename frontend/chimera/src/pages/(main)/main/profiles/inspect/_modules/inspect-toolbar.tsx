import type { RuntimeInspection } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { SearchField } from '@chimera/ui/search-field';
import { Link } from '@tanstack/react-router';
import ArrowBackRounded from '~icons/material-symbols/arrow-back-rounded';
import RefreshRounded from '~icons/material-symbols/refresh-rounded';
import * as m from '@/paraglide/messages';
import { ProfileType } from '../../_modules/consts';

export default function InspectToolbar({
  snapshot,
  fieldQuery,
  onFieldQueryChange,
  onCycleMatch,
  onRefresh,
  loading,
}: {
  snapshot: RuntimeInspection | null | undefined;
  fieldQuery: string;
  onFieldQueryChange: (value: string) => void;
  onCycleMatch: (delta: 1 | -1) => void;
  onRefresh: () => void;
  loading: boolean;
}) {
  return (
    <header
      data-slot="inspect-toolbar"
      className="bg-mixed-background border-outline-variant flex shrink-0 flex-wrap items-center gap-3 border-b px-4 py-3"
    >
      <Link
        to="/main/profiles/$type"
        params={{ type: ProfileType.Profile }}
        aria-label={m.profile_nav_all_profiles()}
        className="text-on-surface-variant hover:bg-surface-variant grid size-10 shrink-0 place-items-center rounded-full"
      >
        <ArrowBackRounded aria-hidden className="size-5" />
      </Link>
      <h1 className="text-on-surface text-xl font-semibold">
        {m.profile_inspect_title()}
      </h1>
      {snapshot && (
        <p
          data-slot="inspect-status"
          className="text-on-surface-variant min-w-0 truncate text-xs"
        >
          {snapshot.target_core} · {m.inspect_revision()} {snapshot.revision}
        </p>
      )}
      <div className="min-w-0 basis-full sm:ml-auto sm:max-w-xs sm:flex-1 sm:basis-auto">
        <SearchField
          data-slot="inspect-field-search"
          value={fieldQuery}
          onValueChange={onFieldQueryChange}
          placeholder={m.inspect_filter_hint()}
          aria-label={m.inspect_find_field()}
          clearLabel={m.inspect_find_clear()}
          disabled={!snapshot}
          onKeyDown={(event) => {
            if (event.key !== 'Enter') return;
            event.preventDefault();
            onCycleMatch(event.shiftKey ? -1 : 1);
          }}
        />
      </div>
      <Button
        type="button"
        variant="basic"
        icon
        data-slot="inspect-refresh"
        onClick={onRefresh}
        disabled={loading}
        title={m.inspect_refresh()}
        aria-label={m.inspect_refresh()}
      >
        <RefreshRounded aria-hidden className="size-5" />
      </Button>
    </header>
  );
}
