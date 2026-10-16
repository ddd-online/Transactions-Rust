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
  chart-blue: "#3964fe"
  chart-amber: "#d97706"
  chart-green: "#15803d"
  chart-pink: "#db2777"
  chart-cyan: "#0891b2"
  chart-red: "#dc2626"
  chart-violet: "#7e22ce"
  chart-neutral: "#61666b"
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

### Chart categorical palette

折线图的候选色池。曲线上色**按它在图表里的位置**（第 N 条 → 第 N 色），与交易类型无关：
一张图里同类型的曲线常常不止一条（预设的「年度收入趋势」就是 4 条收入曲线），
按类型取语义色时第 4 条只能落到别的语义色上。两套主题色相一一对应，切换主题不改变曲线身份。

| # | Token | 浅色 | 深色 | 色相 |
|---|-------|------|------|------|
| 1 | `--transactions-color-chart-blue` | `#3964fe` | `#60a5fa` | 蓝（品牌色） |
| 2 | `--transactions-color-chart-amber` | `#d97706` | `#f59e0b` | 橙 |
| 3 | `--transactions-color-chart-green` | `#15803d` | `#4ade80` | 绿 |
| 4 | `--transactions-color-chart-pink` | `#db2777` | `#ec4899` | 粉 |
| 5 | `--transactions-color-chart-cyan` | `#0891b2` | `#22d3ee` | 青 |
| 6 | `--transactions-color-chart-red` | `#dc2626` | `#ef4444` | 红 |
| 7 | `--transactions-color-chart-violet` | `#7e22ce` | `#a855f7` | 紫 |
| 8 | `--transactions-color-chart-neutral` | `#61666b` | `#b6bcc6` | 灰（第 8 条起） |

顺序不是随手排的：它让**曲线少的时候最容易分**（两两 ΔE76 的下界：2 条 ≥126、3 条 ≥86、
4 条 ≥77、5 条 ≥44；到第 7 条才降到池内的最差值 31）。
灰留在最后 —— 只有第 8 条才用得着它；曲线多于 8 条时按 8 取模。
池内两两可辨（ΔE76 正常视觉 ≥25、红绿色盲 ≥8）与画布对比度 ≥3:1 是硬要求，
判据与实测值在 `fixtures/design-audit.ps1` 第 4 项。

### Named Rules

**The Semantic Silo Rule.** Income/expense/transfer/outlier colors appear only on transaction or stock data — never on chrome, navigation, or brand surfaces.

**The Positional Colour Rule.** Multi-series charts take their colours from the chart categorical palette by the series' position in the list, not from the semantic set. The plot, the legend, and the totals column all use the same index, so a curve's colour is identical in all three.

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

**The Panel Measure Rule.** Body text inside a panel fills that panel: 复盘 and note fields are read as itemised lines ("买入理由：…"), so they keep the width of the table above them — no `ch` cap, which in a panel merely shortens the wrap point and reads as content that failed to fill. Only continuous long-form prose caps its measure (Markdown bodies, 75ch in `key_event.css`).

### Tracking tokens

Letter-spacing is tokenised per role — `--transactions-tracking-display` (−0.015em, display/title text),
`--transactions-tracking-label` (+0.04em, uppercase labels), and the amount ladder
`--transactions-tracking-amount` (−0.01em) / `-amount-lg` (−0.02em, money at title size) /
`-amount-xl` (−0.03em, money at display size). Prose takes no tracking. Line heights are tokens too:
`--transactions-height-tight` (display 1.2) / `-snug` (title 1.4) / `-normal` (body, label 1.6) /
`-relaxed` (long-form prose 1.8).

## Layout

The spacing scale is 8px-based (2 / 4 / 8 / 12 / 16 / 24 / 32 / 48). The app shell fixes a 48px top band, a 200px navigation sidebar, a 56px sub-function rail, and a 44px footer; page content insets are 16px (full-bleed tables take 0 and pad their own cells). Data surfaces are fixed-sidebar layouts (`280px + minmax(0, 1fr)`；事件/日记的两侧轨用 `clamp()` 随窗口收放) or single-column stacks；设置页的字段组用 `repeat(auto-fit, minmax(200px, 1fr))`。多栏页面在 ≤1080px 收成一栏，个别三栏更早先收一档（≤1280 / ≤1200）；共享组件与记账/待办的窄窗兜底在 ≤820px / ≤720px。The window has no minimum size — the breakpoints above carry narrow widths, so they must stay live.

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

## Motion

Motion here is structural, not decorative: it says where a surface came from, that a change landed, or that a press was received. One curve carries all of it — exponential ease-out (`cubic-bezier(0.16, 1, 0.3, 1)`, `--transactions-ease-out-expo`), never bounce or elastic, and exits are instant (nothing waits for a closing animation).

| Beat | Applies to | Travel | Duration |
|---|---|---|---|
| **Enter** | panels and messages: modal, drawer, select / popconfirm / date-time panels, notices | arrives from 4px above (drawer 24px from the side) | 200ms (`--transactions-transition-enter`) |
| **Settle** | the whole content area when a page or sub-function lands — once per mount, never per section | arrives from 8px above | 320ms (`--transactions-transition-settle`) |
| **Draw the data** | the self-drawn chart's line and area draw (charts-rs `AnimationConfig`, 620ms + 60ms delay) | — | the surface's one authored moment |

**The Two Kinds of Arrival.** Masks and backdrops only fade (`tr-fade-in`, opacity alone) — a full-screen layer that moves reads as the window moving. Panels and messages arrive from 4px above (`tr-rise-in`); a whole content area settles from 8px above (`tr-settle`). Both keyframes end at their natural state and use no fill-mode, so content is visible even if the animation never runs. Arrival is always **from above or from outside**: entering elements are routinely exactly as tall as their container, so travelling down would add scrollable height to an `overflow: auto` parent and flash a scrollbar on the right edge.

**The Press Rule.** Desktop clicks have no native press feedback, so buttons, icon buttons and both navigations sink 1px within the 150ms feedback band (`--transactions-transition-fast`). Hover, selected, and switch-handle transitions keep their existing timings. Nothing animates a layout-driving property; the documented exceptions are the two **collapses** that own a column of their own — the sub-function rail's and the sidebar's (`grid-template-columns`, three tracks and two tracks respectively) — and the progress bar's width.

**The Sidebar's Hand-Over.** The sidebar's two states arrange the same seven entries differently (labels beside the icons ↔ under them) and at two different row heights, and neither `flex-direction` nor a row's height may be transitioned. So the beat is: the column's track animates on the settle curve while the **entry contents hand over** — icon and label step aside for the first 25% (every discrete change, including the row's height, happens while they are invisible) and settle back in from 4px above (`tr-sidebar-hand-over`, 320ms). What does not hand over is the structure (a cell's fill, the selected state) and the workspace mark in the top band: it is the identity of the surface, and blinking it costs more than blinking a nav item. The class that triggers the beat is set by the shell on the click and removed 40ms after the animation ends — it never runs on mount, and it never repeats per page.

**Budget.** Only transform, opacity, color and shadow animate; no scroll-driven motion, no parallax, no looping animation outside loading states, and loaders stop when their surface unmounts.

**Reduced motion.** `prefers-reduced-motion: reduce` collapses every beat to an instant state change (the global cap in `base.css`), so focus, colour and state still land — and the chart's dash-offset draw is explicitly reset so a line can never stay hidden.

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
- **Rows of fields:** size the **fields**, not the row. `.ui-input` is `width: 100%` and an `<input>`'s intrinsic width (its default 20-character size, ≈181px here) is that field's *automatic minimum*, so "give the row a width and let the fields shrink" cannot work: a fixed 260px row of two month fields overflowed and the next control painted over the second one, leaving 「结束」 visible. Each field takes `flex: 0 0 <n>px` **plus `min-width: 0`** (without it the automatic minimum wins and the basis is ignored) and the row sizes to its content — 168px holds 「开始月份 YYYY-MM」 with its 12px padding on both ends.
- **Writing surface (prose, not a field):** a paragraph-length note gets its own paper — `ModalSize::Large` (640) with a body ≥320px tall, growing with its content up to 60vh. **At rest it must never clip:** the shell runs WebView2's *overlay* scrollbars (see the scrollbar note in `base.css`), so content taller than its box reads as content that is gone. 320px is the arithmetic, not a taste: 640 − 48 padding = 592px of measure, ~42 CJK glyphs per line at 14px, 1.8 line-height → 500 characters ≈ 12 lines ≈ 302px. Ship a live character count (`tr_domain::util::char_count`, the same ruling as the server), `Ctrl+S` to save, `Esc` to leave, and focus plus caret placed on open — a writer should never click into the field they just asked for.

### Navigation

- **Style:** 56px left rail, 18px icons with labels; inactive text-secondary.
- **States:** hover fill (hover-bg); active item gets active-bg tint with Clarity Blue text, 8px radius.

### Tables

- **Header:** Soft Gray, uppercase caption (+0.04em, 600), hairline bottom border.
- **Rows:** 14px body, hairline dividers; hover hover-bg; selected active-bg.

### Tags / Chips

- **Style:** 10% semantic tint background + semantic text, 6px radius, no border, 2px 8px padding.
- **Editable tag (chip button):** when a tag is something the user can change, it keeps the tag face (caption, 6px radius, `minor-background`, 2px 8px) and gains an 11px caret plus a 22px minimum height so it is a fair pointer target — a bordered 36px field parked in a title row reads as an unfilled input, not as that row's tag (`ui-select--tag`). Both places a round's tag can be changed wear the same chip: the position detail header and each round's header on 记录 — in the latter the chip rides the right end of its row, so its panel opens leftward (`right: 0`) rather than past the window edge. Hover steps the fill to `active-bg` with primary text; open and keyboard focus add the focus ring. Its panel is sized to the widest option, so a long custom tag never wraps inside a narrow chip.

### Tabs

- **Style:** active tab is Clarity Blue text on an `active-bg` tint fill (8px radius, medium weight) — `.ui-tabs__tab.is-active`.

### Modals / Drawers

- **Style:** 16px radius, shadow-xl, header/footer hairline-divided, title 18px 600.
- **Width — three tiers only:** Small 400, Medium 520, Large 640 (`ModalSize` in `components/ui/modal.rs`, mirrored by `.ui-modal__content--sm|md|lg`). Pick the tier from the content, never a one-off pixel width: Small for confirmations and single-field forms, Medium for short forms and pickers, Large for multi-field forms and filter builders. Omitting the prop means Medium. Every modal keeps `max-width: 100%` so a narrow window shrinks it instead of pushing it off-screen.

### Blocking screens

- **The one blocking screen is the workspace picker** (no workspace open yet). It keeps the **whole app shell rendered behind it** — sidebar, page frame and empty states — so the user sees what they are configuring, and covers it with `--transactions-color-screen-scrim` (heavier than a modal mask, lighter than the lightbox scrim). That background layer is `inert`: no pointer, no keyboard focus, nothing in the accessibility tree; only the dialog is reachable. The screen has no dismiss affordance at all — no ×, no cancel, clicking the backdrop does nothing, and the shell refuses the window close request — the user must pick a directory (escape hatch: the tray menu's quit item). Everything else in the copy goes into the ⓘ tooltip: the title and one sentence are the whole text.

### Help tooltips

- **Trigger:** a 12px `Icon::InfoCircle` placed immediately after the label it explains — `--transactions-color-text-tertiary`, `cursor: help`, the same glyph the settings panels use for their fee notes. It is focusable, so the component's own `:focus-within` rule shows the bubble to keyboard users too; give it the standard 2px focus ring.
- **Bubble:** the `Tooltip` component's CSS bubble, holding one sentence in the reader's own words. The wording lives next to the vocabulary it explains (`Metric::hint` in tr-draw), not in the page, so a renamed label and its explanation move together. No formulas here — the number's own sub-line carries the arithmetic.
- **Width:** cap it from the trigger's own class (180–320px) whenever the trigger sits inside a grid, table or panel: `overflow: hidden` ancestors clip an over-wide bubble mid-sentence, and a truncated definition reads as a wrong one.

### Segmented / Switch

- **Style:** Soft Gray track; selected segment lifts to Paper with shadow-sm and Clarity Blue text; switch is a full-radius pill, Clarity Blue when on.

## Do's and Don'ts

### Do:

- **Do** use the `--transactions-*` tokens for every color, size, and spacing value; never introduce one-off hex values.
- **Do** set every monetary amount in JetBrains Mono with tabular-nums and tight tracking.
- **Do** confine income/expense/transfer/outlier colors to transaction and stock data.
- **Do** layer surfaces with tone and 1px hairlines before reaching for shadows.
- **Do** add a 2px Clarity Blue focus ring to every interactive element.
- **Do** honor `prefers-reduced-motion`（全局兜底在 `base.css`）。
- **Do** leave scrollbars alone：全站是 WebView2 的**覆盖式**滚动条（`shell.rs` 的 WEBVIEW_ARGS），配色跟随 `color-scheme`。

### Don't:

- **Don't** draw your own scrollbars（`::-webkit-scrollbar` / `scrollbar-width` 会让 Blink 退回"占宽度"那条老路，`fixtures/static-guards.ps1` 盯着），也别用 OS-native window chrome（the frameless window keeps its custom title bar）。
- **Don't** paint chrome or navigation with income/expense/transfer/outlier colors.
- **Don't** add a second accent color.
- **Don't** use shadows to create hierarchy where a hairline and a tone will do.
