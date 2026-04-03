import { cn } from '@chimera/ui';
import { Add } from '@mui/icons-material';
import { Fab } from '@mui/material';
import { use, useEffect, useState } from 'react';
import { AddProfileContext, ProfileDialog } from './profile-dialog';

export const NewProfileButton = ({ className }: { className?: string }) => {
  const addProfileCtx = use(AddProfileContext);
  const [open, setOpen] = useState(Boolean(addProfileCtx));

  useEffect(() => {
    setOpen(Boolean(addProfileCtx));
  }, [addProfileCtx]);

  return (
    <>
      <Fab
        className={cn(className)}
        color="primary"
        onClick={() => setOpen(true)}
      >
        <Add />
      </Fab>

      <ProfileDialog open={open} onClose={() => setOpen(false)} />
    </>
  );
};

export default NewProfileButton;
