import { LogFilter } from './log-filter';
import { LogLevel } from './log-level';

export const LogHeader = () => {
  return (
    <div className="flex gap-2">
      <LogLevel />
      <LogFilter />
    </div>
  );
};

export default LogHeader;
