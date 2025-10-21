import { alpha } from '@chimera/ui';
import { CircularProgress, SxProps, Theme } from '@mui/material';
import { useControllableValue } from 'ahooks';
import { memo, ReactNode } from 'react';
import { mergeSxProps } from '@/utils/mui-theme';
import { PaperButton, PaperButtonProps } from './paper-button';

export interface PaperSwitchButtonProps extends PaperButtonProps {
  label?: string;
  checked: boolean;
  loading?: boolean;
  disableLoading?: boolean;
  children?: ReactNode;
  onClick?: () => Promise<void> | void;
  sxPaper?: SxProps<Theme>;
}

export const PaperSwitchButton = memo(function PaperSwitchButton({
  label,
  checked,
  loading,
  disableLoading,
  children,
  onClick,
  sxPaper,
  ...props
}: PaperSwitchButtonProps) {
  const [pending, setPending] = useControllableValue<boolean>(
    { loading },
    {
      defaultValue: false,
    },
  );

  const handleClick = async () => {
    if (onClick) {
      if (disableLoading) {
        return onClick();
      }

      setPending(true);
      await onClick();
      setPending(false);
    }
  };

  return (
    <PaperButton
      label={label}
      sxPaper={mergeSxProps(
        (theme: Theme) => ({
          backgroundColor: checked
            ? alpha(theme.palette.primary.main, 0.1)
            : theme.palette.grey[100],
          ...theme.applyStyles('dark', {
            backgroundColor: checked
              ? alpha(theme.palette.primary.main, 0.1)
              : theme.palette.common.black,
          }),
        }),
        sxPaper,
      )}
      sxButton={{
        flexDirection: 'column',
        alignItems: 'start',
        gap: 0.5,
      }}
      onClick={handleClick}
      {...props}
    >
      {pending === true && (
        <CircularProgress
          sx={{
            position: 'absolute',
            bottom: 'calc(50% - 12px)',
            right: 12,
          }}
          color="inherit"
          size={24}
        />
      )}

      {children}
    </PaperButton>
  );
});
