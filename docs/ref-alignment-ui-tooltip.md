# Ref UI: Tooltip lifecycle and application provider alignment

## Baseline and scope

- Chimera base: `master@d926b1cc`.
- Reference: `ref@911644c8b` (`libnyanpasu/clash-nyanpasu` main).
- Merged implementation: `master@8d903008`.
- Risk: L2. The change moves Tooltip timing policy from a provider created per Tooltip root to one provider at the application router root. No persisted state, IPC, backend or native-window behavior changes.

## Source and behavior mapping

- `frontend/ui/src/tooltip.tsx` follows the latest ref implementation; only the utility import is mapped from `@nyanpasu/utils` to `@chimera/utils` and the current formatter's import style is retained.
- `TooltipProvider` now owns the shared Radix context. Its defaults are a 120 ms opening delay and a 300 ms skip-delay window after a Tooltip closes, avoiding independent zero-delay providers on every Tooltip instance.
- `Tooltip` preserves controlled `open`, uncontrolled `defaultOpen`, and `onOpenChange` behavior. Content uses the reference's animated presence lifecycle so the portal remains during the exit transition and unmounts afterwards.
- `frontend/chimera/src/pages/__root.tsx` places the provider around the router `Outlet`, covering both main and legacy route trees without wrapping the application shell or native window boundary.
- The content's outer Popper is not the rounded visual surface. `TooltipBubble` owns the background, padding and rounded shape; E2E assertions inspect that inner surface.

## Verification

- `frontend/ui/tests/ref-tooltip-contract.test.ts` verifies the Tooltip markup beneath the shared provider and that the router root supplies one top-level provider.
- Full UI unit suite at the following Sparkline migration: 43 passed, 0 failed. Full workspace `pnpm typecheck`, Prettier and changed-file Oxlint passed.
- The Windows Tauri E2E run for `8d903008` passed (`42645724751`). The updated `main-tooltip.e2e.ts` uses W3C WebDriver mouse actions, checks a trusted `pointermove` reaches the trigger, verifies the inner Bubble styling, then moves away and waits for the Tooltip portal to unmount.
- The main sidebar hover path is verified in real WebView2. Other individual legacy tooltip consumers are covered by the shared router provider but have not each received a separate visual E2E case.

## Rollback

Revert the isolated Tooltip lifecycle/provider commit and its contract/E2E changes together. No data migration or backend rollback is required.
