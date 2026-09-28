# Application routes in List Management

## Status

Accepted

## Context

Some Windows applications need to use the physical network while BiFlow stays
connected; others need a selected BiFlow client. A hard-coded DIRECT rule for
one executable cannot accommodate both cases or let users choose applications
that are currently running.

## Decision

- List running Windows executable names in List Management and allow search
  and manual refresh. Group duplicate processes by executable name because
  Mihomo routes every process with that name together.
- Let each application use the default BiFlow route, DIRECT, or any enabled
  client. Selecting the default removes its process-specific route.
- Persist application routes in the existing atomic route document. Old
  documents without the new field deserialize as having no application routes.
- Save a selection and apply it to the live Mihomo generation immediately.
  Roll the saved document back if live apply fails.
- Never include executable names in `debug.log`.

## Consequences

- Users can change routing for tools such as `kubectl.exe` without disconnecting.
- A process rule applies to every instance sharing that executable name. It
  routes outbound connections initiated by the process, not child processes.
- Windows discovery uses `tasklist`; other platforms report this feature as
  unsupported for now.
