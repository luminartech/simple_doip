//! `DoIP` tester (client) connection: establishes routing activation with a `DoIP`
//! entity and exchanges diagnostic messages over the resulting TCP connection.

use crate::{
    Error, LogicalAddress, TCP_TIMEOUT_INITIAL_INACTIVITY,
    client_inner::{ControlMessage, Inner},
    connection,
    messages::{
        ActivationTypeCode, MessageError, OwnedMessage, ProtocolVersion, RoutingActivationResponse,
        RoutingActivationResponseCode,
    },
};
use std::{
    net::{IpAddr, SocketAddr},
    string::ToString,
    time::Duration,
    vec::Vec,
};
use tokio::sync::mpsc;
use tracing::{debug, info, trace};

/// Activation options for the routing activation request
///
/// This is used to determine which type of routing activation request to send
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingActivationOptions {
    /// Activation type code
    pub activation_type: ActivationTypeCode,
    /// OEM specific data
    pub oem_specific: Option<[u8; 4]>,
}

/// `DoIP` client options used to specify connection info
/// Derive `Serialize` and `Deserialize` for use in config files
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClientOptions {
    /// Server IP address and port
    pub server_address: SocketAddr,
    /// Target logical addresses, uniquely identifies the ECU to be diagnosed.
    /// Valid range: 0x0001 - 0x0DFF
    pub server_logical_address: LogicalAddress,
    /// (Logical address) Valid range: 0x0001 - 0x0DFF
    pub server_physical_address: LogicalAddress,
    /// Local ip address to bind the TCP and UDP sockets to, e.g. `0.0.0.0`. The port is randomly chosen.
    pub client_address: IpAddr,
    /// Valid range: 0x0E00 - 0x0FFF
    pub client_logical_address: LogicalAddress,
    /// Which protocol version the client should
    pub protocol_version: ProtocolVersion,
    /// The activation type to use when sending the routing activation request
    pub routing_activation_options: Option<RoutingActivationOptions>,
}

/// Selects which of the two target addresses configured on [`ClientOptions`] a
/// diagnostic message should be addressed to.
///
/// This is purely a choice between two configured [`LogicalAddress`] values; the
/// crate does not implement any broadcast or multicast delivery semantics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressType {
    /// Address the message to [`ClientOptions::server_logical_address`].
    Logical,
    /// Address the message to [`ClientOptions::server_physical_address`], the
    /// point-to-point address of a single ECU (range `0x0001`-`0x0DFF`).
    Physical,
}

/// The client is the main entry point for the user to interact with the `DoIP` protocol.
///
/// It handles the connection to the server, and sends and receives messages, silently
/// handling `DoIP` acknowledgements and other protocol details that the user doesn't need to worry about.
#[derive(Debug)]
pub struct Client<Conn = connection::ConnectorSocket> {
    /// The connection configuration this client was created with (server
    /// address, logical/physical addresses, protocol version, routing
    /// activation options).
    pub client_options: ClientOptions,
    /// Sends messages from the user to the inner client
    control_sender: mpsc::Sender<ControlMessage>,
    /// Receives messages from the inner client to the user
    update_receiver: mpsc::Receiver<Result<OwnedMessage, MessageError>>,
    _phantom: std::marker::PhantomData<Conn>,
}

/// Discard update-channel frames left over from a connection that has dropped.
///
/// Returns the number of frames discarded.
///
/// Deliberately **not** `async`: draining must never wait. See
/// [`Client::reconnect`] for why waiting here is actively harmful. Keeping this
/// synchronous means it structurally cannot block on the channel.
///
/// Frames are drained even once the sender has been dropped — `try_recv` yields
/// buffered frames before reporting `Disconnected` — so a closed-but-populated
/// channel is still emptied, and a closed-and-empty one terminates rather than
/// spinning.
fn discard_stale_frames(
    receiver: &mut mpsc::Receiver<Result<OwnedMessage, MessageError>>,
) -> usize {
    let mut stale = 0usize;
    while let Ok(msg) = receiver.try_recv() {
        stale += 1;
        trace!("Discarding stale frame from the previous connection: {msg:?}");
    }
    stale
}

impl<Conn> Client<Conn>
where
    Conn: connection::Connector + 'static + Sync + Send,
{
    /// Create a `DoIP` connection, and automatically send a routing activation request if the client options specify it
    /// The target port defaults to [`crate::TCP_PORT`].
    ///
    /// # Errors
    /// Returns an [`Error`] if the socket cannot be bound or routing activation fails
    pub async fn connect(client_options: ClientOptions) -> Result<Self, Error> {
        let (control_sender, update_receiver) = Inner::<Conn>::spawn(client_options);
        Self::bind_socket(&control_sender, &client_options).await?;

        Ok(Self {
            client_options,
            control_sender,
            update_receiver,
            _phantom: std::marker::PhantomData,
        })
    }

    /// Bind the socket to a local address and port.
    ///
    /// * Standard ISO-13400 clients will bind to the local address and port of the server
    /// * See [`Inner::bind_socket`] for more details
    async fn bind_socket(
        control_sender: &mpsc::Sender<ControlMessage>,
        client_options: &ClientOptions,
    ) -> Result<u16, Error> {
        let (response, message) =
            ControlMessage::create_bind_socket_message(client_options.server_address);
        control_sender.send(message).await.map_err(|_| {
            Error::BindFailed("Could not send BindSocket message to inner client".into())
        })?;
        let port = response
            .await
            .map_err(|_| Error::BindFailed("Connection task terminated unexpectedly".into()))?;

        // Automatically send a routing activation request if the client options specify it
        'routing: {
            if let Some(routing_activation_options) = client_options.routing_activation_options {
                let message = OwnedMessage::routing_activation_request(
                    client_options.protocol_version,
                    client_options.client_logical_address,
                    routing_activation_options.activation_type,
                    routing_activation_options.oem_specific,
                );

                // Send the message and wait for a response
                let (response, message) =
                    ControlMessage::create_routing_activation_message(&message);
                control_sender
                    .send(message)
                    .await
                    .map_err(|_| Error::RoutingActivationFailed)
                    .inspect_err(|e| debug!("Failed to send routing activation request: {e}"))
                    .inspect(|()| trace!("Routing activation request sent successfully"))?;
                let res = tokio::time::timeout(TCP_TIMEOUT_INITIAL_INACTIVITY, response).await;
                // Elapsed error handling
                let Ok(res) = res else {
                    tracing::warn!(
                        "Timeout waiting for routing activation response. Server may not support routing activation."
                    );
                    break 'routing;
                };
                let Ok(res) = res else {
                    tracing::warn!(
                        "Routing activation response channel closed. Server may not support routing activation."
                    );
                    break 'routing;
                };
                // if the timeout specifically and keep working, the routing activation may not be supported
                debug!("Routing Activation Response received: {:?}", res);
                match res {
                    Ok(OwnedMessage { payload, header }) => {
                        let crate::messages::OwnedPayload::RoutingActivationResponse(
                            RoutingActivationResponse {
                                logical_address_tester,
                                logical_address_of_doip_entity,
                                routing_activation_response_code,
                                reserved_oem,
                                oem_specific,
                            },
                        ) = payload
                        else {
                            // Unreachable in practice: client_inner only forwards a
                            // response whose payload type satisfies `is_response`, which
                            // for a routing activation request means exactly
                            // RoutingActivationResponse. Kept as defence in depth so a
                            // future refactor degrades to an error, not a panic.
                            return Err(Error::UnexpectedMessageType(header.payload_type));
                        };
                        info!("Routing Activation Response received:");
                        info!("  Logical Address Tester: {:04X}", logical_address_tester.0);
                        info!(
                            "  Logical Address of DoIP Entity: {:04X}",
                            logical_address_of_doip_entity.0
                        );
                        info!(
                            "  Routing Activation Response Code: {:?}",
                            routing_activation_response_code
                        );
                        info!("  Reserved OEM: {:02X?}", reserved_oem);
                        if let Some(oem) = oem_specific {
                            info!("  OEM Specific: {:02X?}", oem);
                        }

                        // Only the two success codes may proceed; every other
                        // code is a denial (or reserved) and must surface as an
                        // error. Previously the code was merely logged and
                        // `bind_socket` returned `Ok`, so a denial such as
                        // `DeniedSourceAddressAlreadyRegistered` (another tester
                        // already holds our source address) was invisible: the
                        // entity then closed the socket and the caller treated
                        // the reset as transient, reconnecting forever.
                        match routing_activation_response_code {
                            RoutingActivationResponseCode::RoutingSuccessfullyActivated
                            | RoutingActivationResponseCode::RoutingSuccessfullyActivatedConfirmationRequired => {
                            }
                            denied => {
                                return Err(Error::RoutingActivationDenied(denied));
                            }
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
        }
        port
    }

    /// Unbind the socket from the local address and port
    ///
    /// # Panics
    /// Panics if the control message cannot be sent or the response channel is closed
    ///
    /// # Errors
    /// Returns an [`Error`] if the socket is not currently bound
    pub async fn unbind_socket(&mut self) -> Result<(), Error> {
        let (response, message) = ControlMessage::create_unbind_socket_message();
        self.control_sender.send(message).await.unwrap();
        response.await.unwrap()
    }

    /// Re-bind the socket (and re-send routing activation) after a connection loss.
    ///
    /// Returns as soon as the new connection is established. The returned
    /// `Option` is always `None`; see below.
    ///
    /// # Why this does not wait for an in-flight response
    ///
    /// This used to block for a fixed 5 seconds on the update channel, hoping
    /// to surface a response that was in flight when the link dropped. That
    /// wait could never do its job:
    ///
    /// * A genuine in-flight `DiagnosticMessage` never reaches the update
    ///   channel. `Inner` buffers it into `pending_diagnostic_response` and
    ///   hands it to the next `ReceiveDiagnosticResponse`, so the caller's own
    ///   response wait already collects it.
    /// * The only frames that *can* reach the update channel are non-diagnostic
    ///   control frames, and callers uniformly reject those (`uds_on_ip` fails
    ///   with "Expected DiagnosticMessage, got ..."). Returning one is worse
    ///   than returning nothing.
    ///
    /// So the wait's only reachable outcome was to burn 5 seconds and return
    /// `None`. That is fatal here: 5 seconds is the ISO 14229 default S3
    /// session timer, so a reconnect mid-sequence would routinely let the ECU
    /// fall back to the default session before the tester could speak again.
    ///
    /// Returning immediately with `None` is both faster and more correct — it
    /// is the path callers already handle by re-sending the request.
    ///
    /// The channel is still drained non-blockingly: it is bounded, and stale
    /// frames from the dead connection would otherwise accumulate and
    /// eventually stall `Inner`'s send.
    ///
    /// # Errors
    /// Returns an [`Error`] if the socket cannot be re-bound
    pub async fn reconnect(&mut self) -> Result<Option<OwnedMessage>, Error> {
        let _ = Self::bind_socket(&self.control_sender, &self.client_options).await?;

        let stale = discard_stale_frames(&mut self.update_receiver);
        if stale > 0 {
            debug!("Reconnected, discarded {stale} stale frame(s)");
        } else {
            trace!("Reconnected, no stale frames pending");
        }

        Ok(None)
    }

    /// Shut down the client, closing the connection and cleaning up resources
    pub async fn shut_down(self) {
        let Self {
            control_sender,
            mut update_receiver,
            ..
        } = self;
        drop(control_sender);
        debug!("Shutting Down DOIP client");
        while update_receiver.recv().await.is_some() {
            info!(".");
        }
    }

    /// Send a diagnostic message and wait for DoIP-level ACK only.
    ///
    /// Does NOT wait for the diagnostic response - use `receive_diagnostic_response()` for that.
    /// This is the primary send method for UDS communication, allowing proper timeout handling
    /// at the UDS layer (including NRC 0x78 Response Pending scenarios).
    ///
    /// # Errors
    /// Returns an [`Error`] if the socket is not bound, the message cannot be sent,
    /// or a negative ACK is received.
    pub async fn send_diagnostic_message(
        &mut self,
        address_type: AddressType,
        user_data: Vec<u8>,
    ) -> Result<(), Error> {
        let message = OwnedMessage::diagnostic_message(
            self.client_options.protocol_version,
            self.client_options.client_logical_address,
            match address_type {
                AddressType::Logical => self.client_options.server_logical_address,
                AddressType::Physical => self.client_options.server_physical_address,
            },
            user_data,
        );

        let (response, ctrl_msg) = ControlMessage::create_send_diagnostic_message(message);
        self.control_sender
            .send(ctrl_msg)
            .await
            .map_err(|e| Error::SendError(e.to_string()))?;
        response.await.map_err(|_| Error::ConnectionClosed)?
    }

    /// Wait for the next diagnostic message response.
    ///
    /// Call this after `send_diagnostic_message_only()` to receive the response.
    /// Can be called multiple times to handle NRC 0x78 (Response Pending) scenarios
    /// where the server needs more time to process the request.
    ///
    /// # Errors
    /// Returns an [`Error`] if the socket is not bound, the response times out,
    /// or the connection is closed.
    pub async fn receive_diagnostic_response(
        &mut self,
        timeout: Duration,
    ) -> Result<OwnedMessage, Error> {
        let (response, ctrl_msg) = ControlMessage::create_receive_diagnostic_response(timeout);
        self.control_sender
            .send(ctrl_msg)
            .await
            .map_err(|e| Error::SendError(e.to_string()))?;
        response.await.map_err(|_| Error::ConnectionClosed)?
    }
}

#[cfg(test)]
mod tests {
    use super::discard_stale_frames;
    use crate::messages::{MessageError, OwnedMessage};
    use tokio::sync::mpsc;

    type Frame = Result<OwnedMessage, MessageError>;

    fn channel() -> (mpsc::Sender<Frame>, mpsc::Receiver<Frame>) {
        mpsc::channel(16)
    }

    #[test]
    fn empty_channel_discards_nothing() {
        let (_sender, mut receiver) = channel();
        assert_eq!(discard_stale_frames(&mut receiver), 0);
    }

    #[tokio::test]
    async fn queued_frames_are_all_discarded_and_channel_left_empty() {
        let (sender, mut receiver) = channel();
        for _ in 0..3 {
            sender.send(Ok(OwnedMessage::default())).await.unwrap();
        }

        assert_eq!(discard_stale_frames(&mut receiver), 3);
        // Nothing left behind for a later receive to mistake for a live response.
        assert_eq!(discard_stale_frames(&mut receiver), 0);
    }

    #[tokio::test]
    async fn frames_are_drained_even_after_the_sender_is_dropped() {
        // `reconnect()` runs after a connection has died, so the inner task may
        // already be gone. Buffered frames must still be cleared rather than
        // the drain bailing out on the first `Disconnected`.
        let (sender, mut receiver) = channel();
        sender.send(Ok(OwnedMessage::default())).await.unwrap();
        sender.send(Ok(OwnedMessage::default())).await.unwrap();
        drop(sender);

        assert_eq!(discard_stale_frames(&mut receiver), 2);
    }

    #[test]
    fn closed_empty_channel_terminates() {
        // Guards against rewriting the drain in terms of a blocking `recv`,
        // which would hang (or spin) instead of returning here.
        let (sender, mut receiver) = channel();
        drop(sender);
        assert_eq!(discard_stale_frames(&mut receiver), 0);
    }
}
