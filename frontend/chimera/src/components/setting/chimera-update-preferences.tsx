import {
  mutations,
  queries,
  useSetting,
  type ReleaseChannel,
  type UpdateSource,
} from '@chimera/interface';
import { SwitchItem } from '@chimera/ui/switch';
import { useMutation, useQuery } from '@tanstack/react-query';
import { useChimeraUpdate } from '@/components/providers/chimera-update-provider';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const SOURCE_ORDER: UpdateSource[] = [
  'chimera',
  'github',
  'ghfast',
  'sourceforge',
];
const DEFAULT_SOURCES: UpdateSource[] = ['chimera', 'github'];

function sourceLabel(source: UpdateSource) {
  switch (source) {
    case 'chimera':
      return m.settings_about_update_source_chimera();
    case 'github':
      return m.settings_about_update_source_github();
    case 'ghfast':
      return m.settings_about_update_source_ghfast();
    case 'sourceforge':
      return m.settings_about_update_source_sourceforge();
  }
}

export default function ChimeraUpdatePreferences() {
  const autoCheck = useSetting('enable_auto_check_update');
  const autoDownload = useSetting('enable_auto_download_update');
  const updateSources = useSetting('update_sources');
  const releaseChannel = useSetting('release_channel');
  const { isBusy, isSupported } = useChimeraUpdate();
  const installedChannelQuery = useQuery({
    ...queries.getReleaseChannel(),
    enabled: isSupported,
  });
  const setReleaseChannel = useMutation(mutations.setReleaseChannel);
  const installedChannel = installedChannelQuery.data?.installed ?? null;
  const sources = updateSources.value?.length
    ? updateSources.value
    : DEFAULT_SOURCES;

  const saveSources = async (next: UpdateSource[]) => {
    try {
      await updateSources.upsert(next);
      await updateSources.refetch();
    } catch (error) {
      message(formatError(error), { kind: 'error', title: 'Update settings' });
    }
  };

  const toggleSource = async (source: UpdateSource, checked: boolean) => {
    if (isBusy || updateSources.isPending) return;
    if (checked) {
      await saveSources([...sources, source]);
    } else if (sources.length > 1) {
      await saveSources(sources.filter((item) => item !== source));
    }
  };

  const moveSource = async (index: number, direction: -1 | 1) => {
    const destination = index + direction;
    if (destination < 0 || destination >= sources.length) return;
    const next = [...sources];
    [next[index], next[destination]] = [next[destination], next[index]];
    await saveSources(next);
  };

  const selectChannel = async (channel: ReleaseChannel) => {
    try {
      await setReleaseChannel.mutateAsync(channel);
      await releaseChannel.refetch();
    } catch (error) {
      message(formatError(error), { kind: 'error', title: 'Update settings' });
    }
  };

  if (!isSupported) return null;

  return (
    <section className="bg-surface-variant/20 flex flex-col gap-2 rounded-xl p-3">
      <SwitchItem
        className="rounded-lg"
        checked={autoDownload.value ?? false}
        onCheckedChange={(checked) =>
          void autoDownload.upsert(checked).catch((error) =>
            message(formatError(error), {
              kind: 'error',
              title: 'Update settings',
            }),
          )
        }
        loading={autoDownload.isPending}
        disabled={
          isBusy ||
          autoDownload.isPending ||
          autoCheck.value !== true ||
          autoCheck.isPending
        }
        aria-label={m.settings_about_update_auto_download()}
      >
        <span className="flex flex-col">
          <span>{m.settings_about_update_auto_download()}</span>
          <span className="text-on-surface-variant text-xs">
            {m.settings_about_update_auto_download_description()}
          </span>
        </span>
      </SwitchItem>

      <label className="bg-surface-variant/30 flex items-center justify-between gap-3 rounded-lg p-3">
        <span className="flex flex-col">
          <span>{m.settings_about_update_release_channel()}</span>
          <span className="text-on-surface-variant text-xs">
            {installedChannel === 'nightly'
              ? m.settings_about_update_release_channel_nightly_locked()
              : m.settings_about_update_release_channel_description()}
          </span>
        </span>
        <select
          data-slot="app-update-release-channel"
          aria-label={m.settings_about_update_release_channel()}
          className="bg-background rounded-md border px-2 py-1"
          value={releaseChannel.value ?? installedChannel ?? 'stable'}
          disabled={
            isBusy ||
            installedChannel === 'nightly' ||
            releaseChannel.isPending ||
            setReleaseChannel.isPending ||
            installedChannel == null
          }
          onChange={(event) =>
            void selectChannel(event.target.value as ReleaseChannel)
          }
        >
          <option value="stable">
            {m.settings_about_update_channel_stable()}
          </option>
          <option value="beta">{m.settings_about_update_channel_beta()}</option>
          <option value="nightly">
            {m.settings_about_update_channel_nightly()}
          </option>
        </select>
      </label>

      <div className="bg-surface-variant/30 flex flex-col gap-2 rounded-lg p-3">
        <div>
          <div>{m.settings_about_update_sources()}</div>
          <p className="text-on-surface-variant text-xs">
            {m.settings_about_update_sources_description()}
          </p>
        </div>
        {SOURCE_ORDER.map((source) => {
          const index = sources.indexOf(source);
          return (
            <div
              key={source}
              className="flex items-center justify-between gap-2 text-sm"
            >
              <label className="flex items-center gap-2">
                <input
                  type="checkbox"
                  data-slot={`app-update-source-${source}`}
                  checked={index >= 0}
                  disabled={
                    isBusy ||
                    updateSources.isPending ||
                    (index >= 0 && sources.length === 1)
                  }
                  onChange={(event) =>
                    void toggleSource(source, event.target.checked)
                  }
                />
                {sourceLabel(source)}
                {index >= 0 && (
                  <span className="text-on-surface-variant text-xs">
                    {m.settings_about_update_source_priority({
                      position: index + 1,
                    })}
                  </span>
                )}
              </label>
              {index >= 0 && (
                <span className="flex gap-1">
                  <button
                    type="button"
                    aria-label={m.settings_about_update_source_move_up({
                      source: sourceLabel(source),
                    })}
                    disabled={isBusy || updateSources.isPending || index === 0}
                    onClick={() => void moveSource(index, -1)}
                  >
                    ↑
                  </button>
                  <button
                    type="button"
                    aria-label={m.settings_about_update_source_move_down({
                      source: sourceLabel(source),
                    })}
                    disabled={
                      isBusy ||
                      updateSources.isPending ||
                      index === sources.length - 1
                    }
                    onClick={() => void moveSource(index, 1)}
                  >
                    ↓
                  </button>
                </span>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}
