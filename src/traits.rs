use std::fmt::Debug;
use uds_protocol::WireFormat;
pub trait WirePayload: WireFormat + Send + Debug {}

impl<T: WireFormat + Debug + Send> WirePayload for T {}
