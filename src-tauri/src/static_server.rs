use crate::models::{ProxyRule, StaticServerConfig};
use axum::{
    body::Body,
    extract::ws::{Message as WsMessage, WebSocketUpgrade},
    extract::{FromRequestParts, Request, State as AxumState},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use std::path::PathBuf;
use tokio_tungstenite::tungstenite::protocol::CloseFrame as TgCloseFrame;
use tokio_tungstenite::tungstenite::Message as TgMessage;
use tower_http::cors::CorsLayer;

pub async fn start_static_server(
    config: &StaticServerConfig,
) -> Result<tokio::task::AbortHandle, String> {
    let port = config.port;
    let root_dir = config.root_dir.clone();
    let spa_mode = config.spa_mode;
    let index_file = config.index_file.clone();
    let proxy_rules = config.proxy_rules.clone();

    let root = PathBuf::from(&root_dir);
    if !root.exists() {
        return Err(format!("目录不存在: {}", root_dir));
    }
    if !root.is_dir() {
        return Err(format!("不是目录: {}", root_dir));
    }

    let canonical_root = root.canonicalize().map_err(|e| format!("无法解析目录: {}", e))?;

    let server_state = ServerState {
        root_dir: canonical_root,
        spa_mode,
        index_file,
        proxy_rules,
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .connect_timeout(std::time::Duration::from_secs(10))
            .no_proxy()
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?,
    };

    let app = Router::new()
        .fallback(any(handler))
        .layer(CorsLayer::permissive())
        .with_state(server_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .map_err(|e| format!("端口 {} 绑定失败: {}", port, e))?;

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    }).abort_handle();

    Ok(handle)
}

#[derive(Clone)]
struct ServerState {
    root_dir: PathBuf,
    spa_mode: bool,
    index_file: String,
    proxy_rules: Vec<ProxyRule>,
    client: reqwest::Client,
}

fn sanitize_path(path: &str, root: &PathBuf) -> Option<PathBuf> {
    let decoded = percent_decode(path)?;
    let file_path = root.join(&decoded);
    match file_path.canonicalize() {
        Ok(canonical) => {
            if canonical.starts_with(root) {
                Some(canonical)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

/// 解码 URL 编码路径。`+` 按空格处理，`%XX` 转义为对应字节；
/// 解码结果不是合法 UTF-8 时返回 `None`（由调用方拒绝请求，避免回退到根目录）。
fn percent_decode(input: &str) -> Option<String> {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = String::from_utf8_lossy(&bytes[i + 1..i + 3]);
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            result.push(b' ');
        } else {
            result.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8(result).ok()
}

fn match_proxy_rule<'a>(path: &str, rules: &'a [ProxyRule]) -> Option<&'a ProxyRule> {
    let mut best_match: Option<&'a ProxyRule> = None;
    let mut best_len = 0;

    for rule in rules {
        let rule_path = rule.path.trim_end_matches('/');
        let req_path = path.trim_end_matches('/');

        let matched = req_path == rule_path || req_path.starts_with(&format!("{}/", rule_path));
        if matched && rule_path.len() > best_len {
            best_len = rule_path.len();
            best_match = Some(rule);
        }
    }

    best_match
}

async fn handler(
    AxumState(state): AxumState<ServerState>,
    req: Request,
) -> Response {
    let uri = req.uri().clone();
    let path = uri.path();

    let is_ws_upgrade = req
        .headers()
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    if let Some(rule) = match_proxy_rule(path, &state.proxy_rules) {
        if is_ws_upgrade {
            return ws_proxy(rule, req).await;
        }
        return proxy_request(rule, req, &state.client).await;
    }

    let range = req
        .headers()
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let trimmed = path.trim_start_matches('/');

    if trimmed.is_empty() {
        if let Some(resp) = serve_file(&state.root_dir.join(&state.index_file), range.as_deref()).await {
            return resp;
        }
        return (StatusCode::NOT_FOUND, "Not Found").into_response();
    }

    if let Some(file_path) = sanitize_path(trimmed, &state.root_dir) {
        if file_path.is_file() {
            return serve_file(&file_path, range.as_deref())
                .await
                .unwrap_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "Read Error").into_response());
        }

        if file_path.is_dir() {
            let index = file_path.join(&state.index_file);
            if index.is_file() {
                return serve_file(&index, range.as_deref())
                    .await
                    .unwrap_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, "Read Error").into_response());
            }
        }
    }

    if state.spa_mode {
        let index_path = state.root_dir.join(&state.index_file);
        if index_path.is_file() {
            return serve_file(&index_path, range.as_deref())
                .await
                .unwrap_or_else(|| (StatusCode::NOT_FOUND, "Not Found").into_response());
        }
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}

async fn serve_file(path: &PathBuf, range: Option<&str>) -> Option<Response> {
    let content = tokio::fs::read(path).await.ok()?;
    let mime = guess_mime(path);
    let size = content.len();

    if size > 0 {
        if let Some(r) = range {
            if let Some(rest) = r.strip_prefix("bytes=") {
                if let Some((start_s, end_s)) = rest.split_once('-') {
                    let start: usize = start_s.parse().unwrap_or(0).min(size - 1);
                    let end: usize = if end_s.is_empty() {
                        size - 1
                    } else {
                        end_s.parse().unwrap_or(size - 1).min(size - 1)
                    };
                    if start <= end {
                        let len = end - start + 1;
                        let slice = content[start..=end].to_vec();
                        return Some(
                            (
                                StatusCode::PARTIAL_CONTENT,
                                [
                                    (header::CONTENT_TYPE, mime),
                                    (header::CONTENT_LENGTH, len.to_string()),
                                    (
                                        header::CONTENT_RANGE,
                                        format!("bytes {}-{}/{}", start, end, size),
                                    ),
                                    (header::ACCEPT_RANGES, "bytes".to_string()),
                                    (
                                        header::CACHE_CONTROL,
                                        "no-cache, no-store, must-revalidate".to_string(),
                                    ),
                                ],
                                slice,
                            )
                                .into_response(),
                        );
                    }
                }
            }
        }
    }

    Some(
        (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CONTENT_LENGTH, size.to_string()),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (
                    header::CACHE_CONTROL,
                    "no-cache, no-store, must-revalidate".to_string(),
                ),
            ],
            content,
        )
            .into_response(),
    )
}

/// 将 WebSocket 升级请求代理到目标服务（支持 Vite HMR 等场景）
async fn ws_proxy(rule: &ProxyRule, req: Request) -> Response {
    let target = rule.target.trim_end_matches('/');
    let scheme = if target.starts_with("https://") {
        "wss"
    } else {
        "ws"
    };
    let rest = target
        .trim_start_matches("http://")
        .trim_start_matches("https://");

    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();

    let proxy_path = if rule.rewrite {
        let rule_path = rule.path.trim_end_matches('/');
        let stripped = path.trim_end_matches('/').strip_prefix(rule_path).unwrap_or("");
        if stripped.is_empty() {
            String::new()
        } else {
            stripped.to_string()
        }
    } else {
        path.to_string()
    };

    let ws_url = format!("{}://{}{}{}", scheme, rest, proxy_path, query);

    let (mut parts, _body) = req.into_parts();

    let upgrade = match WebSocketUpgrade::from_request_parts(&mut parts, &()).await {
        Ok(u) => u,
        Err(_) => return (StatusCode::BAD_REQUEST, "WebSocket upgrade failed").into_response(),
    };

    upgrade.on_upgrade(move |socket| async move {
        match tokio_tungstenite::connect_async(&ws_url).await {
            Ok((backend, _)) => {
                let (mut client_tx, mut client_rx) = socket.split();
                let (mut backend_tx, mut backend_rx) = backend.split();

                let client_to_backend = async move {
                    while let Some(msg) = client_rx.next().await {
                        match msg {
                            Ok(m) => {
                                if let Some(t) = axum_to_tg(m) {
                                    if backend_tx.send(t).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                };

                let backend_to_client = async move {
                    while let Some(msg) = backend_rx.next().await {
                        match msg {
                            Ok(m) => {
                                if let Some(t) = tg_to_axum(m) {
                                    if client_tx.send(t).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(_) => break,
                        }
                    }
                };

                tokio::select! {
                    _ = client_to_backend => {},
                    _ = backend_to_client => {},
                }
            }
            Err(_) => {}
        }
    })
}

fn axum_to_tg(msg: WsMessage) -> Option<TgMessage> {
    match msg {
        WsMessage::Text(t) => Some(TgMessage::Text(t.to_string().into())),
        WsMessage::Binary(b) => Some(TgMessage::Binary(b.to_vec())),
        WsMessage::Ping(b) => Some(TgMessage::Ping(b.to_vec())),
        WsMessage::Pong(b) => Some(TgMessage::Pong(b.to_vec())),
        WsMessage::Close(f) => Some(TgMessage::Close(f.map(|f| TgCloseFrame {
            code: f.code.into(),
            reason: f.reason.to_string().into(),
        }))),
    }
}

fn tg_to_axum(msg: TgMessage) -> Option<WsMessage> {
    match msg {
        TgMessage::Text(t) => Some(WsMessage::Text(t.to_string().into())),
        TgMessage::Binary(b) => Some(WsMessage::Binary(b.into())),
        TgMessage::Ping(b) => Some(WsMessage::Ping(b.into())),
        TgMessage::Pong(b) => Some(WsMessage::Pong(b.into())),
        TgMessage::Close(f) => Some(WsMessage::Close(f.map(|f| axum::extract::ws::CloseFrame {
            code: f.code.into(),
            reason: f.reason.to_string().into(),
        }))),
        _ => None,
    }
}

async fn proxy_request(rule: &ProxyRule, req: Request, client: &reqwest::Client) -> Response {
    let target = rule.target.trim_end_matches('/');
    let path = req.uri().path();
    let query = req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();

    let proxy_url = if rule.rewrite {
        let rule_path = rule.path.trim_end_matches('/');
        let stripped = path.trim_end_matches('/').strip_prefix(rule_path).unwrap_or("");
        if stripped.is_empty() {
            format!("{}{}", target, query)
        } else {
            format!("{}{}{}", target, stripped, query)
        }
    } else {
        format!("{}{}{}", target, path, query)
    };

    let method = match *req.method() {
        Method::GET => reqwest::Method::GET,
        Method::POST => reqwest::Method::POST,
        Method::PUT => reqwest::Method::PUT,
        Method::DELETE => reqwest::Method::DELETE,
        Method::PATCH => reqwest::Method::PATCH,
        Method::HEAD => reqwest::Method::HEAD,
        Method::OPTIONS => reqwest::Method::OPTIONS,
        _ => reqwest::Method::GET,
    };

    let (parts, body) = req.into_parts();

    let mut req_builder = client.request(method, &proxy_url);

    for (name, value) in parts.headers.iter() {
        let name_str = name.as_str();
        if matches!(
            name_str,
            "host" | "connection" | "transfer-encoding" | "content-length" | "accept-encoding"
        ) {
            continue;
        }
        if let Ok(v) = value.to_str() {
            req_builder = req_builder.header(name_str, v);
        }
    }

    let body_bytes = axum::body::to_bytes(body, 50 * 1024 * 1024).await;

    if let Ok(bytes) = body_bytes {
        if !bytes.is_empty() {
            req_builder = req_builder.body(bytes);
        }
    }

    match req_builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            let resp_headers = resp.headers().clone();

            let stream = resp.bytes_stream();
            let body = Body::from_stream(stream.map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, e)
            }));

            let mut response = Response::builder().status(status.as_u16());

            if let Some(headers_mut) = response.headers_mut() {
                for (key, value) in resp_headers.iter() {
                    let key_str = key.as_str();
                    if matches!(
                        key_str,
                        "content-encoding" | "transfer-encoding" | "connection"
                    ) {
                        continue;
                    }
                    headers_mut.insert(key, value.clone());
                }
            }

            response
                .body(body)
                .unwrap_or_else(|_| (StatusCode::BAD_GATEWAY, "Proxy Error").into_response())
        }
        Err(e) => {
            let msg = format!("Proxy error: {}", e);
            (StatusCode::BAD_GATEWAY, msg).into_response()
        }
    }
}

fn guess_mime(path: &PathBuf) -> String {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8".to_string(),
        "css" => "text/css; charset=utf-8".to_string(),
        "js" | "mjs" => "application/javascript; charset=utf-8".to_string(),
        "json" => "application/json; charset=utf-8".to_string(),
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "gif" => "image/gif".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "ico" => "image/x-icon".to_string(),
        "woff" => "font/woff".to_string(),
        "woff2" => "font/woff2".to_string(),
        "ttf" => "font/ttf".to_string(),
        "eot" => "application/vnd.ms-fontobject".to_string(),
        "webp" => "image/webp".to_string(),
        "webm" => "video/webm".to_string(),
        "mp4" => "video/mp4".to_string(),
        "wasm" => "application/wasm".to_string(),
        "map" => "application/json".to_string(),
        "xml" => "application/xml".to_string(),
        "txt" => "text/plain; charset=utf-8".to_string(),
        "pdf" => "application/pdf".to_string(),
        "ts" => "application/javascript; charset=utf-8".to_string(),
        "jsx" => "application/javascript; charset=utf-8".to_string(),
        "tsx" => "application/javascript; charset=utf-8".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}
