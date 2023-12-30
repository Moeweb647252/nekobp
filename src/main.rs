#![allow(unused_imports)]

use futures_util::StreamExt;
use log::{info, warn};
use ntex::{util::Bytes, web};
use std::{
    convert::Infallible,
    io::{self, Write},
};

const ZERO_BUF: [u8; 10240] = [0 as u8; 10240];

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
    async fn handel_resp(resp: reqwest::Response, req: &web::HttpRequest) -> web::HttpResponse;
}

struct Zero;

impl Iterator for Zero {
    type Item = Result<ntex::util::Bytes, Infallible>;
    fn next(&mut self) -> Option<Self::Item> {
        Some(ntex::util::Bytes::try_from(&ZERO_BUF[..]))
    }
}

struct HandelDlResp;

impl HandelResp for HandelDlResp {
    async fn handel_resp(resp: reqwest::Response, _: &web::HttpRequest) -> web::HttpResponse {
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
}

struct HandelDocResp;

impl HandelResp for HandelDocResp {
    async fn handel_resp(resp: reqwest::Response, req: &web::HttpRequest) -> web::HttpResponse {
        let mut builder = web::HttpResponse::Ok();
        let headers = resp.headers();
        headers
            .iter()
            .map(|i| (i.0.to_string(), i.1.to_str().unwrap_or("").to_string()))
            .for_each(|i| {
                builder.set_header(i.0, i.1);
            });
        let host_and_port = format!(
            "{}:{}",
            req.uri().host().unwrap_or("unknown"),
            req.uri()
                .port()
                .map(|v| v.as_str().to_string())
                .unwrap_or("443".to_string())
        );
        if let Ok(content) = resp.bytes().await {
            if let Ok(content) = String::from_utf8(content.to_vec()) {
                builder.body(
                    content
                        .replace("https://", format!("https://{}/", host_and_port).as_str())
                        .replace("http://", format!("http://{}/", host_and_port).as_str()),
                )
            } else {
                builder.body("")
            }
        } else {
            web::HttpResponse::ServiceUnavailable().body("")
        }
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
    let mut https_failed = false;
    let mut url = format!("https://{}/{}", host, path);
    loop {
        match match req.method().as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            _ => {
                return web::HttpResponse::ServiceUnavailable().body("503");
            }
        }
        .query(&query)
        .body(body.to_vec())
        .headers(
            (&req
                .headers()
                .iter()
                .map(|v| (v.0.to_string(), v.1.to_str().unwrap().to_string()))
                .filter(|v| v.0.to_uppercase().ne("HOST"))
                .collect::<std::collections::HashMap<String, String>>())
                .try_into()
                .unwrap(),
        )
        .send()
        .await
        {
            Ok(resp) => break T::handel_resp(resp, &req).await,
            Err(error) => {
                if !https_failed {
                    url = format!("http://{}/{}", host, path);
                    https_failed = true;
                    continue;
                }
                warn!("Error making request to {}/{}: {}", host, path, error);
                break web::HttpResponse::ServiceUnavailable().body("503");
            }
        }
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
                web::resource("doc/{domain}/{path}*").to(dl::<HandelDocResp>),
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
