use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::body::Bytes;
use hyper::Body;
use hyper::Response;
use tokio::net::TcpListener;
use warp::Filter;

use super::deserialize_change_hashes;
use super::serialize_changes;
use super::utils;
use crate::database::Database;
use crate::logging;

pub struct Sync {
    database: Arc<Database>,
}

impl Sync {
    pub fn new(database: Arc<Database>) -> Arc<Self> {
        Arc::new(Self { database })
    }

    pub async fn start(self: Arc<Self>) {
        let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .expect("Failed to bind a TCP socket? This shouldn't happen.");

        let local_addr = listener
            .local_addr()
            .expect("Failed to get TCP socket address. This shouldn't happen.");

        {
            let sync = self.clone();
            tokio::spawn(sync.query_changes(local_addr.clone()));
        }

        {
            let sync = self.clone();
            tokio::spawn(sync.register(local_addr.clone()));
        }

        let serve_changes = warp::any()
            .and(utils::as_context(&self.clone()))
            .and(warp::path("changes"))
            .and(warp::get())
            .and(warp::body::bytes())
            .then(Self::serve_changes);

        let filters = serve_changes;

        let stream = tokio_stream::wrappers::TcpListenerStream::new(listener);
        warp::serve(filters).run_incoming(stream).await
    }

    async fn serve_changes(self: Arc<Self>, raw_change_hashes: Bytes) -> Response<Body> {
        // TODO: consider learning how 2 macro to make this better?
        // unwrap Result and then write a custom status / message
        let change_hashes = match deserialize_change_hashes(&raw_change_hashes) {
            Err(_) => {
                return Response::builder()
                    .status(400)
                    .body(Body::from("Change hashes could not be deserialized"))
                    .unwrap();
            }
            Ok(change_hashes) => change_hashes,
        };

        let changes = match self.database.get_changes(&change_hashes) {
            Err(_) => {
                return Response::builder()
                    .status(400)
                    .body(Body::from("Failed to get changes from Database."))
                    .unwrap();
            }
            Ok(changes) => changes,
        };

        let raw_changes = match serialize_changes(&changes) {
            Err(_) => {
                return Response::builder()
                    .status(400)
                    .body(Body::from("Failed to serialize changes."))
                    .unwrap();
            }
            Ok(raw_changes) => raw_changes,
        };

        let (mut stream, body) = Body::channel();
        if let Err(_) = stream.send_data(Bytes::from(raw_changes)).await {
            return Response::builder()
                .status(500)
                .body(Body::from("Failed to send changes."))
                .unwrap();
        }

        Response::builder().status(200).body(body).unwrap()
    }

    async fn query_changes(self: Arc<Self>, local_addr: SocketAddr) {
        loop {
            // TODO: implement
            //
            // v0 implementation will be naive
            // and just query registry every time
            // it needs to find its peers
            //
            // - query registry server at super::REGISTRY_ADDR on
            //   GET /api/v1/peers
            //
            // - iterate through Vec<SocketAddr> that gets returned
            //   for each:
            //   - if it's eq. to local_addr, then just skip
            //   - otherwise make a request to that addr on
            //     GET /api/v1/changes
            tokio::time::sleep(Duration::from_secs(7)).await;
        }
    }

    async fn register(self: Arc<Self>, local_addr: SocketAddr) {
        let registry_url = format!(
            "http://{}/api/v1/register",
            super::REGISTRY_ADDR.to_string()
        );
        loop {
            let client = reqwest::Client::new();
            if let Err(e) = client
                .post(&registry_url)
                .body(local_addr.to_string())
                .send()
                .await
            {
                logging::GLOBAL.error(format!("Failed to register client to registry: {}", e));
            }
            tokio::time::sleep(Duration::from_secs(9)).await;
        }
    }
}
