import { useProfile } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Link, useMatchRoute } from '@tanstack/react-router';
import CallMergeRounded from '~icons/material-symbols/call-merge-rounded';
import CodeRounded from '~icons/material-symbols/code-rounded';
import DescriptionOutlineRounded from '~icons/material-symbols/description-outline-rounded';
import JavascriptRounded from '~icons/material-symbols/javascript-rounded';
import MemoryRounded from '~icons/material-symbols/memory-rounded';
import type { ComponentProps, PropsWithChildren, ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import * as m from '@/paraglide/messages';
import { categoryProfiles } from '../$type/_modules/utils';
import { ProfileType } from './consts';

const LinkButton = ({
  href,
  exact = false,
  children,
}: PropsWithChildren<{ href: string; exact?: boolean }>) => {
  const matchRoute = useMatchRoute();
  const isActive = !!matchRoute({ to: href, fuzzy: !exact });

  return (
    <Button variant="fab" data-active={String(isActive)} asChild>
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

const ScriptBadge = ({ children }: PropsWithChildren) => (
  <div className="relative">
    <CodeRounded className="size-8" />
    <span className="bg-surface absolute -right-1 bottom-0 flex size-4 items-center justify-center rounded text-[9px] font-black shadow-sm">
      {children}
    </span>
  </div>
);

const ROUTES = {
  [ProfileType.Profile]: {
    label: m.profile_profile_label(),
    href: '/main/profiles/profile',
    icon: () => (
      <div className="relative">
        <DescriptionOutlineRounded className="size-8" />
        <MemoryRounded className="bg-surface absolute -right-0.5 bottom-0 size-4 rotate-12 rounded p-0.5" />
      </div>
    ),
  },
  [ProfileType.JavaScript]: {
    label: m.profile_javascript_label(),
    href: '/main/profiles/javascript',
    icon: () => (
      <ScriptBadge>
        <JavascriptRounded className="size-4" />
      </ScriptBadge>
    ),
  },
  [ProfileType.Lua]: {
    label: m.profile_lua_label(),
    href: '/main/profiles/lua',
    icon: () => <ScriptBadge>Lua</ScriptBadge>,
  },
  [ProfileType.Merge]: {
    label: m.profile_merge_label(),
    href: '/main/profiles/merge',
    icon: () => (
      <div className="relative">
        <CodeRounded className="size-8" />
        <CallMergeRounded className="bg-surface absolute -right-0.5 bottom-0 size-4 rotate-12 rounded p-0.5" />
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

  return (
    <div className={cn('flex flex-col gap-2', className)} {...props}>
      {Object.entries(ROUTES).map(([profileType, route]) => (
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
      <LinkButton href="/main/profiles/inspect">Profile Inspect</LinkButton>
    </div>
  );
}
