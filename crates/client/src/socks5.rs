//! SOCKS5 server based on <https://github.com/ajmwagar/merino>. Modified to use Cloudflare's
//! DNS servers.

#![allow(dead_code)]

use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use serde::Deserialize;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

/// Version of socks
const SOCKS_VERSION: u8 = 0x05;

const RESERVED: u8 = 0x00;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct User {
    pub username: String,
    password: String,
}

pub struct SocksReply {
    // From rfc 1928 (S6),
    // the server evaluates the request, and returns a reply formed as follows:
    //
    //    +----+-----+-------+------+----------+----------+
    //    |VER | REP |  RSV  | ATYP | BND.ADDR | BND.PORT |
    //    +----+-----+-------+------+----------+----------+
    //    | 1  |  1  | X'00' |  1   | Variable |    2     |
    //    +----+-----+-------+------+----------+----------+
    //
    // Where:
    //
    //      o  VER    protocol version: X'05'
    //      o  REP    Reply field:
    //         o  X'00' succeeded
    //         o  X'01' general SOCKS server failure
    //         o  X'02' connection not allowed by ruleset
    //         o  X'03' Network unreachable
    //         o  X'04' Host unreachable
    //         o  X'05' Connection refused
    //         o  X'06' TTL expired
    //         o  X'07' Command not supported
    //         o  X'08' Address type not supported
    //         o  X'09' to X'FF' unassigned
    //      o  RSV    RESERVED
    //      o  ATYP   address type of following address
    //         o  IP V4 address: X'01'
    //         o  DOMAINNAME: X'03'
    //         o  IP V6 address: X'04'
    //      o  BND.ADDR       server bound address
    //      o  BND.PORT       server bound port in network octet order
    //
    buf: [u8; 10],
}

impl SocksReply {
    pub const fn new(status: ResponseCode) -> Self {
        let buf = [
            // VER
            SOCKS_VERSION,
            // REP
            status as u8,
            // RSV
            RESERVED,
            // ATYP
            1,
            // BND.ADDR
            0,
            0,
            0,
            0,
            // BND.PORT
            0,
            0,
        ];
        Self { buf }
    }

    pub async fn send<T>(&self, stream: &mut T) -> io::Result<()>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        stream.write_all(&self.buf[..]).await?;
        Ok(())
    }
}

#[derive(Error, Debug)]
pub enum MerinoError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Socks error: {0}")]
    Socks(#[from] ResponseCode),
}

#[derive(Debug, Error)]
/// Possible SOCKS5 Response Codes
pub enum ResponseCode {
    #[error("success")]
    Success = 0x00,
    #[error("SOCKS5 Server Failure")]
    Failure = 0x01,
    #[error("SOCKS5 Rule failure")]
    RuleFailure = 0x02,
    #[error("network unreachable")]
    NetworkUnreachable = 0x03,
    #[error("host unreachable")]
    HostUnreachable = 0x04,
    #[error("connection refused")]
    ConnectionRefused = 0x05,
    #[error("TTL expired")]
    TtlExpired = 0x06,
    #[error("command not supported")]
    CommandNotSupported = 0x07,
    #[error("addr type not supported")]
    AddrTypeNotSupported = 0x08,
    #[error("timeout")]
    Timeout = 0x99,
}

impl From<MerinoError> for ResponseCode {
    fn from(e: MerinoError) -> Self {
        match e {
            MerinoError::Socks(e) => e,
            MerinoError::Io(_) => Self::Failure,
        }
    }
}

/// DST.addr variant types
#[derive(PartialEq)]
enum AddrType {
    /// IP V4 address: X'01'
    V4 = 0x01,
    /// DOMAINNAME: X'03'
    Domain = 0x03,
    /// IP V6 address: X'04'
    V6 = 0x04,
}

impl AddrType {
    /// Parse Byte to Command
    const fn from(n: usize) -> Option<Self> {
        match n {
            1 => Some(Self::V4),
            3 => Some(Self::Domain),
            4 => Some(Self::V6),
            _ => None,
        }
    }

    // /// Return the size of the AddrType
    // fn size(&self) -> u8 {
    //     match self {
    //         AddrType::V4 => 4,
    //         AddrType::Domain => 1,
    //         AddrType::V6 => 16
    //     }
    // }
}

/// SOCK5 CMD Type
#[derive(Debug)]
enum SockCommand {
    Connect = 0x01,
    Bind = 0x02,
    UdpAssosiate = 0x3,
}

impl SockCommand {
    /// Parse Byte to Command
    const fn from(n: usize) -> Option<Self> {
        match n {
            1 => Some(Self::Connect),
            2 => Some(Self::Bind),
            3 => Some(Self::UdpAssosiate),
            _ => None,
        }
    }
}

/// Client Authentication Methods
pub enum AuthMethods {
    /// No Authentication
    NoAuth = 0x00,
    // GssApi = 0x01,
    /// Authenticate with a username / password
    UserPass = 0x02,
    /// Cannot authenticate
    NoMethods = 0xFF,
}

pub struct Merino {
    listener: TcpListener,
    users: Arc<Vec<User>>,
    auth_methods: Arc<Vec<u8>>,
    // Timeout for connections
    timeout: Duration,
}

impl Merino {
    /// Create a new Merino instance
    pub async fn new(
        port: u16,
        ip: &str,
        auth_methods: Vec<u8>,
        users: Vec<User>,
        timeout: Duration,
    ) -> io::Result<Self> {
        tracing::info!("listening on {ip}:{port}");
        Ok(Self {
            listener: TcpListener::bind((ip, port)).await?,
            auth_methods: Arc::new(auth_methods),
            users: Arc::new(users),
            timeout,
        })
    }

    pub async fn serve(&mut self) {
        tracing::info!("serving connections...");
        while let Ok((stream, client_addr)) = self.listener.accept().await {
            let users = self.users.clone();
            let auth_methods = self.auth_methods.clone();
            let timeout = self.timeout;
            tokio::spawn(async move {
                let mut client = SOCKClient::new(stream, users, auth_methods, timeout);
                match client.init().await {
                    Ok(()) => {}
                    Err(error) => {
                        tracing::error!("{error:?}, client: {client_addr:?}");

                        if let Err(e) = SocksReply::new(error.into()).send(&mut client.stream).await
                        {
                            tracing::warn!("Failed to send error code: {:?}", e);
                        }

                        if let Err(e) = client.shutdown().await {
                            tracing::warn!("Failed to shutdown TcpStream: {:?}", e);
                        };
                    }
                };
            });
        }
    }
}

pub struct SOCKClient<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> {
    stream: T,
    auth_nmethods: u8,
    auth_methods: Arc<Vec<u8>>,
    authed_users: Arc<Vec<User>>,
    socks_version: u8,
    timeout: Duration,
    resolver: TokioAsyncResolver,
}

impl<T> SOCKClient<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    /// Create a new `SOCKClient`
    pub fn new(
        stream: T,
        authed_users: Arc<Vec<User>>,
        auth_methods: Arc<Vec<u8>>,
        timeout: Duration,
    ) -> Self {
        Self {
            stream,
            auth_nmethods: 0,
            socks_version: 0,
            authed_users,
            auth_methods,
            timeout,
            resolver: TokioAsyncResolver::tokio(
                ResolverConfig::cloudflare(),
                ResolverOpts::default(),
            ),
        }
    }

    /// Create a new `SOCKClient` with no auth
    pub fn new_no_auth(stream: T, timeout: Duration) -> Self {
        // FIXME: use option here
        let authed_users: Arc<Vec<User>> = Arc::new(Vec::new());
        let no_auth: Vec<u8> = vec![AuthMethods::NoAuth as u8];
        let auth_methods: Arc<Vec<u8>> = Arc::new(no_auth);

        Self {
            stream,
            auth_nmethods: 0,
            socks_version: 0,
            authed_users,
            auth_methods,
            timeout,
            resolver: TokioAsyncResolver::tokio(
                ResolverConfig::cloudflare(),
                ResolverOpts::default(),
            ),
        }
    }

    /// Mutable getter for inner stream
    pub fn stream_mut(&mut self) -> &mut T {
        &mut self.stream
    }

    /// Check if username + password pair are valid
    fn authed(&self, user: &User) -> bool {
        self.authed_users.contains(user)
    }

    /// Shutdown a client
    pub async fn shutdown(&mut self) -> io::Result<()> {
        self.stream.shutdown().await?;
        Ok(())
    }

    pub async fn init(&mut self) -> Result<(), MerinoError> {
        tracing::debug!("new connection");
        let mut header = [0u8; 2];
        // Read a byte from the stream and determine the version being requested
        self.stream.read_exact(&mut header).await?;

        self.socks_version = header[0];
        self.auth_nmethods = header[1];

        tracing::trace!(
            "version: {} auth nmethods: {}",
            self.socks_version,
            self.auth_nmethods
        );

        if self.socks_version == SOCKS_VERSION {
            // Authenticate w/ client
            self.auth().await?;
            // Handle requests
            self.handle_client().await?;
        } else {
            tracing::warn!("init: unsupported version: SOCKS{}", self.socks_version);
            self.shutdown().await?;
        }

        Ok(())
    }

    async fn auth(&mut self) -> Result<(), MerinoError> {
        tracing::debug!("authenticating");
        // Get valid auth methods
        let methods = self.get_avalible_methods().await?;
        tracing::trace!("methods: {:?}", methods);

        let mut response = [0u8; 2];

        // Set the version in the response
        response[0] = SOCKS_VERSION;

        if methods.contains(&(AuthMethods::UserPass as u8)) {
            // Set the default auth method (NO AUTH)
            response[1] = AuthMethods::UserPass as u8;

            tracing::debug!("sending USER/PASS packet");
            self.stream.write_all(&response).await?;

            let mut header = [0u8; 2];

            // Read a byte from the stream and determine the version being requested
            self.stream.read_exact(&mut header).await?;

            // debug!("Auth Header: [{}, {}]", header[0], header[1]);

            // Username parsing
            let user_len = header[1] as usize;

            let mut username = vec![0; user_len];

            self.stream.read_exact(&mut username).await?;

            // Password Parsing
            let mut pass_len = [0u8; 1];
            self.stream.read_exact(&mut pass_len).await?;

            let mut password = vec![0; pass_len[0] as usize];
            self.stream.read_exact(&mut password).await?;

            let username = String::from_utf8_lossy(&username).to_string();
            let password = String::from_utf8_lossy(&password).to_string();

            let user = User { username, password };

            // Authenticate passwords
            if self.authed(&user) {
                tracing::debug!("access granted. user: {}", user.username);
                let response = [1, ResponseCode::Success as u8];
                self.stream.write_all(&response).await?;
            } else {
                tracing::debug!("access denied. user: {}", user.username);
                let response = [1, ResponseCode::Failure as u8];
                self.stream.write_all(&response).await?;

                // Shutdown
                self.shutdown().await?;
            }

            Ok(())
        } else if methods.contains(&(AuthMethods::NoAuth as u8)) {
            // set the default auth method (no auth)
            response[1] = AuthMethods::NoAuth as u8;
            tracing::debug!("sending NOAUTH packet");
            self.stream.write_all(&response).await?;
            tracing::debug!("NOAUTH sent");
            Ok(())
        } else {
            tracing::warn!("Client has no suitable Auth methods!");
            response[1] = AuthMethods::NoMethods as u8;
            self.stream.write_all(&response).await?;
            self.shutdown().await?;

            Err(MerinoError::Socks(ResponseCode::Failure))
        }
    }

    /// Handles a client
    pub async fn handle_client(&mut self) -> Result<usize, MerinoError> {
        tracing::debug!("Starting to relay data");

        let req = SOCKSReq::from_stream(&mut self.stream).await?;

        // if req.addr_type == AddrType::V6 {}

        // Log Request
        let displayed_addr = pretty_print_addr(&req.addr_type, &req.addr);
        tracing::info!(
            "new request: command: {:?} addr: {}, port: {}",
            req.command,
            displayed_addr,
            req.port
        );

        // Respond
        match req.command {
            // Use the Proxy to connect to the specified addr/port
            SockCommand::Connect => {
                tracing::debug!("handling CONNECT command");

                let sock_addr =
                    addr_to_socket(&req.addr_type, &req.addr, req.port, &self.resolver).await?;

                tracing::trace!("connecting to: {:?}", sock_addr);

                let mut target = timeout(self.timeout, async move {
                    TcpStream::connect(&sock_addr[..]).await
                })
                .await
                .map_err(|_| MerinoError::Socks(ResponseCode::ConnectionRefused))??;

                tracing::trace!("connected!");

                SocksReply::new(ResponseCode::Success)
                    .send(&mut self.stream)
                    .await?;

                tracing::trace!("copy bidirectional");
                match tokio::io::copy_bidirectional(&mut self.stream, &mut target).await {
                    // ignore not connected for shutdown error
                    Err(e) if e.kind() == std::io::ErrorKind::NotConnected => {
                        tracing::trace!("already closed");
                        Ok(0)
                    }
                    Err(e) => Err(MerinoError::Io(e)),
                    #[allow(clippy::cast_possible_truncation)]
                    Ok((_s_to_t, t_to_s)) => Ok(t_to_s as usize),
                }
            }
            SockCommand::Bind => Err(MerinoError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "Bind not supported",
            ))),
            SockCommand::UdpAssosiate => Err(MerinoError::Io(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "UdpAssosiate not supported",
            ))),
        }
    }

    /// Return the avalible methods based on `self.auth_nmethods`
    async fn get_avalible_methods(&mut self) -> io::Result<Vec<u8>> {
        let mut methods: Vec<u8> = Vec::with_capacity(self.auth_nmethods as usize);
        for _ in 0..self.auth_nmethods {
            let mut method = [0u8; 1];
            self.stream.read_exact(&mut method).await?;
            if self.auth_methods.contains(&method[0]) {
                methods.append(&mut method.to_vec());
            }
        }
        Ok(methods)
    }
}

/// Convert an address and `AddrType` to a `SocketAddr`
async fn addr_to_socket(
    addr_type: &AddrType,
    addr: &[u8],
    port: u16,
    resolver: &TokioAsyncResolver,
) -> io::Result<Vec<SocketAddr>> {
    match addr_type {
        AddrType::V6 => {
            let new_addr = (0..8)
                .map(|x| {
                    tracing::trace!("{} and {}", x * 2, (x * 2) + 1);
                    (u16::from(addr[x * 2]) << 8) | u16::from(addr[(x * 2) + 1])
                })
                .collect::<Vec<u16>>();

            Ok(vec![SocketAddr::from(SocketAddrV6::new(
                Ipv6Addr::new(
                    new_addr[0],
                    new_addr[1],
                    new_addr[2],
                    new_addr[3],
                    new_addr[4],
                    new_addr[5],
                    new_addr[6],
                    new_addr[7],
                ),
                port,
                0,
                0,
            ))])
        }
        AddrType::V4 => Ok(vec![SocketAddr::from(SocketAddrV4::new(
            Ipv4Addr::new(addr[0], addr[1], addr[2], addr[3]),
            port,
        ))]),
        AddrType::Domain => {
            tracing::info!("looking up domain: {:?}", addr);

            Ok(resolver
                .lookup_ip(
                    std::str::from_utf8(addr).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidInput, "invalid domain")
                    })?,
                )
                .await?
                .iter()
                .map(|addr| SocketAddr::from((addr, port)))
                .collect())
        }
    }
}

/// Convert an `AddrType` and address to String
fn pretty_print_addr(addr_type: &AddrType, addr: &[u8]) -> String {
    match addr_type {
        AddrType::Domain => String::from_utf8_lossy(addr).to_string(),
        AddrType::V4 => addr
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<String>>()
            .join("."),
        AddrType::V6 => {
            let addr_16 = (0..8)
                .map(|x| (u16::from(addr[x * 2]) << 8) | u16::from(addr[(x * 2) + 1]))
                .collect::<Vec<u16>>();

            addr_16
                .iter()
                .map(|x| format!("{x:x}"))
                .collect::<Vec<String>>()
                .join(":")
        }
    }
}

/// Proxy User Request
#[allow(dead_code)]
struct SOCKSReq {
    pub version: u8,
    pub command: SockCommand,
    pub addr_type: AddrType,
    pub addr: Vec<u8>,
    pub port: u16,
}

impl SOCKSReq {
    /// Parse a `SOCKSReq` from a `TcpStream`
    async fn from_stream<T>(stream: &mut T) -> Result<Self, MerinoError>
    where
        T: AsyncRead + AsyncWrite + Send + Unpin + 'static,
    {
        // From rfc 1928 (S4), the SOCKS request is formed as follows:
        //
        //    +----+-----+-------+------+----------+----------+
        //    |VER | CMD |  RSV  | ATYP | DST.ADDR | DST.PORT |
        //    +----+-----+-------+------+----------+----------+
        //    | 1  |  1  | X'00' |  1   | Variable |    2     |
        //    +----+-----+-------+------+----------+----------+
        //
        // Where:
        //
        //      o  VER    protocol version: X'05'
        //      o  CMD
        //         o  CONNECT X'01'
        //         o  BIND X'02'
        //         o  UDP ASSOCIATE X'03'
        //      o  RSV    RESERVED
        //      o  ATYP   address type of following address
        //         o  IP V4 address: X'01'
        //         o  DOMAINNAME: X'03'
        //         o  IP V6 address: X'04'
        //      o  DST.ADDR       desired destination address
        //      o  DST.PORT desired destination port in network octet
        //         order
        tracing::trace!("Server waiting for connect");
        let mut packet = [0u8; 4];
        // Read a byte from the stream and determine the version being requested
        stream.read_exact(&mut packet).await?;
        tracing::trace!("Server received {:?}", packet);

        if packet[0] != SOCKS_VERSION {
            tracing::warn!("from_stream Unsupported version: SOCKS{}", packet[0]);
            stream.shutdown().await?;
        }

        // Get command
        let Some(command) = SockCommand::from(packet[1] as usize) else {
            tracing::warn!("Invalid Command");
            stream.shutdown().await?;
            return Err(MerinoError::Socks(ResponseCode::CommandNotSupported));
        };

        // DST.address

        let Some(addr_type) = AddrType::from(packet[3] as usize) else {
            tracing::error!("No Addr");
            stream.shutdown().await?;
            return Err(MerinoError::Socks(ResponseCode::AddrTypeNotSupported));
        };

        tracing::trace!("Getting Addr");
        // Get Addr from addr_type and stream
        let addr: Vec<u8> = match addr_type {
            AddrType::Domain => {
                let mut dlen = [0u8; 1];
                stream.read_exact(&mut dlen).await?;
                let mut domain = vec![0u8; dlen[0] as usize];
                stream.read_exact(&mut domain).await?;
                domain
            }
            AddrType::V4 => {
                let mut addr = [0u8; 4];
                stream.read_exact(&mut addr).await?;
                addr.to_vec()
            }
            AddrType::V6 => {
                let mut addr = [0u8; 16];
                stream.read_exact(&mut addr).await?;
                addr.to_vec()
            }
        };

        // read DST.port
        let mut port = [0u8; 2];
        stream.read_exact(&mut port).await?;

        // Merge two u8s into u16
        let port = (u16::from(port[0]) << 8) | u16::from(port[1]);

        // Return parsed request
        Ok(Self {
            version: packet[0],
            command,
            addr_type,
            addr,
            port,
        })
    }
}
