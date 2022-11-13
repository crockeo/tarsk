use warp::Filter;

pub fn as_context<T: Clone + Send>(
    context: &T,
) -> impl Filter<Extract = (T,), Error = std::convert::Infallible> + Clone {
    let context = context.clone();
    warp::any().map(move || context.clone())
}
