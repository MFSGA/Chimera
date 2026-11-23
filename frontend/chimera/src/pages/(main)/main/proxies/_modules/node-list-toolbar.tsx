import { Button } from '@chimera/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from '@chimera/ui/dropdown-menu';
import { EmptyState } from '@chimera/ui/empty-state';
import { SearchField } from '@chimera/ui/search-field';
import { Tooltip, TooltipContent, TooltipTrigger } from '@chimera/ui/tooltip';
import CloseRounded from '~icons/material-symbols/close-rounded';
import Radar from '~icons/material-symbols/radar';
import SearchRounded from '~icons/material-symbols/search-rounded';
import SortRounded from '~icons/material-symbols/sort-rounded';
import VisibilityOffRounded from '~icons/material-symbols/visibility-off-rounded';
import * as m from '@/paraglide/messages';
import { NODE_SORTS, type NodeSort, type NodeView } from './node-list';

const SORT_LABELS = {
  default: m.proxies_node_sort_default,
  name: m.proxies_node_sort_name,
  delay: m.proxies_node_sort_delay,
} satisfies Record<NodeSort, () => string>;

export function SearchNodesButton({
  search,
  onSearchChange,
  open,
  onOpenChange,
}: {
  search: string;
  onSearchChange: (search: string) => void;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  return open ? (
    <div
      className="flex h-8 max-w-52 min-w-0 items-center gap-1"
      data-slot="proxies-node-search-overlay"
    >
      <SearchField
        autoFocus
        className="h-8 min-w-0 flex-1"
        value={search}
        onValueChange={onSearchChange}
        placeholder={m.proxies_node_search_placeholder()}
        clearLabel={m.proxies_node_search_clear()}
        data-slot="proxies-node-search-field"
        onKeyDown={(event) => {
          if (event.key === 'Escape' && !search) onOpenChange(false);
        }}
      />
      <Button
        icon
        className="size-8 shrink-0"
        aria-label={m.common_close()}
        data-slot="proxies-node-search-close"
        onClick={() => onOpenChange(false)}
      >
        <CloseRounded className="size-4" />
      </Button>
    </div>
  ) : (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          icon
          className="size-8 shrink-0"
          aria-label={m.proxies_node_search_placeholder()}
          data-slot="proxies-node-search-button"
          onClick={() => onOpenChange(true)}
        >
          <SearchRounded className="size-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{m.proxies_node_search_placeholder()}</TooltipContent>
    </Tooltip>
  );
}

export function SortNodesButton({
  view,
  onViewChange,
}: {
  view: NodeView;
  onViewChange: (view: NodeView) => void;
}) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          icon
          className="size-8 shrink-0"
          aria-label={m.proxies_node_sort_label()}
          data-slot="proxies-node-sort-button"
        >
          <SortRounded className="size-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuRadioGroup
          value={view.sort}
          onValueChange={(sort) =>
            onViewChange({ ...view, sort: sort as NodeSort })
          }
        >
          {NODE_SORTS.map((sort) => (
            <DropdownMenuRadioItem key={sort} value={sort}>
              {SORT_LABELS[sort]()}
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

export function HideUnavailableButton({
  pressed,
  onPressedChange,
}: {
  pressed: boolean;
  onPressedChange: (pressed: boolean) => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          icon
          className="aria-pressed:bg-secondary-container aria-pressed:text-on-secondary-container size-8 shrink-0"
          aria-label={m.proxies_node_hide_unavailable()}
          aria-pressed={pressed}
          onClick={() => onPressedChange(!pressed)}
          data-slot="proxies-node-hide-unavailable"
        >
          <VisibilityOffRounded className="size-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{m.proxies_node_hide_unavailable()}</TooltipContent>
    </Tooltip>
  );
}

export function LocateCurrentNodeButton({
  disabled,
  onLocate,
}: {
  disabled: boolean;
  onLocate: () => void;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          icon
          className="size-8 shrink-0"
          aria-label={m.proxies_locate_current_node()}
          disabled={disabled}
          onClick={onLocate}
          data-slot="proxies-locate-current-node"
        >
          <Radar className="size-4" />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{m.proxies_locate_current_node()}</TooltipContent>
    </Tooltip>
  );
}

export function NoMatchingNodes() {
  return (
    <EmptyState
      data-slot="proxies-node-no-match"
      message={m.proxies_node_no_match()}
    />
  );
}
