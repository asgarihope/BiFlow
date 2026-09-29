# BiFlow UI design system

Rules the UI must keep. Every UI change is checked against this file; when a
rule has to change, update this file in the same commit.

## Information architecture

- Five sections, same order in the sidebar and the mobile bottom nav
  (`lib/navigation.ts`): **Home**, **Routing**, **Clients**,
  **Troubleshoot**, **Settings**. About is the last Settings tab, not a
  section. The Basic/Advanced switch, theme, and language sit at the bottom
  of the sidebar (top bar in Basic and on mobile), never above page content.
- Pages with more than one concern split into tabs (`ui/Tabs.tsx`
  `TabBar` + `TabPanel`), and the store remembers the last tab per page
  (`pageTabs`, `setPage(page, tab)`):
  - Routing: Sites / Lists / Apps / Iran rules
  - Troubleshoot: Live / Test / Tools / Logs
  - Settings: Network / Behavior / About
- Every page starts with `PageHeader`: one-line title, optional info tip,
  and primary action on the same row.

## Copy

- No explanatory paragraph under a heading. Explanations go in an
  `InfoTip` (icon, hover/focus tooltip). Buttons are verb-first, 1–3 words.
- Confirmations are a short toast (`showToast`, `ui/Toast.tsx`), not an
  inline paragraph. The toast uses `aria-live`, not `role="status"`.

## Home

- Order: connection hero (status orb, title, "Iran direct · rest via
  [default route]" select, lifecycle buttons), the add-site bar, three
  facts (Exit IP, Rule sets, Health), then the live diagram beside "Your
  sites" (`lg:grid-cols-5`, 3 + 2) while connected, or "Your sites" alone.
- Component state is one Health tile: five dots and one sentence. Its rows
  (status, message, Install / View config) open below the facts as a
  full-width row; they open by themselves only when a component is in
  error or unavailable.
- The add-site bar (`AddSiteBar`) is on Home, Basic, and Routing › Sites:
  paste anything, `extractHost` reduces it, a segmented Direct/client
  choice (a select past three options) defaults to the default client.

## Motion

- Pages and tab panels ease in with `.ui-enter` (180 ms), the toast with
  `.ui-toast`, and the live status orb breathes (`.status-orb-live`). The
  global reduced-motion block disables all of it.

## Density and rhythm

- Cards: `rounded-2xl border border-ink/10 bg-surface p-3.5` (inner tiles may
  use `p-3`). Page sections use `flex flex-col gap-3 pb-2`.
- No oversized hero paddings (`p-5`+) on regular cards; compact is the
  default everywhere.

## Cards with actions → footer pattern

- A card whose primary interaction is a button (Diagnostics tiles: Fresh
  Hiddify start, Permanent debug.log, Support bundle) puts every button in a
  **footer**: `mt-auto flex flex-wrap items-center gap-2 border-t
border-ink/10 pt-3`, with small buttons
  (`rounded-lg px-3 py-1.5 text-xs font-semibold`, icon `size={14}`).
- Tile siblings in one grid row must be equal height: the grid stretches
  children (`[&>div]:flex [&>div]:h-full [&>div]:flex-col` or per-card
  `flex h-full flex-col`) and `mt-auto` pins footers to the bottom, so
  footers align across the row.

## Section order and tiling

- Tabs are ordered by how often they are used: Troubleshoot opens on Live
  (live connections), then Test (test flow, Reachability + Test timeline),
  Tools (client egress, Fresh Hiddify start, debug.log, support bundle),
  and Logs. The egress probe says whether the adapter Mihomo bound can
  deliver a packet, and why it cannot.
- Small independent utilities tile side-by-side (Tools is a
  `md:grid-cols-2` grid) instead of stacking full-width.

## Lists (Routing › Lists)

- Rule lists render as **full-width horizontal rows stacked vertically**
  (`flex flex-col gap-2`), one bar per list: name (inline-editable), entry
  count, Send-through select, Check, add-entry form, delete icon; entries as
  removable chips below the bar. Never a multi-column card grid here.

## Client cards (Clients page)

- Compact by default: header (title, default badge, status pill, Enabled),
  one summary line (`N domains · M IPs · Local port P · Exit IP x.x.x.x`),
  optional warning banner (e.g. OpenVPN missing + platform download link).
- Everything else lives in a collapsible `<details>` labelled
  "Settings & pinned hosts".

## Colors

- Per-client accent colors come from `CLIENT_COLORS` / `clientColor()` in
  `apps/desktop/src/lib/outbound.ts` — the single source used by the live
  traffic diagram and the live-connections badges. DIRECT is always green.

## Live traffic diagram

- One branch per enabled client plus DIRECT; packets are rAF-driven along
  the measured path (SMIL is unreliable in the webview), labels alternate
  above/below the dot, births are staggered, and opacity fades in/out.
  Nodes are keyboard-accessible buttons with a status tooltip (client exit
  IP; DIRECT shows the real public IP).

## Chrome

- The settings-apply banner is `sticky top-0 z-40` inside the scroll
  container with `backdrop-blur` so it stays visible while scrolled.
- Scrollbars are themed globally (thin, `rgb(var(--ink) / 0.22)` thumb,
  transparent track); the OS default scrollbar must never appear.
- App boot renders `PageSkeleton` — a per-page structured skeleton, not a
  centered spinner.
- Pause/Disconnect (and Cancel) stay on one row on `sm+`
  (`sm:flex-nowrap`) and stack full-width below `sm`.
- Connection lifecycle buttons (`ConnectionActionButton`, cancel) use a fixed
  `h-14` height, `whitespace-nowrap`, and an invisible reserve label sized to
  the longest EN/FA idle or stage string so labels never wrap and the control
  row does not shift while Connect/Pause/Resume/Disconnect progress runs.

## Live Mihomo

- The hero states which outbound the live `MATCH` rule is using
  (`data-testid="live-match"`). The Mihomo row in Health opens that config
  read-only, with the controller secret removed. The view is not an editor.
- A client card shows “Default for unmatched” only when Mihomo’s live `MATCH`
  is that client. If the saved choice is unused, or the client is stopped or
  in error, the card is muted and the reason is written on it. A stopped
  client is also disabled in the default-route list.

## Accessibility guardrails

- Never use `sr-only` labels for inputs inside scrollable pages (they anchor
  to the page and break the no-document-overflow e2e); use `aria-label`.
- Interactive SVG nodes get `role="button"`, `tabIndex`, and Enter/Space
  handling.
- Both languages must pass the responsive e2e (no horizontal overflow at
  390px; tables scroll inside their own container).

- In RTL, select chevrons move to the left (`[dir="rtl"] select` in
  `index.css`); the forms plugin otherwise draws them over the text.
