import { useSetting } from '@chimera/interface';
import { AnimatePresence, motion } from 'framer-motion';
import { memo } from 'react';

export const ProxyGroupName = memo(function ProxyGroupName({
  name,
}: {
  name: string;
}) {
  // todo: support global setting
  // const { value: disbaleMotion } = useSetting('lighten_animation_effects');
  const disbaleMotion = true;

  return disbaleMotion ? (
    <>{name}</>
  ) : (
    <AnimatePresence mode="sync" initial={false}>
      <motion.div
        key={`group-name-${name}`}
        className="absolute"
        initial={{ x: 100, opacity: 0 }}
        animate={{ x: 0, opacity: 1 }}
        exit={{ x: -100, opacity: 0 }}
        transition={{
          type: 'spring',
          bounce: 0,
          duration: 0.5,
        }}
      >
        {name}
      </motion.div>
    </AnimatePresence>
  );
});

export default ProxyGroupName;
