//! User → Client → `control_sender` → Inner → SocketManager.sender → TCP Socket → Server
//! User ← Client ← `update_receiver` ← Inner ← SocketManager.receiver ← TCP Socket ← Server
use crate::{
    Error,
    client::ClientOptions,
    messages::{MessageError, OwnedMessage, OwnedPayload},
    socket_manager::SocketManager,
};
use std::{future, net::SocketAddr};
use tokio::{
    select,
    sync::{mpsc, oneshot},
};
use tracing::{debug, trace};

/// Messages used to control the `DoIP` entities
#[derive(Debug)]
pub(super) enum ControlMessage {
    BindSocket(SocketAddr, oneshot::Sender<Result<u16, Error>>),
    UnbindSocket(oneshot::Sender<Result<(), Error>>),
    RoutingActivation(OwnedMessage, oneshot::Sender<Result<OwnedMessage, Error>>),
    AwaitResponse(
        /// the Request message that was sent
        OwnedMessage,
        /// the response channel to send the response to
        oneshot::Sender<Result<OwnedMessage, Error>>,
    ),

    /// Send diagnostic message and wait for `DoIP` ACK only (not full response)
    SendDiagnosticMessage(OwnedMessage, oneshot::Sender<Result<(), Error>>),

    /// Wait for next diagnostic response (no send)
    ReceiveDiagnosticResponse(
        std::time::Duration,
        oneshot::Sender<Result<OwnedMessage, Error>>,
    ),

    /// Internal: waiting for ACK only (after `SendDiagnosticMessage`)
    AwaitAck(oneshot::Sender<Result<(), Error>>),
}

impl ControlMessage {
    /// Helper method to create control messages with oneshot channels
    fn create_oneshot<T>(
        factory: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Self,
    ) -> (oneshot::Receiver<Result<T, Error>>, Self) {
        let (sender, receiver) = oneshot::channel();
        (receiver, factory(sender))
    }

    /// Create a control message to send a routing activation request
    pub fn create_routing_activation_message(
        message: &OwnedMessage,
    ) -> (oneshot::Receiver<Result<OwnedMessage, Error>>, Self) {
        trace!(message = "RoutingActivationRequest");
        Self::create_oneshot(|sender| Self::RoutingActivation(message.clone(), sender))
    }

    /// Builder for the bind socket message
    pub fn create_bind_socket_message(
        gateway_address: SocketAddr,
    ) -> (oneshot::Receiver<Result<u16, Error>>, Self) {
        Self::create_oneshot(|sender| Self::BindSocket(gateway_address, sender))
    }

    /// Create a control message to unbind the TCP socket
    pub fn create_unbind_socket_message() -> (oneshot::Receiver<Result<(), Error>>, Self) {
        Self::create_oneshot(Self::UnbindSocket)
    }

    /// Create a control message to send a diagnostic message and wait for ACK only
    pub fn create_send_diagnostic_message(
        message: OwnedMessage,
    ) -> (oneshot::Receiver<Result<(), Error>>, Self) {
        Self::create_oneshot(|sender| Self::SendDiagnosticMessage(message, sender))
    }

    /// Create a control message to receive the next diagnostic response
    pub fn create_receive_diagnostic_response(
        timeout: std::time::Duration,
    ) -> (oneshot::Receiver<Result<OwnedMessage, Error>>, Self) {
        Self::create_oneshot(|sender| Self::ReceiveDiagnosticResponse(timeout, sender))
    }
}

/// Inner client responsible for the handling of the connection details,
/// including creating 2 channels for sending and receiving messages.
/// It manages its inner state asynchronously, only propagating the
/// results to the outer client when the message is ready.
///
/// The inner state enters an asynchronous loop which runs in the background
pub(super) struct Inner<Conn> {
    client_options: ClientOptions,
    /// MPSC Receiver used to receive control messages from outer client
    control_receiver: mpsc::Receiver<ControlMessage>,
    /// MPSC Sender used to send updates to outer client
    update_sender: mpsc::Sender<Result<OwnedMessage, MessageError>>,

    /// active request in flight (if it exists) in case the connection is lost
    active_request: Option<ControlMessage>,

    /// Deadline for awaiting a response
    await_response_deadline: Option<tokio::time::Instant>,

    /// Socket manager for TCP data socket if bound
    tcp_data_socket: Option<SocketManager<Conn>>,

    /// Write-only bookkeeping flag. It is assigned in four places but never read:
    /// the [`Inner::run`] loop has no `while self.run` check.
    ///
    /// Shutdown does not go through this field. The loop exits when the control
    /// channel closes (the outer [`Client`](crate::client::Client) and its
    /// `control_sender` were dropped) or when
    /// [`Inner::process_received_message`] returns `true`. See ARCHITECTURE.md's
    /// known-issues section.
    run: bool,

    /// Buffer for diagnostic responses that arrive between send and receive calls.
    /// This handles the race condition where the server responds before
    /// `receive_diagnostic_response()` is called.
    pending_diagnostic_response: Option<OwnedMessage>,
}
/// Sender for the control messages sent via the control channel
type ControlSender = mpsc::Sender<ControlMessage>;
/// Receiver for the update messages from the socket
type UpdateReceiver<E> = mpsc::Receiver<Result<OwnedMessage, E>>;

/// DoIP control frames the run loop answers in-band and must NOT use to
/// resolve a pending diagnostic-response wait. An `AliveCheckRequest`
/// from the entity is answered with an `AliveCheckResponse` in
/// [`Inner::process_received_message`] and then swallowed so the loop
/// keeps waiting for the real `DiagnosticMessage`. Surfacing it instead
/// resolved the pending receive with the control frame, which the UDS
/// layer (`uds_on_ip`) then rejected as
/// "Expected DiagnosticMessage, got AliveCheckRequest".
fn is_transparently_handled_control(payload: &OwnedPayload) -> bool {
    matches!(payload, OwnedPayload::AliveCheckRequest)
}

/// How long to wait for the `DoIP` acknowledgement of a diagnostic message we
/// have sent.
///
/// This is `A_DoIP_Diagnostic_Message`, the parameter ISO 13400-2 defines for
/// the sender's wait on an ack/nack. It is deliberately *not*
/// [`crate::TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL`] (50 ms), which is the
/// *entity's* budget for emitting that ack once it has received the message —
/// using the entity's generation budget as the tester's wait made any ECU busy
/// enough to answer in >50 ms (rebooting, erasing flash) look like a dead link.
const ACK_TIMEOUT: std::time::Duration = crate::TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE;

/// Classify a frame taken from the socket manager's receive channel.
///
/// Only a closed channel (`None`) means the connection is gone: the socket loop
/// in [`SocketManager`] breaks — dropping its sender — on EOF, on a connection
/// reset, and on any other Io error. A `Some(Err(..))` is a frame that failed to
/// *decode*; that loop forwards it and keeps reading, so the socket is still
/// live. Collapsing both into [`Error::SocketClosedUnexpectedly`] made a single
/// undecodable frame tear down a healthy connection and report a close that
/// never happened.
fn classify_socket_frame(
    frame: Option<Result<OwnedMessage, MessageError>>,
) -> Result<OwnedMessage, Error> {
    match frame {
        Some(Ok(message)) => Ok(message),
        Some(Err(e)) => Err(Error::MessageError(e)),
        None => Err(Error::SocketClosedUnexpectedly),
    }
}

impl<Conn> Inner<Conn>
where
    Conn: crate::connection::Connector + 'static + Send + Sync,
{
    /// Spawns the inner client to run in the background and returns the send and recieve channels
    pub fn spawn(client_options: ClientOptions) -> (ControlSender, UpdateReceiver<MessageError>) {
        trace!("Spawning inner client");
        let (control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, update_receiver) = mpsc::channel(16);
        let inner = Inner::<Conn> {
            client_options,
            control_receiver,
            update_sender,
            active_request: None,
            await_response_deadline: None,
            tcp_data_socket: None,
            run: true,
            pending_diagnostic_response: None,
        };
        inner.run();
        (control_sender, update_receiver)
    }

    /// Binds the unicast socket to the specified address.
    /// If a socket already exists, it will be replaced with a new connection
    /// (this handles reconnection after connection loss).
    async fn bind_socket(&mut self, gateway_address: SocketAddr) -> Result<u16, Error> {
        // Close existing socket if any (handles reconnection)
        if let Some(old_socket) = self.tcp_data_socket.take() {
            debug!("Closing existing socket for reconnection");
            old_socket.shut_down().await;
        }

        // Bind a new socket
        let socket_manager: SocketManager<Conn> =
            SocketManager::bind(self.client_options, gateway_address).await?;
        let port = socket_manager.port();
        debug!("Bound socket to port: {}", port);
        self.tcp_data_socket = Some(socket_manager);
        self.run = true;
        Ok(port)
    }

    /// Unbind the socket
    /// This is used to gracefully shut down the socket manager
    async fn unbind_socket(&mut self) -> Result<(), Error> {
        // Check if the socket is already bound
        if let Some(socket_manager) = self.tcp_data_socket.take() {
            self.run = false;
            // Unbind the socket
            socket_manager.shut_down().await;
            Ok(())
        } else {
            Err(Error::SocketNotBound)
        }
    }

    /// Send a message to the socket manager
    ///
    /// Errors:
    /// [`Error::SocketNotBound`] - if the socket is not bound
    async fn send_to_socket(&mut self, message: OwnedMessage) -> Result<(), Error> {
        if let Some(socket) = &mut self.tcp_data_socket {
            socket.send(message).await
        } else {
            Err(Error::SocketNotBound)
        }
    }

    async fn receive_socket(
        socket_manager: &mut Option<SocketManager<Conn>>,
    ) -> Result<OwnedMessage, Error> {
        if let Some(receiver) = socket_manager {
            classify_socket_frame(receiver.receive().await)
        } else {
            // If we don't have a receiver, we should return a future that never resolves
            future::pending().await
        }
    }

    /// Handle the [`Inner::active_request`] that was set in the [`Inner::run`] loop.
    ///
    /// The `response` is a oneshot channel that is used to (generally) send the response back to the facade client
    ///
    /// ### Diagnostic Acks:
    /// * will not be sent to the user
    /// * will be handled internally
    async fn handle_control_message(&mut self) {
        // No active request? Nothing to do
        let Some(control_message) = self.active_request.take() else {
            return;
        };

        match control_message {
            ControlMessage::BindSocket(gateway_address, response) => {
                if response
                    .send(self.bind_socket(gateway_address).await)
                    .is_err()
                {
                    debug!("Failed to send bind socket response");
                }
            }
            ControlMessage::UnbindSocket(response) => {
                if response.send(self.unbind_socket().await).is_err() {
                    debug!("Failed to send unbind socket response");
                }
            }
            ControlMessage::RoutingActivation(message, response) => {
                let send_result = self.send_to_socket(message.clone()).await;

                // Await for the response through the run loop
                if send_result.is_ok() {
                    self.await_response_deadline =
                        Some(tokio::time::Instant::now() + crate::TCP_TIMEOUT_INITIAL_INACTIVITY);
                    self.active_request =
                        Some(ControlMessage::AwaitResponse(message.clone(), response));
                } else if let Err(e) = send_result
                    && response.send(Err(e)).is_err()
                {
                    debug!("Failed to send routing activation error response");
                }
            }
            ControlMessage::AwaitResponse(message, response) => {
                trace!("Awaiting response for message: {:?}", message);
                // This is handled in the run loop while receiving messages
                self.active_request = Some(ControlMessage::AwaitResponse(message, response));
            }
            ControlMessage::SendDiagnosticMessage(message, response) => {
                if self.tcp_data_socket.is_none() {
                    if response.send(Err(Error::SocketNotBound)).is_err() {
                        debug!("Failed to send response: Socket not bound");
                    }
                    return;
                }
                let send_result = self.send_to_socket(message).await;
                if send_result.is_ok() {
                    // Set up to wait for ACK only
                    self.await_response_deadline = Some(tokio::time::Instant::now() + ACK_TIMEOUT);
                    self.active_request = Some(ControlMessage::AwaitAck(response));
                } else if let Err(e) = send_result {
                    let _ = response.send(Err(e));
                }
            }
            ControlMessage::ReceiveDiagnosticResponse(timeout, response) => {
                if self.tcp_data_socket.is_none() {
                    if response.send(Err(Error::SocketNotBound)).is_err() {
                        debug!("Failed to send response: Socket not bound");
                    }
                    return;
                }

                // Check if we already have a buffered response from a fast server
                if let Some(buffered) = self.pending_diagnostic_response.take() {
                    debug!("Returning buffered diagnostic response");
                    let _ = response.send(Ok(buffered));
                    return;
                }

                self.await_response_deadline = Some(tokio::time::Instant::now() + timeout);
                // Use a placeholder message - we just want any diagnostic response
                self.active_request = Some(ControlMessage::AwaitResponse(
                    OwnedMessage::default(),
                    response,
                ));
            }
            ControlMessage::AwaitAck(response) => {
                // This is handled in the run loop while receiving messages
                self.active_request = Some(ControlMessage::AwaitAck(response));
            }
        }
    }

    /// Take the pending request only if it is an [`ControlMessage::AwaitAck`].
    ///
    /// Any other pending request is put back untouched: taking it unconditionally would
    /// drop its oneshot `Sender`, closing the channel and silently destroying an
    /// in-flight request.
    fn take_await_ack(&mut self) -> Option<oneshot::Sender<Result<(), Error>>> {
        match self.active_request.take() {
            Some(ControlMessage::AwaitAck(response)) => Some(response),
            other => {
                self.active_request = other;
                None
            }
        }
    }

    /// Take the pending request only if it is an [`ControlMessage::AwaitResponse`].
    ///
    /// As with [`Inner::take_await_ack`], any other pending request is put back untouched
    /// rather than dropped.
    fn take_await_response(
        &mut self,
    ) -> Option<(OwnedMessage, oneshot::Sender<Result<OwnedMessage, Error>>)> {
        match self.active_request.take() {
            Some(ControlMessage::AwaitResponse(request_message, response)) => {
                Some((request_message, response))
            }
            other => {
                self.active_request = other;
                None
            }
        }
    }

    /// Complete any still-pending request with [`Error::RequestSuperseded`] so that a
    /// newly arrived control message can take its place.
    ///
    /// Nothing here may panic: [`Inner::run`] is a detached task, so a panic would be
    /// invisible and would brick the client behind a closed control channel.
    fn supersede_active_request(&mut self) {
        match self.active_request.take() {
            None => {}
            Some(ControlMessage::AwaitAck(response)) => {
                debug!("Superseding pending AwaitAck");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::AwaitResponse(_, response)) => {
                debug!("Superseding pending AwaitResponse");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::RoutingActivation(_, response)) => {
                debug!("Superseding pending RoutingActivation");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::ReceiveDiagnosticResponse(_, response)) => {
                debug!("Superseding pending ReceiveDiagnosticResponse");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::SendDiagnosticMessage(_, response)) => {
                debug!("Superseding pending SendDiagnosticMessage");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::BindSocket(_, response)) => {
                debug!("Superseding pending BindSocket");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
            Some(ControlMessage::UnbindSocket(response)) => {
                debug!("Superseding pending UnbindSocket");
                let _ = response.send(Err(Error::RequestSuperseded));
            }
        }
        // The displaced request's deadline no longer applies; the arms of
        // `handle_control_message` that leave a request pending each set a fresh one.
        self.await_response_deadline = None;
    }

    /// Fail the active request once its response deadline has elapsed.
    ///
    /// Extracted from the [`Inner::run`] select arm so it can be tested
    /// directly. Behaviour is unchanged: every wait that can arm a deadline is
    /// answered, and anything else is put back rather than dropped — dropping a
    /// oneshot `Sender` reaches the caller as [`Error::ConnectionClosed`] (see
    /// [`crate::client::Client::send_diagnostic_message`]), which reports a dead
    /// link on a connection that is fine.
    fn handle_deadline_elapsed(&mut self) {
        debug!(
            "Await response deadline reached, server did not respond, which may mean you will not receive Negative Response or message was suppressed"
        );
        match self.active_request.take() {
            Some(ControlMessage::AwaitResponse(_, response)) => {
                if response.send(Err(Error::ResponseTimeoutExceeded)).is_err() {
                    debug!("Failed to send suppressed response");
                }
            }
            Some(ControlMessage::AwaitAck(response)) => {
                if response.send(Err(Error::ResponseTimeoutExceeded)).is_err() {
                    debug!("Failed to send suppressed ack timeout");
                }
            }
            other => self.active_request = other,
        }
        self.await_response_deadline = None;
    }

    /// Process a message received from the socket. Returns `true` if the run loop should exit.
    async fn process_received_message(&mut self, message: Result<OwnedMessage, Error>) -> bool {
        match message {
            Ok(received_message) => {
                match received_message.payload {
                    OwnedPayload::AliveCheckRequest => {
                        trace!("Received Alive Check Request, answering in-band");
                        let _ = self
                            .tcp_data_socket
                            .as_mut()
                            .unwrap()
                            .send(OwnedMessage::alive_check_response(
                                self.client_options.protocol_version,
                                self.client_options.client_logical_address,
                            ))
                            .await;
                    }
                    OwnedPayload::DiagnosticMessageAck(ref ack) => {
                        // Handle AwaitAck - complete immediately on ACK
                        if let Some(response) = self.take_await_ack() {
                            self.await_response_deadline = None;
                            if ack.ack_code.is_positive_ack() {
                                trace!("Received positive ACK, completing send");
                                let _ = response.send(Ok(()));
                            } else {
                                trace!("Received negative ACK: {:?}", ack.ack_code);
                                let _ =
                                    response.send(Err(Error::DiagnosticMessageNack(ack.ack_code)));
                            }
                            return false;
                        }
                        // For AwaitResponse, continue waiting for DiagnosticMessage
                        if ack.ack_code.is_positive_ack() {
                            trace!("Received Diagnostic Message Ack, waiting for full response");
                            return false;
                        }
                        // Negative ACK while an AwaitResponse is pending. Despite
                        // appearances this does NOT become an error today: for a
                        // `ReceiveDiagnosticResponse` the pending request message is
                        // `OwnedMessage::default()` (payload type `DiagnosticMessage`), and
                        // `Message::is_response` accepts
                        // `DiagnosticMessagePositiveAcknowledge` for it. Negative acks are
                        // stamped `0x8002` too, because
                        // `OwnedMessage::diagnostic_message_ack` hardcodes the positive
                        // payload type - so the `is_response` guard below passes and the
                        // caller receives `Ok(..)` carrying a rejection.
                        //
                        // ENTANGLEMENT: this is downstream of the deliberately-deferred
                        // `0x8002` hardcode (see ARCHITECTURE.md section 7.2). Whoever fixes
                        // that hardcode changes this path: once negative acks carry `0x8003`,
                        // `is_response` stops matching and the caller starts seeing an error
                        // instead. Decide deliberately what a rejected diagnostic message
                        // should surface as when making that change.
                    }
                    _ => trace!("Received message: {received_message:?}"),
                }
                // An in-band control frame (e.g. AliveCheckRequest, answered
                // in the match above) must not resolve a pending response
                // wait — keep the active request and keep listening for the
                // real diagnostic response. Without this guard the frame
                // reaches `take_await_response` below and fails the pending
                // receive with `UnexpectedMessageType`, which the UDS layer
                // (`uds_on_ip`) reports as "Expected DiagnosticMessage, got
                // AliveCheckRequest".
                if is_transparently_handled_control(&received_message.payload) {
                    trace!(
                        "Swallowing in-band control frame, not resolving pending receive: {:?}",
                        received_message.header.payload_type
                    );
                    return false;
                }
                if let Some((request_message, response)) = self.take_await_response() {
                    // Note: unlike the `take_await_ack` path above, this one deliberately
                    // leaves `await_response_deadline` set. That is safe, if not obvious:
                    // the timeout branch of the run loop is guarded on
                    // `active_request.is_some()`, which is now false, and every path that
                    // repopulates `active_request` from a user-issued control message
                    // (`RoutingActivation`, `SendDiagnosticMessage`,
                    // `ReceiveDiagnosticResponse`) sets a fresh deadline first - so the
                    // stale value can never fire.
                    trace!("Received response for request: {:?}", request_message);
                    trace!("{received_message:?}");
                    if request_message.is_response(received_message.header.payload_type) {
                        debug!("Received expected response, sending to the update channel");
                        if response.send(Ok(received_message)).is_err() {
                            return true;
                        }
                    } else if response
                        .send(Err(Error::UnexpectedMessageType(
                            received_message.header.payload_type,
                        )))
                        .is_err()
                    {
                        return true;
                    }
                } else if self.active_request.is_none() {
                    // No active request - check if this is a DiagnosticMessage we should buffer
                    if matches!(received_message.payload, OwnedPayload::DiagnosticMessage(_)) {
                        debug!(
                            "Buffering diagnostic response (no active request): {:?}",
                            received_message
                        );
                        self.pending_diagnostic_response = Some(received_message);
                    } else {
                        trace!("No active request, sending received message to update channel");
                        self.run = true;
                        if self.update_sender.send(Ok(received_message)).await.is_err() {
                            tracing::error!("Failed to send received message to update channel");
                            self.run = false;
                            return true;
                        }
                    }
                } else {
                    // A request is active but it is not an AwaitResponse — in
                    // practice an AwaitAck for a message we just sent. A
                    // diagnostic response can overtake that ack: after a
                    // reconnect the ECU answers the pre-reboot request on the
                    // new socket while our post-reconnect TesterPresent is still
                    // unacknowledged.
                    //
                    // `take_await_response` correctly restored the AwaitAck, but
                    // falling through here without buffering silently discarded
                    // the response. Buffer it for the next
                    // ReceiveDiagnosticResponse, exactly as the
                    // no-active-request path above does, and keep waiting for
                    // the ack.
                    if matches!(received_message.payload, OwnedPayload::DiagnosticMessage(_)) {
                        debug!(
                            "Buffering diagnostic response received while another request is active: {received_message:?}"
                        );
                        self.pending_diagnostic_response = Some(received_message);
                    } else {
                        trace!(
                            "Ignoring {:?} received while another request is active",
                            received_message.header.payload_type
                        );
                    }
                }
                false
            }
            // A frame we could not decode is not a closed connection. The
            // socket is still live and the response we are waiting for may
            // still be coming, so keep the connection and the pending wait
            // intact and let that wait's own deadline decide.
            //
            // TODO(ISO 13400-2 §7.1.5): answer this with a generic DoIP header
            // negative ack, and close the socket only for the nack codes that
            // require it. Part of the fuller spec coverage tracked alongside the
            // 14229-2 session-layer work.
            Err(Error::MessageError(e)) => {
                debug!("Discarding an undecodable frame, connection left intact: {e:?}");
                false
            }
            Err(e) => {
                debug!("Socket error, cleaning up connection: {:?}", e);
                // Clean up the broken socket but keep the inner task running
                // so that reconnection via BindSocket is still possible.
                if let Some(socket) = self.tcp_data_socket.take() {
                    socket.shut_down().await;
                }
                self.await_response_deadline = None;
                // Notify any active request of the connection error
                match self.active_request.take() {
                    Some(ControlMessage::AwaitResponse(_, response)) => {
                        let _ = response.send(Err(e));
                    }
                    Some(ControlMessage::AwaitAck(response)) => {
                        let _ = response.send(Err(e));
                    }
                    _ => {}
                }
                false
            }
        }
    }

    /// The run loop receives messages from the socket and handles control messages
    /// (like UDS Requests, Routing Activation) from the outer client.
    fn run(mut self) {
        tokio::spawn(async move {
            debug!("Starting DOIP processing loop");
            loop {
                let mut socket_message: Option<Result<OwnedMessage, Error>> = None;
                let mut deadline_elapsed = false;
                let Self {
                    control_receiver,
                    tcp_data_socket,
                    active_request,
                    await_response_deadline,
                    run,
                    ..
                } = &mut self;
                select! {
                    // Only sleep if there is a deadline set
                    () = async {
                        if let Some(deadline) = *await_response_deadline {
                            tokio::time::sleep_until(deadline).await;
                        }
                    }, if active_request.is_some() && await_response_deadline.is_some() => {
                        deadline_elapsed = true;
                    }
                    // Receive a control message
                    ctrl_opt = control_receiver.recv() => {
                        if ctrl_opt.is_none() {
                            debug!("Control channel closed, shutting down inner client");
                            *run = false;
                            break;
                        }
                        debug!("Received control message: {:?}", ctrl_opt.as_ref().unwrap());
                        // A request may still be pending here: the caller can stop awaiting
                        // one (`tokio::time::timeout`, `tokio::select!`, or the deliberately
                        // recoverable routing-activation timeout in `Client::bind_socket`) and
                        // then issue another. The new request wins - refusing it would strand
                        // the client behind a request nobody is waiting for - but the displaced
                        // request is told rather than dropped, per the take-or-restore
                        // discipline: dropping its oneshot `Sender` would surface as an
                        // indistinguishable closed channel.
                        self.supersede_active_request();
                        self.active_request = ctrl_opt;
                    }
                    // Receive a message from the socket
                    message = Inner::receive_socket(tcp_data_socket) => {
                        trace!("B: STREAM INCOMING from socket: {message:?}");
                        socket_message = Some(message);
                    }
                }
                if deadline_elapsed {
                    self.handle_deadline_elapsed();
                }
                if let Some(msg) = socket_message
                    && self.process_received_message(msg).await
                {
                    break;
                }
                self.handle_control_message().await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ACK_TIMEOUT, ControlMessage, Inner, classify_socket_frame, is_transparently_handled_control,
    };
    use crate::client::ClientOptions;
    use crate::connection::ConnectorSocket;
    use crate::messages::{
        DiagnosticAckCode, MessageError, OwnedMessage, OwnedPayload, PayloadType, ProtocolVersion,
    };
    use crate::{Error, LogicalAddress, TCP_PORT};
    use alloc::vec;
    use tokio::sync::{mpsc, oneshot};

    const TESTER: LogicalAddress = LogicalAddress(0x0E00);
    const ECU: LogicalAddress = LogicalAddress(0x4010);

    fn options() -> ClientOptions {
        ClientOptions {
            server_address: (std::net::Ipv4Addr::LOCALHOST, TCP_PORT).into(),
            server_logical_address: ECU,
            server_physical_address: ECU,
            client_address: std::net::Ipv4Addr::UNSPECIFIED.into(),
            client_logical_address: TESTER,
            protocol_version: ProtocolVersion::V2012,
            routing_activation_options: None,
        }
    }

    /// An `Inner` with no socket. The paths exercised here (diagnostic
    /// messages and their acks) never touch the socket.
    fn inner() -> Inner<ConnectorSocket> {
        let (_control_sender, control_receiver) = mpsc::channel(16);
        let (update_sender, _update_receiver) = mpsc::channel(16);
        Inner {
            client_options: options(),
            control_receiver,
            update_sender,
            active_request: None,
            await_response_deadline: None,
            tcp_data_socket: None,
            run: true,
            pending_diagnostic_response: None,
        }
    }

    /// `50 02 00 0A 01 F4` — a positive response to DiagnosticSessionControl
    /// ProgrammingSession, as seen from the sensor.
    fn programming_session_response() -> OwnedMessage {
        OwnedMessage::diagnostic_message(
            ProtocolVersion::V2012,
            ECU,
            TESTER,
            vec![0x50, 0x02, 0x00, 0x0A, 0x01, 0xF4],
        )
    }

    /// A positive ack for the TesterPresent (`3E 80`) we just sent.
    fn tester_present_ack() -> OwnedMessage {
        OwnedMessage::diagnostic_message_ack(
            ProtocolVersion::V2012,
            ECU,
            TESTER,
            DiagnosticAckCode::RoutingConfirmationAck,
            vec![0x3E, 0x80],
        )
    }

    #[tokio::test]
    async fn response_overtaking_an_ack_is_buffered_not_dropped() {
        // Regression: after a reconnect the ECU answers the pre-reboot request
        // on the new socket while our post-reconnect TesterPresent is still
        // unacknowledged. `take_await_response` restores the AwaitAck, but
        // without the buffering branch the response was silently discarded.
        // Both must survive.
        let mut inner = inner();
        let (ack_tx, mut ack_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitAck(ack_tx));

        let terminate = inner
            .process_received_message(Ok(programming_session_response()))
            .await;

        assert!(!terminate, "loop must keep running");
        // `Empty` (sender alive, nothing sent) — NOT `Closed`, which is what a
        // dropped oneshot produces and what the caller misreports as a
        // connection close. `is_err()` alone would accept either.
        assert!(
            matches!(ack_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
            "the AwaitAck oneshot must still be open, not resolved or dropped"
        );
        assert!(
            matches!(inner.active_request, Some(ControlMessage::AwaitAck(_))),
            "must still be waiting for the ack"
        );
        let buffered = inner
            .pending_diagnostic_response
            .expect("response must be buffered for the next receive");
        let OwnedPayload::DiagnosticMessage(diag) = buffered.payload else {
            panic!("expected a diagnostic message");
        };
        assert_eq!(diag.user_data, vec![0x50, 0x02, 0x00, 0x0A, 0x01, 0xF4]);
    }

    #[tokio::test]
    async fn ack_arriving_during_a_response_wait_leaves_the_wait_intact() {
        // Regression: `if let Some(AwaitAck(..)) = active_request.take()` took
        // unconditionally, so an ack arriving while an AwaitResponse was
        // pending dropped that response's oneshot. The code then logged
        // "waiting for full response" with nothing left to answer, and the
        // caller saw a spurious "connection closed".
        let mut inner = inner();
        let (resp_tx, mut resp_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitResponse(
            OwnedMessage::default(),
            resp_tx,
        ));

        let terminate = inner
            .process_received_message(Ok(tester_present_ack()))
            .await;

        assert!(!terminate, "loop must keep running");
        // `Empty`, not `Closed` — see the note in the test above.
        assert!(
            matches!(resp_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
            "the AwaitResponse oneshot must still be open"
        );
        assert!(
            matches!(
                inner.active_request,
                Some(ControlMessage::AwaitResponse(..))
            ),
            "must still be waiting for the response"
        );
    }

    #[tokio::test]
    async fn ack_still_completes_a_matching_send() {
        // The behaviour the two fixes above must not regress.
        let mut inner = inner();
        let (ack_tx, ack_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitAck(ack_tx));

        inner
            .process_received_message(Ok(tester_present_ack()))
            .await;

        assert!(
            matches!(ack_rx.await, Ok(Ok(()))),
            "a matching ack must complete the send"
        );
        assert!(inner.active_request.is_none());
        assert!(inner.await_response_deadline.is_none());
    }

    #[tokio::test]
    async fn an_unacked_send_times_out_rather_than_looking_like_a_closed_connection() {
        // Regression: the deadline arm ran `active_request.take()` before
        // matching `AwaitResponse`, so an `AwaitAck` whose deadline elapsed was
        // taken and dropped along with its oneshot. `send_diagnostic_message`
        // maps a closed oneshot to `Error::ConnectionClosed`, which uds_on_ip
        // classifies as a connection error and reconnects — on a link that was
        // merely slow to acknowledge.
        let mut inner = inner();
        let (ack_tx, ack_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitAck(ack_tx));
        inner.await_response_deadline = Some(tokio::time::Instant::now());

        inner.handle_deadline_elapsed();

        assert!(
            matches!(ack_rx.await, Ok(Err(Error::ResponseTimeoutExceeded))),
            "an unacknowledged send must report a timeout, not a closed connection"
        );
        assert!(inner.active_request.is_none());
        assert!(inner.await_response_deadline.is_none());
    }

    #[tokio::test]
    async fn a_response_wait_still_times_out_on_its_deadline() {
        // The behaviour the fix above must not regress.
        let mut inner = inner();
        let (resp_tx, resp_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitResponse(
            OwnedMessage::default(),
            resp_tx,
        ));
        inner.await_response_deadline = Some(tokio::time::Instant::now());

        inner.handle_deadline_elapsed();

        assert!(matches!(
            resp_rx.await,
            Ok(Err(Error::ResponseTimeoutExceeded))
        ));
        assert!(inner.active_request.is_none());
        assert!(inner.await_response_deadline.is_none());
    }

    #[test]
    fn the_ack_wait_uses_the_iso_13400_ack_timeout() {
        // The 50 ms `TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL` is the *entity's*
        // budget for emitting an ack after receiving a diagnostic message, not
        // the tester's budget for waiting on one. Waiting only 50 ms made every
        // busy ECU look like a dead link. `A_DoIP_Diagnostic_Message` is the
        // parameter that governs this wait.
        assert_eq!(ACK_TIMEOUT, crate::TIMEOUT_DIAGNOSTIC_MESSAGE_RESPONSE);
        assert!(ACK_TIMEOUT > crate::TIMEOUT_DIAGNOSTIC_MESSAGE_INITIAL);
    }

    #[test]
    fn an_undecodable_frame_is_not_a_closed_socket() {
        // The socket loop forwards non-Io decode errors and keeps running
        // (`socket_manager.rs`), so collapsing them into
        // `SocketClosedUnexpectedly` reports a close that never happened.
        let frame = classify_socket_frame(Some(Err(MessageError::UnexpectedPayloadType(
            PayloadType::VehicleAnnouncement,
        ))));
        assert!(
            matches!(frame, Err(Error::MessageError(_))),
            "a decode failure must surface as a message error, not a socket close"
        );
    }

    #[test]
    fn a_closed_receive_channel_is_a_closed_socket() {
        // `None` means the socket loop exited — the only genuine close signal.
        assert!(matches!(
            classify_socket_frame(None),
            Err(Error::SocketClosedUnexpectedly)
        ));
    }

    #[tokio::test]
    async fn a_decode_error_leaves_the_pending_wait_and_its_deadline_intact() {
        let mut inner = inner();
        let deadline = tokio::time::Instant::now() + core::time::Duration::from_secs(5);
        inner.await_response_deadline = Some(deadline);
        let (resp_tx, mut resp_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitResponse(
            OwnedMessage::default(),
            resp_tx,
        ));

        let terminate = inner
            .process_received_message(Err(Error::MessageError(
                MessageError::UnexpectedPayloadType(PayloadType::VehicleAnnouncement),
            )))
            .await;

        assert!(!terminate, "loop must keep running");
        assert!(
            matches!(resp_rx.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
            "the pending wait must survive a frame we could not decode"
        );
        assert!(
            matches!(
                inner.active_request,
                Some(ControlMessage::AwaitResponse(..))
            ),
            "must still be waiting for the real response"
        );
        // The connection-lost path clears this. Still set means we did not take
        // it, so the real response can still arrive and the wait can still time
        // out on its own schedule.
        assert_eq!(
            inner.await_response_deadline,
            Some(deadline),
            "an undecodable frame must not cancel the pending wait's deadline"
        );
    }

    #[tokio::test]
    async fn a_real_socket_close_still_fails_the_pending_wait() {
        // The behaviour the fix above must not regress.
        let mut inner = inner();
        inner.await_response_deadline =
            Some(tokio::time::Instant::now() + core::time::Duration::from_secs(5));
        let (resp_tx, resp_rx) = oneshot::channel();
        inner.active_request = Some(ControlMessage::AwaitResponse(
            OwnedMessage::default(),
            resp_tx,
        ));

        let terminate = inner
            .process_received_message(Err(Error::SocketClosedUnexpectedly))
            .await;

        assert!(!terminate, "the task stays alive so a reconnect can rebind");
        assert!(matches!(
            resp_rx.await,
            Ok(Err(Error::SocketClosedUnexpectedly))
        ));
        assert!(inner.active_request.is_none());
        assert!(inner.await_response_deadline.is_none());
    }

    #[test]
    fn alive_check_request_is_handled_in_band_not_surfaced() {
        // Regression: an AliveCheckRequest arriving while awaiting a
        // diagnostic response must be answered + swallowed (keep
        // waiting), not resolve the pending receive. Surfacing it made
        // uds_on_ip fail with "Expected DiagnosticMessage, got
        // AliveCheckRequest".
        assert!(is_transparently_handled_control(
            &OwnedPayload::AliveCheckRequest
        ));
    }

    #[test]
    fn substantive_frames_are_not_swallowed() {
        assert!(!is_transparently_handled_control(
            &OwnedPayload::DiagnosticMessageNack
        ));
        assert!(!is_transparently_handled_control(
            &OwnedPayload::EntityStatusRequest
        ));
        assert!(!is_transparently_handled_control(
            &OwnedPayload::VehicleIdentificationRequest
        ));
    }
}
