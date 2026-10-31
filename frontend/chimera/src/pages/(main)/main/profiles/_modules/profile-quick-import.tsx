import { useProfile, type FolderId } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import CloseSmallOutlineRounded from '~icons/material-symbols/close-small-outline-rounded';
import DownloadRounded from '~icons/material-symbols/download-rounded';
import LinkRounded from '~icons/material-symbols/link-rounded';
import { useState } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { subscriptionUrlSchema } from './subscription-url-schema';

export default function ProfileQuickImport({
  folder,
}: {
  folder?: FolderId | null;
}) {
  const { create } = useProfile();
  const [url, setUrl] = useState('');
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (event: React.FormEvent) => {
    event.preventDefault();
    const value = url.trim();

    if (!subscriptionUrlSchema.safeParse(value).success) {
      void message(m.subscription_onboarding_invalid_protocol(), {
        title: m.common_error(),
        kind: 'error',
      });
      return;
    }

    try {
      setLoading(true);
      await create.mutateAsync({
        type: 'url',
        data: { url: value, option: null, folder: folder ?? null },
      });
      setUrl('');
      message(m.profile_quick_import_success_message(), {
        title: m.common_success(),
        kind: 'info',
      });
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    } finally {
      setLoading(false);
    }
  };

  return (
    <form
      className="bg-surface shadow-outline/30 dark:shadow-surface-variant/10 relative flex h-10 w-full flex-1 items-center gap-1 rounded-full pr-1 pl-3 shadow"
      onSubmit={(event) => void handleSubmit(event)}
      data-slot="profile-quick-import"
    >
      <LinkRounded className="size-6" />
      <input
        className="h-full min-w-0 flex-1 bg-transparent px-1 text-sm outline-hidden"
        type="url"
        value={url}
        onChange={(event) => setUrl(event.target.value)}
        placeholder={m.profile_quick_import_placeholder()}
        autoComplete="off"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />

      {url && (
        <>
          {!loading && (
            <Button
              icon
              className="size-8"
              type="button"
              onClick={() => setUrl('')}
            >
              <CloseSmallOutlineRounded className="size-6" />
            </Button>
          )}
          <Button icon className="size-8" type="submit" loading={loading}>
            <DownloadRounded className="size-6" />
          </Button>
        </>
      )}
    </form>
  );
}
