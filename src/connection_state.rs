use crate::messages::PayloadType;
use tracing::warn;

/// The DoIP connection state machine
#[allow(unused)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ConnectionState {
    /// Socket is not connected. This state is here for the sake of
    /// completeness. The socket manager will generally not be in this state
    Listen,
    /// Socket is connected and waiting for activation
    /// 3.DoIP-127 NL
    Initialized,
    /// Routing activation succeeded (in DoIP, not DoIPInt)
    /// The Source Address (SA) will be connected at this point
    /// 3.DoIP-128 NL
    RegisteredPendingAuthentication,
    /// Routing activation succeeded (in DoIP, not DoIPInt)
    /// and authentication is complete (or is not required)
    /// 3.DoIP-129 NL
    RegisteredPendingConfirmation,
    /// After the confirmation is complete (or not required)
    /// 3.DoIP-130 NL
    RegisteredRoutingActive,
    /// TCP socket shall be closed and reset to the listen state
    /// and resources will be freed to allow a new connection.
    ///
    /// If authentication failed, or confirmation is rejected, or a timeout
    /// occurs, the connection state will be set to this state.
    /// 3.DoIP-132 NL
    Finalize,
}

#[allow(unused)]
impl ConnectionState {
    /// Check whether the given payload type is permitted in the current connection state
    pub fn is_message_allowed(&self, payload_type: &PayloadType) -> bool {
        match self {
            // if in Listen or Finalize state, no messages are allowed
            Self::Listen | Self::Finalize => return false,
            // if in RegisteredRoutingActive state, all messages are allowed
            Self::RegisteredRoutingActive => return true,
            // if in any other state, only routing activation and alive check messages are allowed
            _ => match payload_type {
                PayloadType::AliveCheckRequest => return self.allows_alive_check(),
                PayloadType::AliveCheckResponse => return self.allows_alive_check(),
                PayloadType::RoutingActivationRequest | PayloadType::RoutingActivationResponse => {
                    return self.allows_all_messages();
                }
                _ => {}
            },
        }
        warn!(
            "Implement message type check for state: {:?} and payload: {:?}",
            self, payload_type
        );
        true
    }
    /// Do not allow DoIP messages besides routing activation, authentication,
    /// and confirmation to be sent or responded to unless we're in the correct state.
    /// 3.DoIP-131 NL
    pub fn allows_all_messages(&self) -> bool {
        matches!(self, Self::RegisteredRoutingActive)
    }

    /// Only allow alive check messages to be sent when in a `Registered` state
    /// 3.DoIP-134 NL
    pub fn allows_alive_check(&self) -> bool {
        matches!(
            self,
            Self::RegisteredPendingAuthentication
                | Self::RegisteredPendingConfirmation
                | Self::RegisteredRoutingActive
        )
    }
}
