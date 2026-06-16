import { alpha } from '@chimera/ui';
import { Radar } from '@mui/icons-material';
import { Button, Tooltip } from '@mui/material';

export const ScrollCurrentNode = ({ onClick }: { onClick?: () => void }) => {
  return (
    <Tooltip title="Locate">
      <Button
        size="small"
        className="!size-8 !min-w-0"
        sx={(theme) => ({
          backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
        })}
        onClick={onClick}
      >
        <Radar />
      </Button>
    </Tooltip>
  );
};

export default ScrollCurrentNode;
