# Changelog

All notable changes to this crate are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
this crate adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While the crate is at `0.x`, the minor position carries breaking changes.

## 0.4.0

The release that makes `simple_doip::server` able to drive a real UDS tester: a
handler can now emit the `DiagnosticMessageAck` and the UDS response as separate
messages, hold an NRC `0x78` pending wait open, serve on a caller-chosen socket,
and answer UDP discovery.

### Breaking

- `ServerConnectionHandler::diagnostic_message` now takes a
  `responses: &mut dyn ResponseWriter` and returns `Result<(), Error>` instead of
  returning a single `OwnedMessage`. Handlers write the `DiagnosticMessageAck`
  and the UDS response separately. Migration: what was `Ok(message)` becomes
  `responses.send(message).await?; Ok(())`, with an ack sent first — a tester
  blocks waiting for that ack, which is why the previous single-message signature
  could not drive one. See `examples/echo_server.rs` for the ack-then-response
  shape.

### Added

- `ResponseWriter` — the sink handlers write responses into. Each send reaches
  the socket immediately, so a handler can hold an NRC `0x78` pending wait open
  across awaits.
- `Server::run_server_with_listener` — serve a `TcpListener` the caller bound,
  for loopback aliases and ephemeral ports. `run_server` keeps its signature and
  delegates to it.
- `Server::run_udp_responder` — answer UDP vehicle-identification probes on a
  caller-bound `UdpSocket`. Note that `run_server` still binds TCP only: an
  entity that wants to be discoverable must drive both, via `join!`/`select!` or
  a second task.
- `OwnedMessage::vehicle_identification_response`.

### Fixed

- The TCP accept loop no longer panics on an accept error; it logs and
  continues.
- `TCP_NODELAY` is now set on accepted connections. `ConnectorSocket` already
  set it client-side, but an accepted socket did not, so consecutive small
  frames — an ack then a response, or successive NRC `0x78` pendings — waited
  on the peer's delayed ACK. Measured at 43 ms of added latency per exchange,
  straight out of the P2 budget.
- Every failure inside `run_udp_responder` is logged and skipped rather than
  ending the loop. A fatal path here would be reachable by any host on the
  network: on Windows, where an oversized datagram fails `recvfrom` with
  `WSAEMSGSIZE` instead of truncating, a single 2 KB packet could otherwise end
  discovery permanently. A failing identification handler could do the same.

### Changed

- `run_udp_responder` answers only the broadcast vehicle-identification request
  (`0x0001`). The directed forms — `0x0002` naming an EID and `0x0003` naming a
  VIN — are declined, because `Payload::decode` discards the EID/VIN bytes and
  the responder cannot tell whether it is the addressee; answering regardless
  would mean every entity on a network replied to a tester's directed probe.
  Declining degrades to a discovery timeout, which testers already handle. A
  correct implementation requires `Payload` to preserve those bytes, and the
  `ServerConnectionHandler::vehicle_identification_with_eid` /
  `vehicle_identification_with_vin` hooks remain unconsulted until it does.
