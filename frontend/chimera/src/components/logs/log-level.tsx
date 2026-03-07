import { alpha } from '@chimera/ui';
import { Button, Menu, MenuItem } from '@mui/material';
import { useState } from 'react';
import { useLogContext } from './log-provider';

export const LogLevel = () => {
  const { logLevel, setLogLevel } = useLogContext();
  const [anchorEl, setAnchorEl] = useState<null | HTMLElement>(null);

  const handleClick = (value: string) => {
    setAnchorEl(null);
    setLogLevel(value);
  };

  const mapping: Record<string, string> = {
    all: 'ALL',
    inf: 'INFO',
    warn: 'WARN',
    err: 'ERROR',
  };

  return (
    <>
      <Button
        size="small"
        sx={(theme) => ({
          minWidth: 78,
          height: 36,
          borderRadius: 10,
          textTransform: 'none',
          fontSize: 13,
          fontWeight: 700,
          letterSpacing: '0.08em',
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
          color: theme.vars.palette.text.primary,
          '&:hover': {
            backgroundColor: alpha(theme.vars.palette.primary.main, 0.16),
          },
        })}
        onClick={(event) => setAnchorEl(event.currentTarget)}
      >
        {mapping[logLevel]}
      </Button>

      <Menu
        anchorEl={anchorEl}
        open={Boolean(anchorEl)}
        onClose={() => setAnchorEl(null)}
        slotProps={{
          paper: {
            sx: {
              mt: 1,
              borderRadius: 3,
            },
          },
        }}
      >
        {Object.entries(mapping).map(([key, value]) => {
          return (
            <MenuItem key={key} onClick={() => handleClick(key)}>
              {value}
            </MenuItem>
          );
        })}
      </Menu>
    </>
  );
};
