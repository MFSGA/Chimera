# Shared FilterChip and Accordion reference alignment

## Baseline, objective and risk

- Chimera baseline: `master@7baaf1cdec5db7bed53e5ab010c5274b33392c23`.
- Pinned, read-only Clash Nyanpasu reference:
  `ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`.
- Objective: add reference-equivalent interactive state primitives to the
  shared UI package without changing existing main/legacy UI flows.
- Risk L2: the primitives include controlled state, Radix keyboard semantics
  and motion exit transitions; real browser verification is necessary.
- Non-goals: backend, IPC, agent, user data, persisted preferences, reference
  gitlink, and wholesale replacement of Chimera's existing filter UI.

## Source, symbol and caller mapping

- `ref/frontend/ui/src/chip.tsx::FilterChip,FilterChipProps` maps to
  `frontend/ui/src/chip.tsx`. The ref's controlled/uncontrolled
  `pressed` logic, `TogglePrimitive.Root`, check icon transitions
  and `useReducedMotion` are retained; only `@nyanpasu/utils` is mapped to
  `@chimera/utils` and project formatting is applied.
- `ref/frontend/ui/src/accordion.tsx::{Accordion,AccordionItem,
  AccordionTrigger,AccordionContent}` maps to
  `frontend/ui/src/accordion.tsx`. Use the exact reference single-open,
  collapsible state semantics and `AnimatePresence` content lifecycle,
  with reduced-motion fallbacks and context-driven chevrons.
- Both are exported from `frontend/ui/src/index.ts`, as in the upstream
  reference; the existing deep-import wildcard also exposes each path.
- Main UI's existing
  `frontend/chimera/src/pages/(main)/main/_modules/filter-chip.tsx`
  is a Chimera-specific wrapper with its own filtering behavior. It is not
  replaced by the new shared `FilterChip` during this foundation slice.
  The legacy UI is not changed. Future caller-by-caller alignment needs
  separate review and user-visible E2E acceptance.

## Executed checks and limitations

- Six Node contracts were **executed and passed (6/6)** with the actual
  production TypeScript sources compiled in a temporary isolated harness.
  Git Blob SHA verification confirms the tested source and test files match
  this PR's committed bytes.
- Contracts cover FilterChip default/controlled/disabled state, pressed
  check animation and reduced motion; Accordion single/collapsible state,
  callbacks, expanded chevron/content, closed content and reduced motion.
- `pnpm --filter @chimera/ui test:unit` executes the committed tests.
  No dependencies or lockfile changes are required.
- The tests mock Radix, React and Motion boundaries. They do **not**
  establish real DOM accessibility, Space/Enter activation, pointer focus,
  animation end timing, actual unmount or user-visible layout.
- The local AppleDev workspace was unavailable. Frozen install, full
  TypeScript typecheck, frontend boundaries, repository Prettier, production
  Vite build, and actual desktop WebDriver have **not** been run.
  This API-created commit is unsigned and the PR must stay Draft until
  locally verified/signed and independently reviewed.
- Prioritize the pinned reference's
  `ref/frontend/ui/tests/chip.browser.test.tsx` click, Space and disabled
  scenarios plus Accordion keyboard/focus/exit E2E before approval.

## Rollback

Revert the single feature commit. No persisted data or shared application
API state is migrated; existing main/legacy filter flows remain intact.
