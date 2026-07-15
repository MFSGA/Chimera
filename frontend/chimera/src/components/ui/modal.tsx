import { cn } from '@chimera/ui';
import { AnimatePresence, motion } from 'motion/react';
import { Dialog as DialogPrimitive, Slot } from 'radix-ui';
import {
  createContext,
  useContext,
  useId,
  useState,
  type ComponentProps,
} from 'react';
import { Button, type ButtonProps } from './button';

export const ModalPortal = DialogPrimitive.Portal;
export const ModalTitle = DialogPrimitive.Title;
export const ModalDescription = DialogPrimitive.Description;

type ModalContextValue = { open: boolean; layoutId: string };
const ModalContext = createContext<ModalContextValue | null>(null);

const useModalContext = () => {
  const context = useContext(ModalContext);
  if (!context) {
    throw new Error('Modal compound components must be rendered inside Modal');
  }
  return context;
};

export function ModalTrigger({
  className,
  children,
  asChild,
  ...props
}: ComponentProps<typeof DialogPrimitive.Trigger>) {
  const { layoutId } = useModalContext();
  const Comp = asChild ? Slot.Root : 'button';

  return (
    <DialogPrimitive.Trigger {...props} asChild>
      <Comp
        className={cn('relative', className)}
        data-slot="modal-trigger"
        data-layout-id={layoutId}
      >
        <Slot.Slottable>{children}</Slot.Slottable>
        <div className="@container-[size] absolute inset-0 -z-10 flex items-center justify-center">
          <motion.div
            className="size-full"
            style={{
              maxWidth: 'min(100%, calc(4 * 100cqh))',
              maxHeight: 'min(100%, calc(4 * 100cqw))',
            }}
            layout
            layoutId={layoutId}
          />
        </div>
      </Comp>
    </DialogPrimitive.Trigger>
  );
}

export function ModalClose({
  children,
  asChild,
  ...props
}: ComponentProps<typeof DialogPrimitive.Close> & ButtonProps) {
  return (
    <DialogPrimitive.Close {...props} asChild>
      {asChild ? children : <Button>{children}</Button>}
    </DialogPrimitive.Close>
  );
}

export function ModalOverlay({
  className,
  ...props
}: ComponentProps<typeof DialogPrimitive.Overlay>) {
  return (
    <DialogPrimitive.Overlay {...props} asChild>
      <motion.div
        className={cn(
          'bg-on-primary-container/30 dark:bg-on-primary/15 fixed inset-0 z-50 backdrop-blur-xl',
          className,
        )}
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
      />
    </DialogPrimitive.Overlay>
  );
}

export function ModalContent({
  className,
  children,
  ...props
}: ComponentProps<typeof DialogPrimitive.Content>) {
  const { open, layoutId } = useModalContext();

  return (
    <AnimatePresence initial={false}>
      {open && (
        <ModalPortal forceMount>
          <ModalOverlay />
          <div
            className={cn(
              'fixed inset-0 z-50 grid place-items-center',
              className,
            )}
          >
            <DialogPrimitive.Content
              {...props}
              aria-describedby={undefined}
              data-slot="modal-content"
              data-layout-id={layoutId}
              asChild
            >
              <motion.div
                layout
                layoutId={layoutId}
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
              >
                {children}
              </motion.div>
            </DialogPrimitive.Content>
          </div>
        </ModalPortal>
      )}
    </AnimatePresence>
  );
}

export function Modal({
  open: controlledOpen,
  defaultOpen,
  onOpenChange,
  ...props
}: ComponentProps<typeof DialogPrimitive.Root>) {
  const layoutId = useId();
  const [uncontrolledOpen, setUncontrolledOpen] = useState(
    defaultOpen ?? false,
  );
  const open = controlledOpen ?? uncontrolledOpen;
  const setOpen = (next: boolean) => {
    if (controlledOpen === undefined) setUncontrolledOpen(next);
    onOpenChange?.(next);
  };

  return (
    <ModalContext.Provider value={{ open, layoutId }}>
      <DialogPrimitive.Root open={open} onOpenChange={setOpen} {...props} />
    </ModalContext.Provider>
  );
}
