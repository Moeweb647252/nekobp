use futures_util::StreamExt;
use log::{info, warn};
use ntex::{util::Bytes, web};
use std::{
    convert::Infallible,
    io::{self, Write},
};

const HOST: &str = "073.pw";

fn input(prompt: &str) -> io::Result<String> {
    io::stdout().write(prompt.as_bytes())?;
    io::stdout().flush()?;
    let mut buf = String::new();
    io::stdin().read_line(&mut buf)?;
    #[cfg(debug_assertions)]
    info!("input: {}", buf);
    Ok(buf)
}

trait HandelResp {
    fn handel_stream_resp(resp: reqwest::Response) -> web::HttpResponse;
    fn handel_resp(headers: reqwest::header::HeaderMap, content: Vec<u8>) -> web::HttpResponse;
    fn is_stream() -> bool;
}

struct Zero;

impl Iterator for Zero {
    type Item = Result<ntex::util::Bytes, Infallible>;
    fn next(&mut self) -> Option<Self::Item> {
        Some(ntex::util::Bytes::try_from(vec![0 as u8; 1024]))
    }
}

struct HandelDlResp;

impl HandelResp for HandelDlResp {
    fn handel_stream_resp(resp: reqwest::Response) -> web::HttpResponse {
        let mut builder = web::HttpResponse::Ok();
        resp.headers()
            .iter()
            .map(|i| (i.0.to_string(), i.1.to_str().unwrap_or("").to_string()))
            .for_each(|i| {
                builder.set_header(i.0, i.1);
            });
        let stream = resp
            .bytes_stream()
            .map(|s| s.map(|s| ntex::util::Bytes::from(s.to_vec())));
        return builder.streaming(stream);
    }
    fn handel_resp(_: reqwest::header::HeaderMap, _: Vec<u8>) -> web::HttpResponse {
        todo!()
    }
    fn is_stream() -> bool {
        true
    }
}

struct HandelMikananiResp;

impl HandelResp for HandelMikananiResp {
    fn handel_resp(headers: reqwest::header::HeaderMap, content: Vec<u8>) -> web::HttpResponse {
        let mut builder = web::HttpResponse::Ok();
        headers
            .iter()
            .map(|i| (i.0.to_string(), i.1.to_str().unwrap_or("").to_string()))
            .for_each(|i| {
                builder.set_header(i.0, i.1);
            });
        if let Ok(content) = String::from_utf8(content) {
            builder.body(content.replace("https://", format!("https://{}/", HOST).as_str()))
        } else {
            builder.body("")
        }
    }
    fn handel_stream_resp(_: reqwest::Response) -> web::HttpResponse {
        todo!()
    }
    fn is_stream() -> bool {
        false
    }
}

async fn hello() -> &'static str {
    "Hello"
}

async fn zero() -> web::HttpResponse {
    web::HttpResponse::Ok().streaming(futures_util::stream::iter(Zero))
}

async fn dl<T: HandelResp>(
    path: web::types::Path<(String, String)>,
    req: web::HttpRequest,
    body: Bytes,
) -> web::HttpResponse {
    let mut query = Vec::new();
    for i in req.query_string().split("&") {
        let mut i = i.split("=");
        if let (Some(k), Some(v)) = (i.next(), i.next()) {
            query.push((k.to_string(), v.to_string()))
        }
    }
    let host = &path.0;
    let path = &path.1;
    #[cfg(debug_assertions)]
    info!("host: {}, path:{}", host, path);
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
    .query(&query)
    .body(body.to_vec())
    .send()
    .await
    {
        Ok(resp) => {
            return match T::is_stream() {
                true => T::handel_stream_resp(resp),

                false => {
                    let headers = resp.headers().clone();
                    if let Ok(content) = resp.bytes().await {
                        T::handel_resp(headers, content.to_vec())
                    } else {
                        web::HttpResponse::ServiceUnavailable().body("")
                    }
                }
            }
        }
        Err(_) => match match req.method().as_str() {
            "GET" => client.get(format!("http://{}/{}", host, path)),
            "POST" => client.post(format!("http://{}/{}", host, path)),
            _ => return web::HttpResponse::ServiceUnavailable().body("503"),
        }
        .query(&query)
        .body(body.to_vec())
        .send()
        .await
        {
            Ok(resp) => {
                return match T::is_stream() {
                    true => T::handel_stream_resp(resp),

                    false => {
                        let headers = resp.headers().clone();
                        if let Ok(content) = resp.bytes().await {
                            T::handel_resp(headers, content.to_vec())
                        } else {
                            web::HttpResponse::ServiceUnavailable().body("")
                        }
                    }
                }
            }
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
                web::resource("rss/{domain}/{path}*").to(dl::<HandelMikananiResp>),
                web::resource("{domain}/{path}*").to(dl::<HandelDlResp>),
                web::resource("").to(hello),
                web::resource("zero").to(zero),
            ))
    })
    .bind((addr, port))?
    .workers(num_cpus::get() * 2)
    .run()
    .await
}
