//! Recorded HTTP: a local `POST /exec` server asserts the documented request shape.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use cyclone_ext_exe::{ExeDevClient, EXEC_BODY_LIMIT};

struct CapturedRequest {
    start_line: String,
    authorization: Option<String>,
    content_type: Option<String>,
    body: String,
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let line = line.trim();
        if line.to_ascii_lowercase().starts_with("content-length:") {
            return line.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

fn spawn_exec_stub(
    status: u16,
    response_body: &str,
) -> (String, thread::JoinHandle<CapturedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let response_body = response_body.to_string();
    let handle = thread::spawn(move || {
        listener.set_nonblocking(false).expect("blocking");
        let (mut stream, _) = listener.accept().expect("accept");
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        let mut buf = Vec::new();
        loop {
            let mut tmp = [0u8; 2048];
            match stream.read(&mut tmp) {
                Ok(0) => break,
                Ok(n) => buf.extend_from_slice(&tmp[..n]),
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => panic!("read: {e}"),
            }
            if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let header_end = pos + 4;
                if let Some(len) =
                    content_length(std::str::from_utf8(&buf[..header_end]).unwrap_or(""))
                {
                    if buf.len() >= header_end + len {
                        break;
                    }
                } else if buf.len() >= header_end {
                    break;
                }
            }
        }
        let raw = String::from_utf8_lossy(&buf);
        let (header_part, body) = raw.split_once("\r\n\r\n").unwrap_or((raw.as_ref(), ""));
        let mut lines = header_part.lines();
        let start_line = lines.next().unwrap_or("").to_string();
        let mut authorization = None;
        let mut content_type = None;
        for line in lines {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("authorization:") {
                authorization = line.split_once(':').map(|(_, v)| v.trim().to_string());
            }
            if lower.starts_with("content-type:") {
                content_type = line.split_once(':').map(|(_, v)| v.trim().to_string());
            }
        }
        let reason = if (200..300).contains(&status) {
            "OK"
        } else {
            "ERR"
        };
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response_body}",
            response_body.len()
        );
        stream.write_all(resp.as_bytes()).ok();
        stream.flush().ok();
        CapturedRequest {
            start_line,
            authorization,
            content_type,
            body: body.to_string(),
        }
    });
    (format!("http://{addr}/exec"), handle)
}

#[test]
fn ureq_client_posts_documented_exec_request() {
    let ls_body = include_str!("fixtures/ls_empty.json");
    let (url, handle) = spawn_exec_stub(200, ls_body);
    let client = ExeDevClient::new(url, "fixture-token-not-a-real-secret");
    let resp = client.exec("ls --json dev-box").expect("exec");
    assert_eq!(resp.status, 200);
    assert!(resp.body.contains("\"vms\""));

    let captured = handle.join().expect("server thread");
    assert!(
        captured.start_line.starts_with("POST /exec"),
        "start line: {}",
        captured.start_line
    );
    assert_eq!(
        captured.authorization.as_deref(),
        Some("Bearer fixture-token-not-a-real-secret")
    );
    assert!(
        captured
            .content_type
            .as_deref()
            .unwrap_or("")
            .starts_with("text/plain"),
        "content-type: {:?}",
        captured.content_type
    );
    assert_eq!(
        captured.body.trim_end_matches('\0').trim(),
        "ls --json dev-box"
    );
}

#[test]
fn ureq_client_maps_401_without_echoing_token() {
    let (url, handle) = spawn_exec_stub(401, "invalid token");
    let token = "exe1.this-must-not-appear-in-errors";
    let client = ExeDevClient::new(url, token);
    let err = client.exec("whoami").expect_err("401");
    let msg = err.to_string();
    assert!(msg.contains("401"));
    assert!(!msg.contains("this-must-not-appear"));
    assert!(!msg.contains(token));
    let _ = handle.join();
}

#[test]
fn client_rejects_oversize_command_locally() {
    let client = ExeDevClient::new("https://exe.dev/exec", "fixture-token");
    let huge = "x".repeat(EXEC_BODY_LIMIT + 1);
    let err = client.exec(&huge).expect_err("limit");
    let msg = err.to_string();
    assert!(
        msg.contains("body limit") && msg.contains(&EXEC_BODY_LIMIT.to_string()),
        "{msg}"
    );
}
