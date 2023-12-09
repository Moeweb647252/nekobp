use futures_util::StreamExt;
use log::{info, warn};
use ntex::web;
async fn hello() -> &'static str {
    "Hello"
}

async fn dl(path: web::types::Path<(String, String)>, req: web::HttpRequest) -> web::HttpResponse {
    let host = &path.0;
    let path = &path.1;
    info!("Streaming: {}/{}", host, path);
    let builder = reqwest::Client::builder();
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            warn!("Error building reqwest client: {}", error);
            return web::HttpResponse::ServiceUnavailable().body("503");
        }
    };
    let resp = match req.method().as_str() {
        "GET" => client.get(format!("http://{}/{}", host, path)).send(),
        _ => {
            return web::HttpResponse::ServiceUnavailable().body("503");
        }
    };
    match resp.await {
        Ok(resp) => {
            let mut builder = web::HttpResponse::Ok();
            builder.content_type(
                resp.headers()
                    .get("Content-Type")
                    .map(|t| t.to_str().unwrap_or("application/octet-stream"))
                    .unwrap_or("application/octet-stream"),
            );
            if let Some(t) = resp.content_length() {
                builder.content_length(t);
            }
            let stream = resp
                .bytes_stream()
                .map(|s| s.map(|s| ntex::util::Bytes::from(s.to_vec())));
            return builder.streaming(stream);
        }
        Err(error) => {
            warn!("Error making request to {}/{}: {}", host, path, error);
            return web::HttpResponse::ServiceUnavailable().body("503");
        }
    }
}

#[ntex::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "info");
    env_logger::init();

    web::server(|| {
        web::App::new()
            // enable logger
            .wrap(web::middleware::Logger::default())
            .service((
                web::resource("{domain}/{path}*").to(dl),
                web::resource("/").to(hello),
            ))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
