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
                let body = Body::from("hello world");
                return Response::new(body);
            }
            Ok(raw_socket_addr) => raw_socket_addr,
        };
        let socket_addr = match SocketAddr::from_str(raw_socket_addr) {
            Err(_) => todo!(),
            Ok(socket_addr) => socket_addr,
        };

        {
            // TODO: do we need to care about deduplication?
            let mut peers = self.peers.write().await;
            if !peers.contains(&socket_addr) {
                peers.push(socket_addr);
            }
        }

        let body = Body::default();
        Response::new(body)
    }

    async fn get_peers(self: Arc<Self>) -> Response<Body> {
        let peers = self.peers.read().await;
        let rendered_peers = match serde_json::to_string(&*peers) {
            Err(_) => todo!(),
            Ok(rendered_peers) => rendered_peers,
        };

        let (mut stream, body) = Body::channel();
        if let Err(_) = stream.send_data(Bytes::from(rendered_peers)).await {
            todo!()
        }
        Response::new(body)
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
