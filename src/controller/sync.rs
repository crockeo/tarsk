use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use hyper::body::Bytes;
use hyper::Body;
use hyper::Response;
use reqwest::Client;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use warp::Filter;

use super::deserialize_change_hashes;
use super::deserialize_changes;
use super::serialize_change_hashes;
use super::serialize_changes;
use super::utils;
use super::Event;
use crate::database::Database;
use crate::logging;

pub struct Sync {
    database: Arc<Database>,
    tx: mpsc::UnboundedSender<Event>,
}

impl Sync {
    pub fn new(database: Arc<Database>, tx: mpsc::UnboundedSender<Event>) -> Arc<Self> {
        Arc::new(Self { database, tx })
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
        let peers_url = format!("http://{}/api/v1/peers", super::REGISTRY_ADDR.to_string(),);
        let client = reqwest::Client::new();
        loop {
            let raw_peers = match client.get(&peers_url).send().await {
                Err(e) => {
                    logging::GLOBAL.error(format!("Failed to get peers from registry: {}", e));
                    continue;
                }
                Ok(raw_peers) => raw_peers,
            };

            // TODO: maybe handle this better?
            let raw_peers = raw_peers.bytes().await.expect("Can't get bytes?");
            let peers: Vec<SocketAddr> = match serde_json::from_slice(&raw_peers) {
                Err(e) => {
                    logging::GLOBAL.error(format!("Failed to parse peers : {}", e));
                    continue;
                }
                Ok(peers) => peers,
            };

            for peer in peers.into_iter() {
                if peer == local_addr {
                    continue;
                }

                if let Err(e) = self.query_changes_from_peer(&client, peer).await {
                    logging::GLOBAL.error(format!("Failed to sync with peer {}: {}", peer, e));
                    continue;
                }
            }

            let _ = self.tx.send(Event::Pull);
            tokio::time::sleep(Duration::from_secs(7)).await;
        }
    }

    async fn query_changes_from_peer(
        self: &Arc<Self>,
        client: &Client,
        peer: SocketAddr,
    ) -> anyhow::Result<()> {
        let change_hashes = self.database.get_heads();
        let raw_change_hashes = serialize_change_hashes(&change_hashes)?;

        let changes_url = format!("http://{}/api/v1/changes", peer,);
        let res = client
            .post(changes_url)
            .body(raw_change_hashes)
            .send()
            .await?;

        let raw_changes = res.bytes().await?;
        let changes = deserialize_changes(&raw_changes)?;
        self.database.apply_changes(changes)
    }

    async fn register(self: Arc<Self>, local_addr: SocketAddr) {
        let registry_url = format!(
            "http://{}/api/v1/register",
            super::REGISTRY_ADDR.to_string()
        );
        let client = reqwest::Client::new();
        loop {
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
