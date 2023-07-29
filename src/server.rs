use std::net::TcpListener;

const SERVER_TCP_PORT: u16 = 13400;
const SERVER_TCP_TLS_PORT: u16 = 3496;

pub struct Server {
    tcp_listener: TcpListener,
}

impl Server {
    pub fn new(tls: bool) -> Self {
        let port = match tls {
            true => SERVER_TCP_TLS_PORT,
            false => SERVER_TCP_PORT,
        };
        let tcp_listener =
            TcpListener::bind(format!("127.0.0.1:{port}")).expect("Failed to bind to TCP port");
        Server { tcp_listener }
    }

    pub fn run(&self) {
        for stream in self.tcp_listener.incoming() {
            match stream {
                Ok(stream) => {
                    println!("New connection: {}", stream.peer_addr().unwrap());
                }
                Err(e) => {
                    println!("Error: {}", e);
                }
            }
        }
    }
}
