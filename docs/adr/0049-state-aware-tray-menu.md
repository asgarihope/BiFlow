# ADR 0049: State-aware tray menu

## Status

Accepted

## Context

The tray menu listed Connect, Pause, Resume, Disconnect, Open, About, Quit UI,
and Disconnect & Quit at once. Operators could pick both sides of a pair, and
the labels did not follow the live stack phase.

## Decision

- The tray always shows Dashboard first (always enabled), then Connect or
  Disconnect, Pause or Resume, and Quit, with a separator between each item.
- Dashboard shows, unminimizes, and focuses the window, then emits
  `app-navigate` with `"dashboard"` so Basic and Advanced land on the same
  page. Left-click on the icon still only shows the window.
- Labels come from `StackPhase`: a live stack (`running`, `degraded`, `paused`)
  shows Disconnect; only `paused` shows Resume; every other phase shows Connect
  and Pause.
- Rebuild the menu on every stack snapshot so the items change immediately.
- Identify the icon as `main` so the snapshot watcher can replace its menu.

## Consequences

- Open and About remain available from the window; left-click still shows the
  main window.
- Quit releases the owned tunnel, system DNS, and loopback system proxy before
  the process exits, so the machine keeps a working network (ADR 0120).
