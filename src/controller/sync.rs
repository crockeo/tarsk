use std::sync::Arc;

use hyper::body::Bytes;
use hyper::Body;
use hyper::Response;
use warp::Filter;

use super::deserialize_change_hashes;
use super::serialize_changes;
use super::utils;
use crate::database::Database;

pub struct Sync {
    database: Arc<Database>,
}

impl Sync {
    pub fn new(database: Arc<Database>) -> Arc<Self> {
        Arc::new(Self { database })
    }

    pub async fn start(self: Arc<Self>) {
        {
            let sync = self.clone();
            tokio::spawn(sync.query_changes());
        }

        {
            let sync = self.clone();
            tokio::spawn(sync.register());
        }

        let serve_changes = warp::any()
            .and(utils::as_context(&self.clone()))
            .and(warp::path("changes"))
            .and(warp::get())
            .and(warp::body::bytes())
            .then(Self::serve_changes);

        let filters = serve_changes;

        // TODO: how to get our socket address here?
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

    async fn query_changes(self: Arc<Self>) {
        todo!()
    }

    async fn register(self: Arc<Self>) {
        todo!()
    }
}
