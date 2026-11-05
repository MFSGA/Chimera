# Ref-alignment: Tooltip provider and popover lifecycle

## Baseline, objective and risk

- Chimera baseline: `master@d438e217880e542bd75d86aa4e31bf79e1e2ca8e`.
- Pinned reference: `ref@5ed3c019e13015fc5b06fc8d7069b330607d9036`.
- Objective: converge the shared Tooltip controlled-state/provider/content behavior with the pinned reference.
- Non-goals: change backend, IPC, legacy MUI Tooltip, user data, the agent, submodules, or global theming.
- Risk L2: hover delay, keyboard focus, Escape, and exit animation may affect both main and legacy views.

## Mapping

| Reference | Chimera | Decision |
| --- | --- | --- |
| `frontend/ui/src/tooltip.tsx` | same | Use ref controlled/uncontrolled root, AnimatePresence portal lifecycle and the already-aligned TooltipBubble. Open delay 120ms and skip-delay window 300ms |
| `frontend/nyanpasu/src/pages/__root.tsx` | `frontend/chimera/src/pages/__root.tsx` | One TooltipProvider around Outlet within CustomCssProvider; keep Chimera providers and window readiness unchanged |
| Reference browser behavior tests | `frontend/ui/tests/ref-tooltip-contract.test.ts` | Add Node SSR source-loading and root-provider composition checks; **not** WebDriver validation |

The old nested zero-delay Provider is removed. MUI Tooltip used by legacy components is not replaced. The copied Tooltip implementation preserves ref symbols and animation transitions; Chimera branding stays in package imports.

## Verification and rollback

The local DevSpace connection was unavailable during this iteration. No local typecheck, unit-test, browser or desktop E2E results are claimed. Remote Draft PR CI is required before merge, followed by actual focus, hover-delay, Escape, overlay nesting and exit animation checks. Remote API-authored commits may need maintainer signing when local access returns.

Rollback: revert the isolated feature-branch commit. No persisted data or backend migration is involved.
