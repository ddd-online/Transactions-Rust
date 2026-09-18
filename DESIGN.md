---
name: Transactions
description: A clean, airy, trustworthy desktop personal finance workspace
colors:
  primary: "#3964fe"
  primary-light: "#5b7fff"
  primary-active: "#2b52e6"
  primary-tint: "rgba(57, 100, 254, 0.08)"
  canvas: "#f9fafb"
  surface: "#ffffff"
  surface-soft: "#f3f4f6"
  text-major: "#0f1115"
  text-secondary: "#61666b"
  text-tertiary: "#81858c"
  text-disabled: "#9aa0a8"
  text-inverse: "#ffffff"
  border: "#e8eaed"
  border-l2: "#e2e4e8"
  border-l3: "#d5d8dd"
  divider: "#eceef1"
  hover-bg: "rgba(15, 17, 21, 0.06)"
  active-bg: "rgba(57, 100, 254, 0.10)"
  income: "#16a34a"
  expense: "#dc2626"
  transfer: "#3b82f6"
  outlier: "#d97706"
  income-tint: "rgba(22, 163, 74, 0.10)"
  expense-tint: "rgba(220, 38, 38, 0.10)"
  transfer-tint: "rgba(59, 130, 246, 0.10)"
  outlier-tint: "rgba(217, 119, 6, 0.10)"
  ledger-forest: "#4a8e70"
  ledger-amber: "#c6963a"
  ledger-slate-blue: "#5c8db5"
  ledger-vermillion: "#d9705a"
  ledger-warm-brown: "#8c7b6e"
  ledger-ochre: "#9e8c7e"
  ledger-moss: "#6b9e7e"
  ledger-camel: "#b89a80"
typography:
  display:
    fontFamily: "'Inter', system-ui, -apple-system, sans-serif"
    fontSize: "28px"
    fontWeight: 700
    lineHeight: 1.2
    letterSpacing: "-0.015em"
  title:
    fontFamily: "'Inter', system-ui, -apple-system, sans-serif"
    fontSize: "20px"
    fontWeight: 600
    lineHeight: 1.4
  body:
    fontFamily: "'Inter', system-ui, -apple-system, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: "'Inter', system-ui, -apple-system, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.6
    letterSpacing: "0.04em"
  amount:
    fontFamily: "'JetBrains Mono', 'SF Mono', Consolas, 'Courier New', monospace"
    fontSize: "14px"
    fontWeight: 500
    lineHeight: 1.6
    letterSpacing: "-0.01em"
rounded:
  sm: "6px"
  md: "8px"
  lg: "12px"
  xl: "16px"
  full: "9999px"
spacing:
  2xs: "2px"
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  2xl: "32px"
  3xl: "48px"
components:
  button-primary:
    backgroundColor: "{colors.primary}"
    textColor: "{colors.text-inverse}"
    rounded: "{rounded.md}"
    height: "36px"
    padding: "0 16px"
  button-primary-hover:
    backgroundColor: "{colors.primary-light}"
    textColor: "{colors.text-inverse}"
  button-primary-active:
    backgroundColor: "{colors.primary-active}"
    textColor: "{colors.text-inverse}"
  button-primary-danger:
    backgroundColor: "{colors.expense}"
    textColor: "{colors.text-inverse}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-major}"
    rounded: "{rounded.md}"
    height: "36px"
    padding: "0 16px"
  button-text:
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.md}"
    height: "36px"
    padding: "0 12px"
  input:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.text-major}"
    rounded: "{rounded.md}"
    height: "36px"
    padding: "0 12px"
  card:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.lg}"
    padding: "24px"
  tag:
    backgroundColor: "{colors.income-tint}"
    textColor: "{colors.income}"
    rounded: "{rounded.sm}"
    padding: "2px 8px"
  nav-item:
    textColor: "{colors.text-secondary}"
    rounded: "{rounded.md}"
    padding: "8px 16px"
  nav-item-active:
    backgroundColor: "{colors.active-bg}"
    textColor: "{colors.primary}"
---

# Design System: Transactions

## Overview

**Creative North Star: "The Focus Desk"**

Transactions is a desktop workstation for personal finance. The interface stays clean and airy — cool neutral surfaces, generous white space, and a single blue accent — so the numbers are the loudest thing on screen. Trustworthiness comes from precision: every amount is set in monospace with tabular figures, every layer is separated by a hairline or a tone rather than a heavy shadow, and every interactive element announces itself with a clear focus ring.

The system is refined and restrained by design: components are small, consistent, and quiet, built for dense data work rather than spectacle. Nothing competes with the ledger. Depth is expressed through light layering — surface tone plus 1px borders — with shadows reserved for genuine lift: hovers, popovers, and modals.

**Key Characteristics:**

- One accent voice (Clarity Blue) across actions, focus, and selection.
- Cool neutral hierarchy (Canvas / Paper / Soft Gray) with hairline dividers.
- Inter for UI, JetBrains Mono with tabular-nums for every monetary value.
- 8px-based spacing rhythm; compact 36px controls at the heart of the system.
- Light layering: tone and hairlines first, shadows only where surfaces truly lift.
- Dual theme (light + dark) over the same token names; dark values are tuned for contrast, never re-themed.

## Colors

Cool neutrals carry the interface; Clarity Blue is the single accent; a small semantic set (income/expense/transfer/outlier) exists strictly for transaction and stock data.

### Primary

- **Clarity Blue** (#3964fe): primary buttons, active navigation, links, tab ink, focus rings, selected states. Hover #5b7fff, active #2b52e6, tint rgba(57,100,254,0.08) for active fills and selected rows.

### Neutral

- **Canvas** (#f9fafb): app background behind content.
- **Paper** (#ffffff): cards, surfaces, elevated panels, button faces.
- **Soft Gray** (#f3f4f6): minor surfaces, sidebar fill, table headers, segmented tracks.
- **Ink** (#0f1115): primary text and icons; secondary #61666b, tertiary #81858c, disabled #9aa0a8.
- **Hairlines** (#e8eaed border, #eceef1 divider, #e2e4e8 l2, #d5d8dd l3): all 1px separations.
- **Interactive fills**: hover rgba(15,17,21,0.06), active rgba(57,100,254,0.10).

### Semantic (transaction data only)

- **Income** (#16a34a), **Expense** (#dc2626), **Transfer** (#3b82f6), **Outlier/Warning** (#d97706), each with a 10% tint for tags and chip backgrounds.

### Ledger identity

Eight natural tones give each ledger a stable identity: forest #4a8e70, amber #c6963a, slate-blue #5c8db5, vermillion #d9705a, warm-brown #8c7b6e, ochre #9e8c7e, moss #6b9e7e, camel #b89a80. Dark theme lightens each for legibility.

### Named Rules

**The Semantic Silo Rule.** Income/expense/transfer/outlier colors appear only on transaction or stock data — never on chrome, navigation, or brand surfaces.

**The One Accent Rule.** Clarity Blue is the system's only accent color. There is no secondary accent; emphasis comes from weight and placement.

## Typography

**Display Font:** Inter (system-ui, -apple-system, sans-serif)
**Body Font:** Inter (same stack)
**Label/Mono Font:** JetBrains Mono (SF Mono, Consolas, Courier New)

**Character:** A clean geometric sans — calm, neutral, highly readable at small sizes — paired with a monospace face that makes numbers feel like data, not decoration.

### Hierarchy

- **Display** (700, 28px, 1.2, −0.015em): page titles and key metrics; display-xl 36px for hero figures, display-sm 24px for section-level titles.
- **Title** (600, 20px, 1.4): panel and card titles; title-sm 18px for sub-headers.
- **Section** (500, 16px, 1.6): UI labels and section names.
- **Body** (400, 14px, 1.6): default content and table rows; body-sm 13px for dense lists.
- **Label** (500, 12px, uppercase, +0.04em): table headers, statistic labels, badges; small 11px for micro-labels.
- **Amount** (JetBrains Mono, 500, tabular-nums): every monetary figure; larger sizes use tighter tracking (−0.02em at title, −0.03em at display).

### Named Rules

**The Mono Money Rule.** Every monetary amount renders in JetBrains Mono with `font-variant-numeric: tabular-nums` and slightly negative letter-spacing. Never typeset money in the body face.

## Layout

The spacing scale is 8px-based (2 / 4 / 8 / 12 / 16 / 24 / 32 / 48). The app shell fixes a 48px header, 56px left rail, and 44px footer with 20px content padding. Data surfaces use either auto-filling card grids (`minmax(340px, 1fr)`, 24px gap) or fixed-sidebar layouts (280px rail + fluid main column). Multi-column pages stack to a single column at ≤1080px; wider three-column layouts collapse at ≤1365px.

## Elevation & Depth

Light layering: surfaces separate by tone and 1px hairline borders; shadows are structural cues reserved for genuine lift. Cards rest flat on Paper with shadow-sm and lift to shadow-md on hover. Modals and drawers are the tallest layer (shadow-xl); focus is a 2px Clarity Blue ring.

### Shadow Vocabulary

- **sm** (`0 1px 2px rgba(0,0,0,0.05)`): resting cards, chips, segmented selection.
- **md** (`0 4px 12px rgba(0,0,0,0.08)`): hovered cards, dropdowns.
- **lg** (`0 8px 24px rgba(0,0,0,0.10)`): popovers, messages, notifications.
- **xl** (`0 16px 40px rgba(0,0,0,0.14)`): modals and drawers.
- **focus** (`0 0 0 2px rgba(57,100,254,0.25)`): keyboard focus ring on inputs and controls.

### Named Rules

**The Hairline Layering Rule.** Separate surfaces with tone and a 1px hairline first; add shadows only when a surface actually needs to lift (hover, popover, modal).

## Shapes

Gently curved, consistent radii: 6px for tags and dropdown items, 8px for buttons, inputs, and menus, 12px for cards, popovers, and messages, 16px for modals and drawers, 9999px for switches and pills, and 20px for chat bubbles. Borders are always hairlines — never heavy outlines.

## Components

### Buttons

- **Shape:** 8px radius; heights 28 / 36 / 44px (small / default / large); default min-width 72px.
- **Primary:** solid Clarity Blue, white text, padding 0 16px. Hover → #5b7fff; active → #2b52e6.
- **Secondary (default):** Paper face, 1px hairline border, ink text; hover adds a soft fill (hover-bg).
- **Text:** neutral secondary text, hover fill; text-danger turns Expense on hover with a danger-tint fill.
- **Primary danger:** solid Expense, white text.
- **Focus:** 2px Clarity Blue outline, 2px offset, on `:focus-visible`.

### Cards / Containers

- **Corner Style:** 12px radius.
- **Background:** Paper; borders 1px hairline (window-border).
- **Shadow Strategy:** shadow-sm at rest, shadow-md on hover.
- **Internal Padding:** 24px (space-xl).

### Inputs / Fields

- **Style:** Paper face, 1px hairline border, 8px radius, 36px height, padding 0 12px.
- **Focus:** border turns Clarity Blue with the 2px focus ring.
- **DatePicker / Select:** same height and radius; hover darkens border to l2.

### Navigation

- **Style:** 56px left rail, 18px icons with labels; inactive text-secondary.
- **States:** hover fill (hover-bg); active item gets active-bg tint with Clarity Blue text, 8px radius.

### Tables

- **Header:** Soft Gray, uppercase caption (+0.04em, 600), hairline bottom border.
- **Rows:** 14px body, hairline dividers; hover hover-bg; selected active-bg.

### Tags / Chips

- **Style:** 10% semantic tint background + semantic text, 6px radius, no border, 2px 8px padding.

### Tabs

- **Style:** active tab Clarity Blue 600 with a 2px Clarity Blue ink bar.

### Modals / Drawers

- **Style:** 16px radius, shadow-xl, header/footer hairline-divided, title 18px 600.

### Segmented / Switch

- **Style:** Soft Gray track; selected segment lifts to Paper with shadow-sm and Clarity Blue text; switch is a full-radius pill, Clarity Blue when on.

## Do's and Don'ts

### Do:

- **Do** use the `--transactions-*` tokens for every color, size, and spacing value; never introduce one-off hex values.
- **Do** set every monetary amount in JetBrains Mono with tabular-nums and tight tracking.
- **Do** confine income/expense/transfer/outlier colors to transaction and stock data.
- **Do** layer surfaces with tone and 1px hairlines before reaching for shadows.
- **Do** add a 2px Clarity Blue focus ring to every interactive element.
- **Do** honor `prefers-reduced-motion` and use the project's custom scrollbar mixin.

### Don't:

- **Don't** use browser-default scrollbars or OS-native window chrome (the frameless window keeps its custom title bar).
- **Don't** paint chrome or navigation with income/expense/transfer/outlier colors.
- **Don't** add a second accent color.
- **Don't** use shadows to create hierarchy where a hairline and a tone will do.
