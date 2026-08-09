import { useSetting } from '@chimera/interface';
import { AnimatePresence } from 'motion/react';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { SettingsCardAnimatedItem } from '../../_modules/settings-card';

const DEFAULT_BYPASS =
  'localhost;127.;192.168.;10.;' +
  '172.16.;172.17.;172.18.;172.19.;172.20.;172.21.;172.22.;172.23.;' +
  '172.24.;172.25.;172.26.;172.27.;172.28.;172.29.;172.30.;172.31.*';

export default function ProxyBypassConfig() {
  const systemProxyBypass = useSetting('system_proxy_bypass');
  const savedValue = systemProxyBypass.value ?? '';
  const [draft, setDraft] = useState(savedValue);

  useEffect(() => setDraft(savedValue), [savedValue]);

  const isDirty = draft !== savedValue;

  const handleApply = async () => {
    try {
      await systemProxyBypass.upsert(draft || DEFAULT_BYPASS);
    } catch (error) {
      message(formatError(error), { title: m.common_error(), kind: 'error' });
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <Input
        variant="outlined"
        label={m.settings_system_proxy_proxy_bypass_label()}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
      />

      <AnimatePresence initial={false}>
        {isDirty && (
          <SettingsCardAnimatedItem>
            <div className="flex justify-end gap-2 pt-1">
              <Button type="button" onClick={() => setDraft(savedValue)}>
                {m.common_reset()}
              </Button>
              <Button
                variant="raised"
                loading={systemProxyBypass.isPending}
                onClick={() => void handleApply()}
              >
                {m.common_apply()}
              </Button>
            </div>
          </SettingsCardAnimatedItem>
        )}
      </AnimatePresence>
    </div>
  );
}
