## What and why

<!-- What does this change, and what problem does it solve? Link the issue: "Fixes #123". -->

## How it was tested

<!-- Commands you ran and what you checked by hand. Name the systems: Windows, macOS, Linux. -->

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`
- [ ] `pnpm mobile:typecheck`, `pnpm test`, `pnpm mobile:build` (only if `mobile/` changed)
- [ ] `claude plugin test crates/hangar-core/claude-mod/hangar-bridge` (only if the mod changed)

## Checklist

- [ ] Commits are in English, in the imperative mood ("Add…", "Keep…", "Show…")
- [ ] New interface text is in `crates/hangar-i18n/dict/en.json` and `fr.json`, regenerated (or the PR says the French needs a review)
- [ ] Screenshots or a clip for interface changes
- [ ] MCP tools added or changed are described in the README
