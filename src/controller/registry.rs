use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;

use hyper::body::Bytes;
use hyper::Body;
use hyper::Response;
use tokio::sync::RwLock;
use warp::Filter;

use super::utils;

pub struct Registry {
    // TODO: maintain a better list of SocketAddrs
    // like:
    //
    // - maintaining a list of TTLs so we don't surface stale peers
    // - actively culling peers which go offline
    peers: RwLock<Vec<SocketAddr>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            peers: RwLock::new(vec![]),
        }
    }
}

impl Registry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn start(self: &Arc<Self>) {
        let register_peer = warp::any()
            .and(utils::as_context(&self.clone()))
            .and(warp::path("register"))
            .and(warp::post())
            .and(warp::body::bytes())
            .then(Self::register_peer);

        let get_peers = warp::any()
            .and(utils::as_context(&self.clone()))
            .and(warp::path("peers"))
            .and(warp::get())
            .then(Self::get_peers);

        let filters = warp::any()
            .and(warp::path("api"))
            .and(warp::path("v1"))
            .and(register_peer.or(get_peers));

        // TODO: have this return a Result<...> so that i can recover
        // if there's another registry active on the OS
        warp::serve(filters).run(([127, 0, 0, 1], 8042)).await
    }

    async fn register_peer(self: Arc<Self>, raw_socket_addr: Bytes) -> Response<Body> {
        let raw_socket_addr = match std::str::from_utf8(&raw_socket_addr) {
            Err(_) => {
                return Response::builder()
                    .status(400)
                    .body(Body::from("Provided socket addr is not valid utf8"))
                    .unwrap();
            }
            Ok(raw_socket_addr) => raw_socket_addr,
        };
        let socket_addr = match SocketAddr::from_str(raw_socket_addr) {
            Err(_) => {
                return Response::builder()
                    .status(400)
                    .body(Body::from(format!("Invalid socket `{}`", raw_socket_addr)))
                    .unwrap();
            }
            Ok(socket_addr) => socket_addr,
        };

        {
            // TODO: do we need to care about deduplication?
            let mut peers = self.peers.write().await;
            if !peers.contains(&socket_addr) {
                peers.push(socket_addr);
            }
        }

        Response::builder()
            .status(200)
            .body(Body::from(raw_socket_addr.to_string()))
            .unwrap()
    }

    async fn get_peers(self: Arc<Self>) -> Response<Body> {
        let peers = self.peers.read().await;
        let rendered_peers = serde_json::to_string(&*peers)
            .expect("Failed to render SocketAddrs. This should not happen.");

        Response::builder()
            .status(200)
            .body(Body::from(rendered_peers))
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_serve_registry() -> anyhow::Result<()> {
        let registry = Registry::new();
        registry.start().await;
        Ok(())
    }
}
