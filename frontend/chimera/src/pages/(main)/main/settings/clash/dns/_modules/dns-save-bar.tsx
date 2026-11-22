import { Button } from '@chimera/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import { useDnsDraft } from './dns-draft';
import DnsYamlModal from './dns-yaml-modal';

export default function DnsSaveBar() {
  const { dirty, saving, discard, save } = useDnsDraft();
  const handleSave = useLockFn(async () => {
    try {
      await save();
    } catch (error) {
      message(`${m.dns_save_failed()}\n${formatError(error)}`, {
        title: m.common_error(),
        kind: 'error',
      });
    }
  });
  return (
    <div
      className="bg-surface/90 sticky bottom-0 z-10 flex flex-wrap items-center gap-2 py-3 backdrop-blur"
      data-slot="dns-save-bar"
    >
      <DnsYamlModal />
      <div className="flex-1" />
      <Button
        data-slot="dns-discard"
        variant="stroked"
        disabled={!dirty || saving}
        onClick={discard}
      >
        {m.dns_discard()}
      </Button>
      <Button
        data-slot="dns-save"
        variant="flat"
        disabled={!dirty || saving}
        loading={saving}
        onClick={handleSave}
      >
        {m.dns_save()}
      </Button>
    </div>
  );
}
