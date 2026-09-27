# 0106: Drop legacy `ncp-ciphers` before starting Windscribe OpenVPN

## Status

Accepted

## Context

The imported Windscribe profile on the affected Windows install declares
`ncp-ciphers AES-256-GCM:AES-256-CBC:AES-128-GCM`. BiFlow already supplies an
AEAD-only `--data-ciphers` list for OpenVPN 2.7, but its sanitized profile
still retained `ncp-ciphers`. OpenVPN then disabled ovpn-dco and fell back to
a TAP adapter. That adapter had no usable OpenVPN gateway, so BiFlow correctly
refused to install a route that would blackhole traffic.

The profile uses TCP/443 and the configured server port is reachable from the
machine. The saved egress probe also confirms an OpenVPN DCO adapter can reach
`1.1.1.1:80`. The failure is in selecting the Windows adapter, before the
application can use the working egress path.

## Decision

- On Windows, remove the obsolete `ncp-ciphers` directive from the temporary
  sanitized profile. The original `.ovpn` remains untouched. Linux keeps the
  imported directive because its cipher arguments do not override it.
- Keep the helper's AEAD-only `--data-ciphers` setting for OpenVPN 2.7.
- Add a regression case matching the imported profile's mixed AEAD/CBC list.

## Consequences

OpenVPN 2.7 can keep ovpn-dco enabled for the imported Windscribe profile and
avoid falling back to TAP because of its legacy cipher list. The helper still
reports any later handshake or egress failure with its specific reason.
