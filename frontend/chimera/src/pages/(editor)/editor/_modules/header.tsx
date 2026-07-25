import type { ComponentProps } from 'react';
import {
  DefaultHeader,
  isMacOS,
  MacOSHeader,
} from '@/components/window/system-titlebar';
import WindowControl from '@/components/window/window-control';
import WindowTitle from '@/components/window/window-title';

const DEFAULT_TITLE = 'Clash Chimera - Editor';

function Title({ title }: { title: string }) {
  return (
    <WindowTitle>
      <div
        className="text-on-surface text-base font-bold text-nowrap"
        data-tauri-drag-region
      >
        {title}
      </div>
    </WindowTitle>
  );
}

export default function Header({
  beforeClose,
  className,
  title = DEFAULT_TITLE,
  ...props
}: ComponentProps<'div'> & {
  beforeClose?: ComponentProps<typeof WindowControl>['beforeClose'];
  title?: string;
}) {
  return isMacOS ? (
    <MacOSHeader className={className} {...props}>
      <Title title={title} />
    </MacOSHeader>
  ) : (
    <DefaultHeader beforeClose={beforeClose} className={className} {...props}>
      <Title title={title} />
    </DefaultHeader>
  );
}
