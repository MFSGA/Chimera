import { main } from './src/prepare/check.ts';

// Keep the historical command path working while the prepare implementation
// follows the reference module layout under scripts/src/prepare/.
export { main } from './src/prepare/check.ts';

if (import.meta.main) await main();
