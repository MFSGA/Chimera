import { useProfile, type ProfileQueryResultItem } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import { useDeleteProfile } from '../../../_modules/use-delete-profile';
import { SettingsGroup } from '../../../../_modules/settings-card';
import { DetailRow } from './detail-section';

/** The one destructive action, kept apart from the rest. */
export default function DeleteSection({
  profile,
  onDeleted,
}: {
  profile: ProfileQueryResultItem;
  onDeleted: () => void | Promise<void>;
}) {
  const { query } = useProfile();
  const isActive = query.data?.current === profile.uid;

  const deleteProfile = useDeleteProfile(profile, {
    description: (isActive
      ? m.profile_detail_delete_confirm_active
      : m.profile_detail_delete_confirm)({ name: profile.name }),
    onSuccess: onDeleted,
  });

  return (
    <SettingsGroup data-slot="profile-detail-delete">
      <DetailRow
        destructive
        label={m.profile_delete_title()}
        description={m.profile_detail_delete_description()}
        loading={deleteProfile.isPending}
        trailing={null}
        onClick={() => void deleteProfile.handleClick()}
      />
    </SettingsGroup>
  );
}
