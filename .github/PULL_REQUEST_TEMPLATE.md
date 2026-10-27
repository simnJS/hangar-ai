## What and why

<!-- What does this change, and what problem does it solve? Link the issue: "Fixes #123". -->

## How it was tested

<!-- Commands you ran and what you checked by hand. Name the systems: Windows, macOS, Linux. -->

- [ ] `pnpm exec tsc --noEmit -p .`, `pnpm exec vitest run`, `pnpm build`, `pnpm demo:build`
- [ ] In `src-tauri/`: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`
- [ ] `claude plugin test src-tauri/claude-mod/hangar-bridge` (only if the mod changed)

## Checklist

- [ ] Commits are in English, in the imperative mood ("Add…", "Keep…", "Show…")
- [ ] New interface text is in `src/i18n.ts`, in English and French (or the PR says the French needs a review)
- [ ] Screenshots or a clip for interface changes
- [ ] A Tauri command added or renamed is also answered in `demo/backend/index.ts`
- [ ] MCP tools added or changed are described in the README
