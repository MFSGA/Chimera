import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import {
  Modal,
  ModalContent,
  ModalDescription,
  ModalTitle,
} from '@chimera/ui/modal';
import { m } from '@/paraglide/messages';

/** Controlled by the pending navigation/editor decision, with no local trigger. */
export default function LeaveDialog({
  open,
  description,
  onStay,
  onLeave,
}: {
  open: boolean;
  /** What would be lost, in the words of the page that asks. */
  description: string;
  onStay: () => void;
  onLeave: () => void;
}) {
  return (
    <Modal open={open} onOpenChange={(next) => !next && onStay()}>
      <ModalContent>
        <Card className="w-96" data-slot="profile-leave-dialog">
          <CardHeader>
            <ModalTitle>{m.profile_leave_title()}</ModalTitle>
          </CardHeader>

          <CardContent>
            <ModalDescription className="text-on-surface-variant text-sm">
              {description}
            </ModalDescription>
          </CardContent>

          <CardFooter className="gap-1">
            <Button onClick={onStay} data-slot="profile-leave-stay">
              {m.profile_leave_stay()}
            </Button>

            <Button
              variant="flat"
              onClick={onLeave}
              data-slot="profile-leave-confirm"
            >
              {m.profile_leave_confirm()}
            </Button>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
}
