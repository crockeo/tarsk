use hyper::Body;
use tower::Service;

// https://tokio.rs/blog/2021-05-14-inventing-the-service-trait
// Learn how to do the Service trait thing using the tokio thing.
struct Registry {}

impl Registry {
    fn new() -> Self {
        todo!()
    }
}

impl Service<Body> for Registry {
    type Response = ();
    type Error = ();
    type Future = ();

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        todo!()
    }

    fn call(&mut self, req: Body) -> Self::Future {
        todo!()
    }
}
