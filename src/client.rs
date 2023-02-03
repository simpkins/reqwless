use crate::headers::ContentType;
use crate::request::*;
use crate::response::*;
use crate::Error;
use embedded_io::asynch::{Read, Write};
use embedded_io::Error as _;
use embedded_nal_async::{Dns, SocketAddr, TcpConnect};
use embedded_tls::{Aes128GcmSha256, TlsConnection};
/// Client using embedded-nal-async traits to establish connections and perform HTTP requests.
///
use nourl::{Url, UrlScheme};
use rand_chacha::ChaCha8Rng;
use rand_core::{RngCore, SeedableRng};

/// An async HTTP client that can establish a TCP connection and perform
/// HTTP requests.
pub struct HttpClient<'a, T, D>
where
    T: TcpConnect + 'a,
    D: Dns + 'a,
{
    client: &'a T,
    dns: &'a D,
    tls: Option<TlsConfig<'a>>,
}

/// Type for TLS configuration of HTTP client.
pub struct TlsConfig<'a> {
    seed: u64,
    buffer: &'a mut [u8],
    verify: TlsVerify<'a>,
}

/// Supported verification modes.
pub enum TlsVerify<'a> {
    /// No verification of the remote host
    None,
    /// Use pre-shared keys for verifying
    Psk { identity: &'a [u8], psk: &'a [u8] },
}

impl<'a> TlsConfig<'a> {
    pub fn new(seed: u64, buffer: &'a mut [u8], verify: TlsVerify<'a>) -> Self {
        Self { seed, buffer, verify }
    }
}

impl<'a, T, D> HttpClient<'a, T, D>
where
    T: TcpConnect + 'a,
    D: Dns + 'a,
{
    /// Create a new HTTP client for a given connection handle and a target host.
    pub fn new(client: &'a T, dns: &'a D) -> Self {
        Self { client, dns, tls: None }
    }

    /// Create a new HTTP client for a given connection handle and a target host.
    pub fn new_with_tls(client: &'a T, dns: &'a D, tls: TlsConfig<'a>) -> Self {
        Self {
            client,
            dns,
            tls: Some(tls),
        }
    }

    async fn connect<'m>(
        &'m mut self,
        url: &Url<'m>,
    ) -> Result<HttpConnection<T::Connection<'m>, TlsConnection<'m, T::Connection<'m>, Aes128GcmSha256>>, Error> {
        let host = url.host();
        let port = url.port_or_default();

        let remote = self
            .dns
            .get_host_by_name(host, embedded_nal_async::AddrType::Either)
            .await
            .map_err(|_| Error::Dns)?;

        let conn = self
            .client
            .connect(SocketAddr::new(remote, port))
            .await
            .map_err(|e| Error::Network(e.kind()))?;

        if url.scheme() == UrlScheme::HTTPS {
            if let Some(tls) = self.tls.as_mut() {
                use embedded_tls::{TlsConfig, TlsContext};
                let mut rng = ChaCha8Rng::seed_from_u64(tls.seed);
                tls.seed = rng.next_u64();
                let mut config = TlsConfig::new().with_server_name(url.host());
                if let TlsVerify::Psk { identity, psk } = tls.verify {
                    config = config.with_psk(psk, &[identity]);
                }
                let mut conn: TlsConnection<'m, T::Connection<'m>, Aes128GcmSha256> =
                    TlsConnection::new(conn, tls.buffer);
                conn.open::<_, embedded_tls::NoVerify>(TlsContext::new(&config, &mut rng))
                    .await
                    .map_err(Error::Tls)?;
                Ok(HttpConnection::Tls(conn))
            } else {
                Ok(HttpConnection::Plain(conn))
            }
        } else {
            Ok(HttpConnection::Plain(conn))
        }
    }

    /// Create a connection to a server with the provided `endpoint_url`.
    /// The path in the url is considered the base path for requests.
    pub async fn endpoint<'m>(
        &'m mut self,
        endpoint_url: &'m str,
    ) -> Result<
        HttpEndpoint<'m, HttpConnection<T::Connection<'m>, TlsConnection<'m, T::Connection<'m>, Aes128GcmSha256>>>,
        Error,
    > {
        let endpoint_url = Url::parse(endpoint_url)?;

        let conn = self.connect(&endpoint_url).await?;
        Ok(HttpEndpoint {
            conn,
            host: endpoint_url.host(),
            base_path: endpoint_url.path(),
        })
    }
}

/// A HTTP endpoint
///
/// The underlying connection is closed when drop'ed.
pub struct HttpEndpoint<'a, C>
where
    C: Read + Write,
{
    pub conn: C,
    pub host: &'a str,
    pub base_path: &'a str,
}

impl<'a, C> HttpEndpoint<'a, C>
where
    C: Read + Write,
{
    /// Build a http request.
    ///
    /// The returned request builder can be used to modify request parameters,
    /// before sending the request.
    pub fn request<'conn>(&'conn mut self, method: Method, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::new(method, path).base_path(self.base_path).host(self.host),
        )
    }

    /// Create a new GET http request.
    pub fn get<'conn>(&'conn mut self, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::get(path).base_path(self.base_path).host(self.host),
        )
    }

    /// Create a new POST http request.
    pub fn post<'conn>(&'conn mut self, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::post(path).base_path(self.base_path).host(self.host),
        )
    }

    /// Create a new PUT http request.
    pub fn put<'conn>(&'conn mut self, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::put(path).base_path(self.base_path).host(self.host),
        )
    }

    /// Create a new DELETE http request.
    pub fn delete<'conn>(&'conn mut self, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::delete(path).base_path(self.base_path).host(self.host),
        )
    }

    /// Create a new HEAD http request.
    pub fn head<'conn>(&'conn mut self, path: &'a str) -> HttpRequestBuilder<'a, 'conn, C> {
        HttpRequestBuilder::new(
            &mut self.conn,
            Request::head(path).base_path(self.base_path).host(self.host),
        )
    }

    /// Send a request to an endpoint.
    ///
    /// The base path of the endpoint is prepended to the request path.
    /// The response headers are stored in the provided rx_buf, which should be sized to contain at least the response headers.
    ///
    /// The response is returned.
    pub async fn send<'buf, 'conn>(
        &'conn mut self,
        mut request: Request<'a>,
        rx_buf: &'buf mut [u8],
    ) -> Result<Response<'buf, 'conn, C>, Error> {
        request.base_path = Some(self.base_path);
        request.write(&mut self.conn).await?;
        Response::read(&mut self.conn, request.method, rx_buf).await
    }
}

/// Represents a HTTP connection that may be encrypted or unencrypted.
pub enum HttpConnection<T, S>
where
    T: Read + Write,
    S: Read + Write,
{
    Plain(T),
    Tls(S),
}

impl<T, S> HttpConnection<T, S>
where
    T: Read + Write,
    S: Read + Write,
{
    /// Send a request on an established connection.
    ///
    /// The request is sent in its raw form without any base path from the endpoint.
    /// The response headers are stored in the provided rx_buf, which should be sized to contain at least the response headers.
    ///
    /// The response is returned.
    pub async fn send<'buf, 'conn>(
        &'conn mut self,
        request: Request<'conn>,
        rx_buf: &'buf mut [u8],
    ) -> Result<Response<'buf, 'conn, HttpConnection<T, S>>, Error> {
        request.write(self).await?;
        Response::read(self, request.method, rx_buf).await
    }
}

impl<T, S> embedded_io::Io for HttpConnection<T, S>
where
    T: Read + Write,
    S: Read + Write,
{
    type Error = embedded_io::ErrorKind;
}

impl<T, S> embedded_io::asynch::Read for HttpConnection<T, S>
where
    T: Read + Write,
    S: Read + Write,
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Plain(conn) => conn.read(buf).await.map_err(|e| e.kind()),
            Self::Tls(conn) => conn.read(buf).await.map_err(|e| e.kind()),
        }
    }
}

impl<T, S> embedded_io::asynch::Write for HttpConnection<T, S>
where
    T: Read + Write,
    S: Read + Write,
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        match self {
            Self::Plain(conn) => conn.write(buf).await.map_err(|e| e.kind()),
            Self::Tls(conn) => conn.write(buf).await.map_err(|e| e.kind()),
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Plain(conn) => conn.flush().await.map_err(|e| e.kind()),
            Self::Tls(conn) => conn.flush().await.map_err(|e| e.kind()),
        }
    }
}

/// An async HTTP connection for performing a HTTP request + response roundtrip.
pub struct HttpRequestBuilder<'a, 'conn, T> {
    conn: &'conn mut T,
    request: RequestBuilder<'a>,
}

impl<'a, 'conn, T> HttpRequestBuilder<'a, 'conn, T>
where
    T: Write + Read,
{
    fn new(conn: &'conn mut T, request: RequestBuilder<'a>) -> Self {
        Self { conn, request }
    }

    /// Set optional headers on the request.
    pub fn headers(mut self, headers: &'a [(&'a str, &'a str)]) -> Self {
        self.request = self.request.headers(headers);
        self
    }

    /// Set the data to send in the HTTP request body.
    pub fn body(mut self, body: &'a [u8]) -> Self {
        self.request = self.request.body(body);
        self
    }

    /// Set the content type header for the request.
    pub fn content_type(mut self, content_type: ContentType) -> Self {
        self.request = self.request.content_type(content_type);
        self
    }

    /// Set the basic authentication header for the request.
    pub fn basic_auth(mut self, username: &'a str, password: &'a str) -> Self {
        self.request = self.request.basic_auth(username, password);
        self
    }

    /// Perform a HTTP request.
    ///
    /// The response headers are stored in the provided rx_buf, which should be sized to contain at least the response headers.
    ///
    /// The response together with the established connection is returned.
    pub async fn send<'buf>(mut self, rx_buf: &'buf mut [u8]) -> Result<Response<'buf, 'conn, T>, Error> {
        let request = self.request.build();
        request.write(&mut self.conn).await?;
        let response = Response::read(self.conn, request.method, rx_buf).await?;
        Ok(response)
    }
}
