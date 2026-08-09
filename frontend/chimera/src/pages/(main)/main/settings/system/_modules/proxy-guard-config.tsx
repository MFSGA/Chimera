import { useSetting } from '@chimera/interface';
import { AnimatePresence } from 'motion/react';
import { useEffect, useState } from 'react';
import { Button } from '@/components/ui/button';
import { NumericInput } from '@/components/ui/input';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { SettingsCardAnimatedItem } from '../../_modules/settings-card';

export default function ProxyGuardConfig() {
  const proxyGuardInterval = useSetting('proxy_guard_interval');
  const savedValue = proxyGuardInterval.value ?? 1;
  const [draft, setDraft] = useState<number | null>(savedValue);

  useEffect(() => setDraft(savedValue), [savedValue]);

  const isDirty = draft !== savedValue;
  const isValid = draft != null && Number.isInteger(draft) && draft >= 1;

  const handleApply = async () => {
    if (!isValid || draft == null) return;

    try {
      await proxyGuardInterval.upsert(draft);
    } catch (error) {
      message(formatError(error), { title: m.common_error(), kind: 'error' });
    }
  };

  return (
    <div className="flex flex-col gap-2">
      <NumericInput
        variant="outlined"
        label={m.settings_system_proxy_proxy_guard_interval_label()}
        value={draft}
        min={1}
        allowNegative={false}
        decimalScale={0}
        onChange={setDraft}
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
                disabled={!isValid}
                loading={proxyGuardInterval.isPending}
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
