import {
  scopedTransformsOf,
  useProfile,
  type ProfileDefinition_Deserialize,
} from '@chimera/interface';
import { useMemo } from 'react';
import * as m from '@/paraglide/messages';
import { type ConfigProfile } from '../../../_modules/utils';
import { TransformChainEditor } from './chian-editor-card';
import { DetailSection } from './detail-section';

/** The transform chain that applies only to one config item. */
export default function TransformChainSection({
  profile,
}: {
  profile: ConfigProfile;
}) {
  const { replaceDefinition } = useProfile();

  // The edited config item's own scoped transforms (File or Composition).
  const transforms = useMemo(() => scopedTransformsOf(profile), [profile]);

  return (
    <DetailSection
      title={m.profile_chain_editor_title()}
      description={m.profile_chain_editor_scoped_hint()}
      className="col-span-2 md:col-span-4"
    >
      {/* Keyed per profile so a draft cannot leak into another config whose
          baseline happens to be identical. */}
      {/* Temporary adapter: the upstream shared TransformChainList has not yet
          replaced Chimera's global/scoped editor with runtime diagnostics. */}
      <TransformChainEditor
        key={profile.uid}
        layout="stacked"
        scopeUid={profile.uid}
        showTitle={false}
        taskKey={`update-chain-${profile.uid}`}
        transforms={transforms}
        onApply={async (next) => {
          // Rebuild the config definition with the reordered scoped
          // transforms, preserving every other field, and replace it
          // atomically.
          const definition: ProfileDefinition_Deserialize = {
            type: 'config',
            config: { ...profile.config, transforms: next },
          };

          await replaceDefinition.mutateAsync({
            uid: profile.uid,
            definition,
          });
        }}
      />
    </DetailSection>
  );
}
