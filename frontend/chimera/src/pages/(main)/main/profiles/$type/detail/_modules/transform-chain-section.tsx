import { scopedTransformsOf, useProfile } from '@chimera/interface';
import { useMemo } from 'react';
import * as m from '@/paraglide/messages';
import type { ConfigProfile } from '../../../_modules/utils';
import { DetailRow } from './detail-section';

/** The persisted scoped chain entry that opens the transform side panel. */
export default function TransformChainSection({
  profile,
  selected = false,
  onOpen,
}: {
  profile: ConfigProfile;
  selected?: boolean;
  onOpen: () => void;
}) {
  const {
    query: { data },
  } = useProfile();

  const enabled = useMemo(() => {
    const names = new Map(
      (data?.items ?? [])
        .filter((item) => item.type === 'transform')
        .map((item) => [item.uid, item.name]),
    );
    return scopedTransformsOf(profile).flatMap((uid) => {
      const name = names.get(uid);
      return name ? [name] : [];
    });
  }, [data?.items, profile]);

  const summary =
    data == null
      ? undefined
      : enabled.length === 0
        ? m.profile_chain_editor_empty()
        : [
            m.profile_chain_editor_enabled_count({ count: enabled.length }),
            enabled.join(', '),
          ].join(' · ');

  return (
    <DetailRow
      label={m.profile_chain_editor_title()}
      description={summary}
      aria-current={selected || undefined}
      data-selected={selected || undefined}
      data-profile-uid={profile.uid}
      data-panel="transforms"
      data-slot="profile-transform-chain-entry"
      onClick={onOpen}
    />
  );
}
