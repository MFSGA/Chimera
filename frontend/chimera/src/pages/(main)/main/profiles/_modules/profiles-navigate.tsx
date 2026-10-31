import { useProfile } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import { Separator } from '@chimera/ui/separator';
import { useDroppable } from '@dnd-kit/react';
import { Link, useMatchRoute } from '@tanstack/react-router';
import AddRounded from '~icons/material-symbols/add-rounded';
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded';
import JavascriptRounded from '~icons/material-symbols/javascript-rounded';
import LanguageRounded from '~icons/material-symbols/language-rounded';
import LuaIcon from '~icons/mdi/language-lua';
import ChipLine from '~icons/mingcute/chip-line';
import YamlIcon from '~icons/nonicons/yaml-16';
import ScriptIcon from '~icons/streamline-plump/script-2-remix';
import type { ComponentProps, PropsWithChildren, ReactNode } from 'react';
import * as m from '@/paraglide/messages';
import { ProfileType } from './consts';
import FolderNavItem from './folder-nav-item';
import { NAV_ALL_ID, PROFILE_DRAG_TYPE } from './resolve-profile-drop';
import { useProfileLayout } from './use-profile-layout';
import { categoryProfiles } from './utils';

const LinkButton = ({
  href,
  exact = false,
  dataSlot,
  children,
}: PropsWithChildren<{ href: string; exact?: boolean; dataSlot?: string }>) => {
  const matchRoute = useMatchRoute();
  const isActive = !!matchRoute({ to: href, fuzzy: !exact });

  return (
    <Button
      variant="fab"
      data-active={String(isActive)}
      data-slot={dataSlot}
      asChild
    >
      <Link
        className={cn(
          'h-14',
          'flex items-center gap-2',
          'data-[active=true]:bg-surface-variant/80',
          'data-[active=false]:bg-transparent',
          'data-[active=false]:shadow-none',
          'data-[active=false]:hover:shadow-none',
          'data-[active=false]:hover:bg-surface-variant/30',
        )}
        to={href}
      >
        {children}
      </Link>
    </Button>
  );
};

const ROUTES = {
  [ProfileType.Profile]: {
    label: m.profile_profile_label(),
    href: '/main/profiles/profile',
    icon: () => (
      <div className="relative" data-profile-type-icon="profile">
        <DescriptionOutlineRounded className="size-8" />
        <ChipLine
          data-profile-type-badge="profile"
          className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-gray-300 p-0.5 dark:bg-gray-500"
        />
      </div>
    ),
  },
  [ProfileType.JavaScript]: {
    label: m.profile_javascript_label(),
    href: '/main/profiles/javascript',
    icon: () => (
      <div className="relative" data-profile-type-icon="javascript">
        <ScriptIcon className="size-8" />
        <JavascriptRounded
          data-profile-type-badge="javascript"
          className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-amber-400 dark:bg-amber-700"
        />
      </div>
    ),
  },
  [ProfileType.Lua]: {
    label: m.profile_lua_label(),
    href: '/main/profiles/lua',
    icon: () => (
      <div className="relative" data-profile-type-icon="lua">
        <ScriptIcon className="size-8" />
        <LuaIcon
          data-profile-type-badge="lua"
          className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-blue-300 p-0.5 dark:bg-blue-700"
        />
      </div>
    ),
  },
  [ProfileType.Merge]: {
    label: m.profile_merge_label(),
    href: '/main/profiles/merge',
    icon: () => (
      <div className="relative" data-profile-type-icon="merge">
        <ScriptIcon className="size-8" />
        <YamlIcon
          data-profile-type-badge="merge"
          className="absolute -right-0.5 bottom-0 size-4 rotate-12 rounded bg-orange-400 p-0.75 dark:bg-orange-700"
        />
      </div>
    ),
  },
} satisfies Record<
  ProfileType,
  { label: string; href: string; icon: () => ReactNode }
>;

export default function ProfilesNavigate({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { query } = useProfile();
  const categorized = categoryProfiles(query.data?.items ?? []);
  const { layout, openCreateFolder } = useProfileLayout();
  const folders = layout?.folders ?? [];
  const { ref: allProfilesRef, isDropTarget: isAllProfilesDropTarget } =
    useDroppable({ id: NAV_ALL_ID, accept: PROFILE_DRAG_TYPE });

  return (
    <div className={cn('flex flex-col gap-2', className)} {...props}>
      <div
        className="flex items-center justify-between gap-1 px-2"
        data-slot="profiles-folder-controls"
      >
        <span className="text-on-surface-variant text-xs font-medium">
          {m.profile_group_config()}
        </span>
        <Button
          icon
          className="size-8"
          aria-label={m.profile_folder_new_title()}
          data-slot="profiles-folder-add"
          onClick={() => openCreateFolder()}
        >
          <AddRounded className="size-5" aria-hidden />
        </Button>
      </div>
      <div
        ref={allProfilesRef}
        data-drop-target={isAllProfilesDropTarget || undefined}
      >
        <LinkButton
          href={ROUTES[ProfileType.Profile].href}
          dataSlot="profiles-all-nav-item"
        >
          <div className="size-8">{ROUTES[ProfileType.Profile].icon()}</div>
          <div className="min-w-0 text-sm font-medium">
            <p>{m.profile_nav_all_profiles()}</p>
            <p className="text-xs text-zinc-500">
              {m.profile_profile_label_count({
                count: categorized[ProfileType.Profile].length,
              })}
            </p>
          </div>
        </LinkButton>
      </div>
      {folders.map((folder, index) => (
        <FolderNavItem key={folder.id} folder={folder} index={index} />
      ))}
      <Separator />
      {Object.entries(ROUTES)
        .filter(([profileType]) => profileType !== ProfileType.Profile)
        .map(([profileType, route]) => (
          <LinkButton key={route.href} href={route.href}>
            <div className="size-8">{route.icon()}</div>
            <div className="min-w-0 text-sm font-medium">
              <p>{route.label}</p>
              <p className="text-xs text-zinc-500">
                {m.profile_profile_label_count({
                  count:
                    categorized[profileType as keyof typeof categorized].length,
                })}
              </p>
            </div>
          </LinkButton>
        ))}

      <Separator />
      <LinkButton
        href="/main/profiles/global"
        dataSlot="global-transform-chain-nav"
      >
        <div className="flex size-8 items-center justify-center">
          <LanguageRounded className="size-6" />
        </div>
        <div className="text-sm font-medium">
          <p>{m.profile_global_transform_title()}</p>
          <p className="text-xs text-zinc-500">
            {m.profile_profile_label_count({
              count: query.data?.global_transforms?.length ?? 0,
            })}
          </p>
        </div>
      </LinkButton>
      <LinkButton href="/main/profiles/inspect">Profile Inspect</LinkButton>
    </div>
  );
}
