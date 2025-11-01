import { useAtom } from 'jotai';
import { lazy, Suspense } from 'react';
import { UpdaterInstanceAtom } from '@/store/updater';

const UpdaterDialog = lazy(() => import('./updater-dialog'));

export const UpdaterDialogWrapper = () => {
  const [manifest, setManifest] = useAtom(UpdaterInstanceAtom);

  if (!manifest) return null;
  return (
    <Suspense fallback={null}>
      <UpdaterDialog
        open
        onClose={() => {
          setManifest(null);
        }}
        update={manifest}
      />
    </Suspense>
  );
};

export default UpdaterDialogWrapper;
