use futures_util::StreamExt;
use log::{info, warn};
use ntex::{util::Bytes, web};
use std::io::{self, Write};

fn input(prompt: &str) -> io::Result<String> {
    io::stdout().write(prompt.as_bytes())?;
    io::stdout().flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    #[cfg(debug_assertions)]
    info!("input: {}", buf);
    Ok(buf)
}

fn handel_resp(resp: reqwest::Response) -> web::HttpResponse {
    info!("Streaming: {}", resp.url());
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

async fn hello() -> &'static str {
    "Hello"
}

async fn dl(
    path: web::types::Path<(String, String)>,
    req: web::HttpRequest,
    body: Bytes,
) -> web::HttpResponse {
    let host = &path.0;
    let path = &path.1;
    let builder = reqwest::Client::builder();
    let client = match builder.build() {
        Ok(client) => client,
        Err(error) => {
            warn!("Error building reqwest client: {}", error);
            return web::HttpResponse::ServiceUnavailable().body("503");
        }
    };
    match match req.method().as_str() {
        "GET" => client.get(format!("https://{}/{}", host, path)),
        "POST" => client.post(format!("https://{}/{}", host, path)),
        _ => {
            return web::HttpResponse::ServiceUnavailable().body("503");
        }
    }
    .query(req.query_string())
    .body(body.to_vec())
    .send()
    .await
    {
        Ok(resp) => return handel_resp(resp),
        Err(_) => match match req.method().as_str() {
            "GET" => client.get(format!("http://{}/{}", host, path)),
            "POST" => client.post(format!("http://{}/{}", host, path)),
            _ => return web::HttpResponse::ServiceUnavailable().body("503"),
        }
        .query(req.query_string())
        .body(body.to_vec())
        .send()
        .await
        {
            Ok(resp) => return handel_resp(resp),
            Err(error) => {
                warn!("Error making request to {}/{}: {}", host, path, error);
                return web::HttpResponse::ServiceUnavailable().body("503");
            }
        },
    }
}

#[ntex::main]
async fn main() -> io::Result<()> {
    std::env::set_var("RUST_LOG", "info");
    env_logger::init();

    let args: Vec<String> = std::env::args().collect();
    let usage = format!("Usage: {} [addr] / [ip] [port]", args[0]);
    let (addr, port) = match args.len() {
        1 => (
            input("addr:")?.trim().to_string(),
            input("port:")?.trim().parse().expect(&usage),
        ),
        2 => {
            let mut args = args[1].split(":");
            (
                args.next().expect(&usage).to_string(),
                args.next().expect(&usage).parse().expect(&usage),
            )
        }
        _ => (args[1].clone(), args[2].parse().expect(&usage)),
    };

    web::server(|| {
        web::App::new()
            // enable logger
            .wrap(web::middleware::Logger::default())
            .service((
                web::resource("{domain}/{path}*").to(dl),
                web::resource("/").to(hello),
            ))
    })
    .bind((addr, port))?
    .workers(num_cpus::get())
    .run()
    .await
}
