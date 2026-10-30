import {
  getRemoteSource,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import { useNavigate } from '@tanstack/react-router';
import AccountTreeRounded from '~icons/material-symbols/account-tree-rounded';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import DragClickRounded from '~icons/material-symbols/drag-click-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import type { ComponentProps } from 'react';
import * as m from '@/paraglide/messages';
import { useActiveProfile } from '../../../_modules/use-active-profile';
import { useDeleteProfile } from '../../../_modules/use-delete-profile';
import { isProxyProfile } from '../../../_modules/utils';
import ProfileNameEditor from './profile-name-editor';
import SubscriptionUrlEditor from './subscription-url-editor';

const ActionButton = ({
  className,
  ...props
}: ComponentProps<typeof Button>) => (
  <Button
    variant="basic"
    className={cn(
      'bg-primary-container dark:bg-surface-variant/30 flex h-14 items-center gap-3 truncate rounded-2xl text-base font-semibold',
      className,
    )}
    {...props}
  />
);

function scrollViewportImmediately(viewport: HTMLElement, top: number) {
  const previous = viewport.style.getPropertyValue('scroll-behavior');
  const previousPriority =
    viewport.style.getPropertyPriority('scroll-behavior');
  viewport.style.setProperty('scroll-behavior', 'auto', 'important');
  try {
    viewport.scrollTop = top;
  } finally {
    if (previous) {
      viewport.style.setProperty('scroll-behavior', previous, previousPriority);
    } else {
      viewport.style.removeProperty('scroll-behavior');
    }
  }
}

function scrollToProfileTransformChainEditor(type: string, profileUid: string) {
  // Measure on the next frame so the target uses the post-click layout.
  requestAnimationFrame(() => {
    const routeSegments = window.location.pathname.split('/').filter(Boolean);
    if (
      routeSegments.at(-5) !== 'main' ||
      routeSegments.at(-4) !== 'profiles' ||
      routeSegments.at(-3) !== type ||
      routeSegments.at(-2) !== 'detail'
    ) {
      return;
    }
    const encodedRouteUid = routeSegments.at(-1);
    if (!encodedRouteUid) return;
    try {
      if (decodeURIComponent(encodedRouteUid) !== profileUid) return;
    } catch {
      return;
    }

    const editor = Array.from(
      document.querySelectorAll<HTMLElement>(
        '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
      ),
    ).find((candidate) => candidate.dataset.profileUid === profileUid);
    const viewport = editor?.closest<HTMLElement>(
      '[data-slot="scroll-area-viewport"]',
    );
    if (!editor || !viewport) return;

    const editorRect = editor.getBoundingClientRect();
    const viewportRect = viewport.getBoundingClientRect();
    const centerOffset = Math.max(
      0,
      (viewport.clientHeight - editorRect.height) / 2,
    );
    const maxScrollTop = Math.max(
      0,
      viewport.scrollHeight - viewport.clientHeight,
    );
    const targetScrollTop =
      viewport.scrollTop +
      editorRect.top -
      viewportRect.top -
      viewport.clientTop -
      centerOffset;
    const target = Math.min(maxScrollTop, Math.max(0, targetScrollTop));

    scrollViewportImmediately(viewport, target);
  });
}

export default function ActionCard({
  type,
  profile,
}: {
  type: string;
  profile: ProfileQueryResultItem;
}) {
  const navigate = useNavigate();
  const active = useActiveProfile(profile);
  const deletion = useDeleteProfile(profile, {
    onSuccess: async () => {
      await navigate({ to: '/main/profiles/$type', params: { type } });
    },
  });
  const isProxy = isProxyProfile(profile);
  const isPending = active.isPending || deletion.isPending;

  return (
    <div className="col-span-2 grid grid-cols-2 gap-4">
      <ProfileNameEditor profile={profile} asChild>
        <ActionButton>
          <EditSquareOutlineRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_name_editor_title()}</span>
        </ActionButton>
      </ProfileNameEditor>

      {getRemoteSource(profile) && (
        <SubscriptionUrlEditor profile={profile} asChild>
          <ActionButton>
            <EditSquareOutlineRounded className="size-4 shrink-0" />
            <span className="truncate">
              {m.profile_subscription_url_editor_label()}
            </span>
          </ActionButton>
        </SubscriptionUrlEditor>
      )}

      {isProxy && (
        <ActionButton
          disabled={isPending}
          loading={active.isPending}
          onClick={() => void active.handleClick()}
        >
          <DragClickRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_active_title()}</span>
        </ActionButton>
      )}

      {isProxy && (
        <ActionButton
          disabled={isPending}
          data-slot="profile-transform-chain"
          data-profile-uid={profile.uid}
          onClick={() => scrollToProfileTransformChainEditor(type, profile.uid)}
        >
          <AccountTreeRounded className="size-4 shrink-0" />
          <span className="truncate">{m.profile_menu_proxy_chains()}</span>
        </ActionButton>
      )}

      <ActionButton
        disabled={isPending}
        loading={deletion.isPending}
        onClick={() => void deletion.handleClick()}
      >
        <DeleteForeverOutlineRounded className="size-4 shrink-0" />
        <span className="truncate">{m.profile_delete_title()}</span>
      </ActionButton>
    </div>
  );
}
