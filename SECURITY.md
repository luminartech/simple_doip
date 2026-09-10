# Security policy

## Reporting a vulnerability

Report security issues through GitHub's private vulnerability reporting: open
the [Security tab](https://github.com/luminartech/simple_doip/security) and
choose **Report a vulnerability**. That opens a private advisory visible only
to the maintainers.

Please do not open a public issue for a security report.

A report is most useful with the crate version, the feature set enabled, and a
byte sequence or test case that reproduces the behavior.

## Supported versions

This crate is pre-1.0. Fixes land on the latest published version; there are no
maintained release branches.

## Scope

`simple_doip` implements the ISO 13400-2 transport. Two properties of that
protocol matter when assessing a report, because they are the specification's
design rather than defects in this crate:

- **DoIP carries no transport security.** Connections on `TCP_PORT` (`13400`)
  are in the clear. `TCP_TLS_PORT` (`3496`) is defined by ISO 13400-2, and
  nothing in this crate uses it.
- **Routing activation is not authentication.** It is an addressing handshake.
  Any access control over a diagnostic session belongs to the layers above
  this one.

What is in scope: anything that makes the crate misbehave on attacker-supplied
bytes — a panic, an out-of-bounds read, an unbounded allocation, a decode that
accepts a frame it should reject, or a hang reachable from the wire.
