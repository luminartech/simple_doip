use std::net::{IpAddr, SocketAddr};

use doip::{
    client::{Client, ClientOptions},
    messages::{ActivationTypeCode, ProtocolVersion},
    LIDAR_LOGICAL_ADDRESS, TCP_PORT,
};
use uds_protocol::{ProtocolRequest, ProtocolResponse};

// Want to make using the client as easy as possible
// Ideally the code would be Client::connect("127.0.0.1") or something simple like that
// so we don't have to do a lot of boilerplate, but some is inevitable for such a complicated system,
// so the client options is likely the best way
//
// Should the Client connection do the routing activation, or do we want the user to still have to do each of those pieces?
// Where is the line for the facade. May not matter just to get something working.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = ClientOptions {
        server_address: SocketAddr::from(([127, 0, 0, 1], TCP_PORT)),
        // Is this always 0xE400 for the sensor?
        server_logical_address: LIDAR_LOGICAL_ADDRESS,
        server_physical_address: 0x4010,
        client_address: IpAddr::from([0, 0, 0, 0]),
        client_logical_address: 0x0E01,
        protocol_version: ProtocolVersion::V2012,
    };
    let mut client = Client::<ProtocolRequest, ProtocolResponse>::connect(options).await?;

    client.close().await?;
    Ok(())
}
