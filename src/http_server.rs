use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{sync_channel, Sender, SyncSender},
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{info, warn};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::ime::ImeMode;

pub const PORT: u16 = 5998;
const BIND_HOST: &str = "0.0.0.0";
const RESPONSE_WAIT_TIMEOUT: Duration = Duration::from_secs(3);
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub struct HttpServer {
    stop_flag: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl HttpServer {
    pub fn stop(mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(join_handle) = self.join_handle.take() {
            let _ = join_handle.join();
        }
    }
}

pub struct SwitchRequest {
    pub desired_mode: ImeMode,
    response_sender: SyncSender<SwitchResponse>,
}

impl SwitchRequest {
    fn new(desired_mode: ImeMode, response_sender: SyncSender<SwitchResponse>) -> Self {
        Self {
            desired_mode,
            response_sender,
        }
    }

    pub fn respond(self, response: SwitchResponse) {
        let _ = self.response_sender.send(response);
    }
}

#[derive(Clone, Debug)]
pub struct SwitchResponse {
    pub success: bool,
    pub changed: bool,
    pub message: String,
    pub mode: Option<ImeMode>,
}

impl SwitchResponse {
    pub fn success(mode: ImeMode, changed: bool, message: impl Into<String>) -> Self {
        Self {
            success: true,
            changed,
            message: message.into(),
            mode: Some(mode),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            changed: false,
            message: message.into(),
            mode: None,
        }
    }
}

pub fn start(sender: Sender<SwitchRequest>, hwnd: HWND, message_id: u32) -> Result<HttpServer> {
    let bind_addr = format!("{BIND_HOST}:{PORT}");
    let listener = TcpListener::bind(&bind_addr)
        .with_context(|| format!("failed to bind HTTP server on {bind_addr}"))?;
    listener
        .set_nonblocking(true)
        .context("failed to set HTTP listener nonblocking")?;

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_flag_for_thread = Arc::clone(&stop_flag);
    let hwnd_raw = hwnd.0 as isize;

    let join_handle = thread::spawn(move || {
        info!("HTTP server listening on {}", bind_addr);

        while !stop_flag_for_thread.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = handle_connection(stream, &sender, hwnd_raw, message_id) {
                        warn!(?error, "failed to handle HTTP request");
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
                Err(error) => {
                    warn!(?error, "HTTP accept failed");
                    thread::sleep(ACCEPT_POLL_INTERVAL);
                }
            }
        }
    });

    Ok(HttpServer {
        stop_flag,
        join_handle: Some(join_handle),
    })
}

fn handle_connection(
    stream: TcpStream,
    sender: &Sender<SwitchRequest>,
    hwnd_raw: isize,
    message_id: u32,
) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .context("failed to read request line")?;

    let response = match parse_requested_mode(&request_line) {
        Ok(mode) => forward_switch_request(sender, mode, hwnd_raw, message_id),
        Err(message) => build_http_response(400, &SwitchResponse::error(message)),
    };

    let mut stream = reader.into_inner();
    stream
        .write_all(response.as_bytes())
        .context("failed to write HTTP response")?;
    stream.flush().context("failed to flush HTTP response")?;

    Ok(())
}

fn forward_switch_request(
    sender: &Sender<SwitchRequest>,
    desired_mode: ImeMode,
    hwnd_raw: isize,
    message_id: u32,
) -> String {
    let (response_sender, response_receiver) = sync_channel(1);
    let request = SwitchRequest::new(desired_mode, response_sender);

    if sender.send(request).is_err() {
        return build_http_response(500, &SwitchResponse::error("应用未在处理 HTTP 切换请求"));
    }

    unsafe {
        let _ = PostMessageW(HWND(hwnd_raw as _), message_id, WPARAM(0), LPARAM(0));
    }

    match response_receiver.recv_timeout(RESPONSE_WAIT_TIMEOUT) {
        Ok(response) => build_http_response(200, &response),
        Err(_) => build_http_response(504, &SwitchResponse::error("等待输入法切换结果超时")),
    }
}

fn parse_requested_mode(request_line: &str) -> std::result::Result<ImeMode, &'static str> {
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().ok_or("缺少 HTTP 方法")?;
    let path = parts.next().ok_or("缺少请求路径")?;

    if let Some(mode) = parse_mode_from_path(path) {
        return Ok(mode);
    }

    if let Some((_, query)) = path.split_once('?') {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if matches!(key, "mode" | "value") {
                return parse_mode_value(value).ok_or("mode 只能传 1 或 0");
            }
        }
    }

    Err("请使用 /switch?mode=1 或 /switch?mode=0")
}

fn parse_mode_from_path(path: &str) -> Option<ImeMode> {
    if let Some(value) = path.strip_prefix("/switch/") {
        return parse_mode_value(value);
    }

    if let Some(value) = path.strip_prefix('/') {
        return parse_mode_value(value);
    }

    None
}

fn parse_mode_value(value: &str) -> Option<ImeMode> {
    match value {
        "1" => Some(ImeMode::Chinese),
        "0" => Some(ImeMode::English),
        _ => None,
    }
}

fn build_http_response(status_code: u16, response: &SwitchResponse) -> String {
    let status_text = match status_code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        504 => "Gateway Timeout",
        _ => "OK",
    };
    let mode = match response.mode {
        Some(ImeMode::Chinese) => "\"chinese\"".to_string(),
        Some(ImeMode::English) => "\"english\"".to_string(),
        Some(ImeMode::Unknown) => "\"unknown\"".to_string(),
        None => "null".to_string(),
    };
    let body = format!(
        "{{\"success\":{},\"changed\":{},\"mode\":{},\"message\":\"{}\"}}",
        response.success,
        response.changed,
        mode,
        json_escape(&response.message),
    );

    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status_code,
        status_text,
        body.as_bytes().len(),
        body,
    )
}

fn json_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}
