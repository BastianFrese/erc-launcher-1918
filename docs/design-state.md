# Design State Audit

## AUDIT
Scope: Native desktop UI (installer and launcher) of the Swofty Launcher and its surrounding documentation.

## PROJECT TYPE
Native desktop launcher & mod manager for F1 25 – **confirmed** (see `installer/src/main.rs` uses `eframe` / `egui` to create a desktop window).

## PURPOSE
Provides installation, update, and launch of the Swofty Launcher, which manages addons, mods, profiles, and in‑game addon settings for F1 25. **Sources:** `README.md` (lines 1‑4), `docs/guide/installation.md` (overview of installer, requirements).

## AUDIENCE / GOAL
Primary audience: F1 25 players who want to mod the game. Goal: Let users install the launcher, manage addons/mods, and start the game with selected profiles. **Sources:** `README.md` (features list), installation guide (requirements, supported platforms).

## DESIGN DIRECTION
Adaptive dark UI with high contrast, minimal visual noise, and clear primary actions (Install, Browse). Emphasises clarity over decorative styles. **Confidence:** high – matches existing colour palette and layout.

## STACK
- **Language / Build:** Rust, Cargo (`Cargo.toml`).
- **UI framework:** eframe + egui (native desktop GUI).
- **Styling:** Colours defined as Rust constants in `installer/src/theme.rs`; UI layout driven by egui widgets, no CSS.
- **Run / test commands:** `cargo run -p swofty-launcher` (from README), `cargo test` for unit tests.

## FILES INSPECTED
Approx. 10 files:
- `README.md`
- `docs/guide/installation.md`
- `installer/src/main.rs`
- `installer/src/app/view.rs`
- `installer/src/theme.rs`
- `installer/src/app/mod.rs`
- `Cargo.toml`
- `docs/design/index.md`
- `installer/README.md`
- `installer/src/data/mod.rs`

## TOKENS
- **Color tokens:** Defined in `installer/src/theme.rs` (BACKGROUND, SURFACE, TEXT, MUTED, ACCENT, etc.) – present.
- **Spacing / radius tokens:** Corner radius and margins set in `theme.rs` (`CornerRadius::same(6)`, `Margin::same(12)`) – present.
- **Typography tokens:** Not defined – UI uses default egui fonts.
- **Other design tokens:** None.

## TOP ISSUES
1. **Accessibility – focus & keyboard navigation** – UI does not expose explicit focus outlines or keyboard shortcuts in `view.rs`; may fail WCAG 2.2 AA.
2. **Color contrast** – ACCENT (purple) against BACKGROUND may not meet 4.5:1 for normal text; needs verification (`installer/src/theme.rs`).
3. **Internationalisation** – All UI strings are hard‑coded English (e.g., "Install", "Use them") in `view.rs`; no localisation framework.

## UNKNOWNS
None critical; all major aspects identifiable from inspected files.

## NEXT
`/design-implement` – redesign the installer UI focusing on:
- explicit focus indicators and keyboard navigation,
- verifying and adjusting colour contrast,
- extracting UI strings for localisation,
- exposing typography tokens.
