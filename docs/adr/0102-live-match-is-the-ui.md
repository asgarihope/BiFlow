# 0102: The UI shows the live MATCH rule

## Status

Accepted

## Context

Choosing Windscribe as “Default for everything else” saved that choice and
reported a successful reload, while Mihomo’s live `MATCH` rule stayed on
Hiddify. The client list and the traffic diagram then described the saved
setting, so the screen the operator trusts disagreed with the packets.

A reload that returns success without changing `MATCH` is the same failure
as a rejected reload. An empty controller path answers HTTP 204 and leaves
the previous rule loaded.

## Decision

- After every hot reload, read `GET /rules`. If the live `MATCH` proxy is not
  the proxy in the config that was just sent, the apply fails and says which
  proxy Mihomo kept.
- Before that reload, when the saved default is a side tunnel, the helper
  installs the adapter route Mihomo binds with `interface-name`. A default
  side tunnel that has no adapter fails the reload instead of sending
  unmatched traffic to a closed placeholder port.
- Health refreshes record that live proxy. While the stack is running, up to
  three refreshes re-apply routing when it disagrees with the saved default.
- The Mihomo card names the live outbound and opens the running config
  read-only. The controller secret is replaced before the text reaches the UI.
- “Default for unmatched” is drawn only for the outbound Mihomo is using. A
  saved client that Mihomo is not using, or a client that is stopped or in
  error, is muted in the list with the reason. A stopped client cannot be
  selected as the default.

## Consequences

- A default-route change that does not stick is visible on the card and in
  `debug.log` (`mihomo.live_match_disagreed`), and the stack tries again.
- The operator can read the loaded config without a way to edit it in place.
