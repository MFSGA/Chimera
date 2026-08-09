import { commands, unwrapResult } from '@chimera/interface';
import { useQuery } from '@tanstack/react-query';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import * as m from '@/paraglide/messages';
import {
  SettingsCard,
  SettingsCardAnimatedItem,
  SettingsCardContent,
  SettingsCardFooter,
  SettingsCardHeader,
} from '../../_modules/settings-card';

const formatValue = (value: string) => {
  try {
    const parsed = JSON.parse(value);
    const normalized = typeof parsed === 'string' ? JSON.parse(parsed) : parsed;
    return JSON.stringify(normalized, null, 2);
  } catch {
    return value;
  }
};

export default function KVStorage() {
  const query = useQuery({
    queryKey: ['debug-kv-storage'],
    queryFn: async () => unwrapResult(await commands.getAllStorageItems()),
  });

  const handleRemove = async (key: string) => {
    unwrapResult(await commands.removeStorageItem(key));
    await query.refetch();
  };

  const handleClear = async () => {
    unwrapResult(await commands.clearStorage());
    await query.refetch();
  };

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>KV Storage</SettingsCardHeader>
        <SettingsCardContent>
          <div className="flex items-center gap-1 select-text">
            <span className="font-medium">Total Items:</span>
            <span>
              {query.isLoading ? 'Loading…' : (query.data?.length ?? 0)}
            </span>
          </div>

          {query.data?.map((storage) => (
            <div key={storage.key} className="flex items-center gap-2">
              <div className="min-w-0 flex-1 truncate">{storage.key}</div>
              <Button
                variant="stroked"
                className="h-8 min-w-0 px-3"
                onClick={() => void handleRemove(storage.key)}
              >
                Delete
              </Button>
              <Modal>
                <ModalTrigger asChild>
                  <Button variant="stroked" className="h-8 min-w-0 px-3">
                    Detail
                  </Button>
                </ModalTrigger>
                <ModalContent>
                  <Card className="min-w-96">
                    <CardHeader>
                      <ModalTitle>Storage Detail</ModalTitle>
                    </CardHeader>
                    <CardContent>
                      <pre className="max-h-[70vh] max-w-[80vw] overflow-auto font-mono text-wrap select-text">
                        {formatValue(storage.value)}
                      </pre>
                    </CardContent>
                    <CardFooter>
                      <ModalClose>{m.common_close()}</ModalClose>
                    </CardFooter>
                  </Card>
                </ModalContent>
              </Modal>
            </div>
          ))}
        </SettingsCardContent>
        <SettingsCardFooter>
          <Button onClick={() => void handleClear()}>Clear All Storage</Button>
        </SettingsCardFooter>
      </SettingsCardAnimatedItem>
    </SettingsCard>
  );
}
