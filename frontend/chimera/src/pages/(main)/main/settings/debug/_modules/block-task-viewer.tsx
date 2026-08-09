import { useBlockTaskContext } from '@/components/providers/block-task-provider';
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

export default function BlockTaskViewer() {
  const { tasks, clearTask } = useBlockTaskContext();
  const entries = Object.entries(tasks);

  return (
    <SettingsCard asChild>
      <SettingsCardAnimatedItem>
        <SettingsCardHeader>Block Task Viewer</SettingsCardHeader>
        <SettingsCardContent>
          {entries.length === 0 ? (
            <div className="text-on-surface-variant">No active tasks</div>
          ) : (
            entries.map(([key, task]) => (
              <div key={key} className="flex items-center gap-2">
                <div className="min-w-0 flex-1 truncate">{key}</div>
                <div>{task.status}</div>
                <Modal>
                  <ModalTrigger asChild>
                    <Button variant="stroked" className="h-8 min-w-0 px-3">
                      Detail
                    </Button>
                  </ModalTrigger>
                  <ModalContent>
                    <Card className="min-w-96">
                      <CardHeader>
                        <ModalTitle>Task Detail</ModalTitle>
                      </CardHeader>
                      <CardContent>
                        <pre className="max-h-[60vh] overflow-auto font-mono select-text">
                          {JSON.stringify(task, null, 2)}
                        </pre>
                      </CardContent>
                      <CardFooter className="gap-2">
                        <ModalClose>{m.common_close()}</ModalClose>
                        <Button onClick={() => clearTask(key)}>Clear</Button>
                      </CardFooter>
                    </Card>
                  </ModalContent>
                </Modal>
              </div>
            ))
          )}
        </SettingsCardContent>
        {entries.length > 0 && (
          <SettingsCardFooter>
            <Button onClick={() => entries.forEach(([key]) => clearTask(key))}>
              Clear All
            </Button>
          </SettingsCardFooter>
        )}
      </SettingsCardAnimatedItem>
    </SettingsCard>
  );
}
