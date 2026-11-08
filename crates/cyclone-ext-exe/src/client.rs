use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::error::{sanitize, ExeDevError};

/// Documented HTTPS API endpoint (`POST` body = SSH/CLI command).
/// See <https://exe.dev/docs/https-api>.
pub const DEFAULT_EXEC_URL: &str = "https://exe.dev/exec";

/// Official env var used by the exe.dev Python SDK (`exedev.Client()`).
pub const TOKEN_ENV: &str = "EXE_TOKEN";

/// Optional override of [`DEFAULT_EXEC_URL`] (tests, proxies). Not a secret.
pub const EXEC_URL_ENV: &str = "EXE_DEV_EXEC_URL";

/// Documented `POST /exec` body limit.
pub const EXEC_BODY_LIMIT: usize = 64 * 1024;

/// Documented command timeout (the API returns HTTP 504 after 30s).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(29);

#[derive(Debug, Clone)]
pub struct ExecResponse {
    pub status: u16,
    pub body: String,
}

/// HTTP POST seam so tests can replay fixture responses without a network.
pub trait ExecTransport: Send + Sync {
    fn post(&self, url: &str, bearer_token: &str, body: &str) -> Result<ExecResponse, ExeDevError>;
}

/// `ureq` transport for `POST {url}` with `Authorization: Bearer …`.
#[derive(Debug, Default, Clone, Copy)]
pub struct UreqTransport;

impl ExecTransport for UreqTransport {
    fn post(&self, url: &str, bearer_token: &str, body: &str) -> Result<ExecResponse, ExeDevError> {
        let auth = format!("Bearer {bearer_token}");
        let result = ureq::post(url)
            .set("Authorization", &auth)
            .set("Content-Type", "text/plain; charset=utf-8")
            .set("User-Agent", concat!("cyclone/", env!("CARGO_PKG_VERSION")))
            .timeout(REQUEST_TIMEOUT)
            .send_string(body);
        match result {
            Ok(resp) => read_ureq(resp, bearer_token),
            Err(ureq::Error::Status(_, resp)) => read_ureq(resp, bearer_token),
            Err(err) => Err(ExeDevError::Transport(sanitize(
                &err.to_string(),
                bearer_token,
            ))),
        }
    }
}

fn read_ureq(resp: ureq::Response, token: &str) -> Result<ExecResponse, ExeDevError> {
    let status = resp.status();
    let body = resp.into_string().unwrap_or_default();
    Ok(ExecResponse {
        status,
        body: sanitize(&body, token),
    })
}

/// Bearer token wrapper. `Debug` does not print the secret; dropped bytes are zeroized.
struct BearerToken(String);

impl Drop for BearerToken {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.0.zeroize();
    }
}

impl std::fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BearerToken(***)")
    }
}

/// Client for `POST https://exe.dev/exec` (SSH API over HTTPS).
pub struct ExeDevClient {
    endpoint: String,
    token: BearerToken,
    transport: Arc<dyn ExecTransport>,
}

impl std::fmt::Debug for ExeDevClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExeDevClient")
            .field("endpoint", &self.endpoint)
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl ExeDevClient {
    pub fn new(endpoint: impl Into<String>, token: impl Into<String>) -> Self {
        Self::with_transport(endpoint, token, Arc::new(UreqTransport))
    }

    pub fn with_transport(
        endpoint: impl Into<String>,
        token: impl Into<String>,
        transport: Arc<dyn ExecTransport>,
    ) -> Self {
        Self {
            endpoint: endpoint.into(),
            token: BearerToken(token.into()),
            transport,
        }
    }

    pub fn from_env() -> Result<Self, ExeDevError> {
        let token = std::env::var(TOKEN_ENV).unwrap_or_default();
        let token = token.trim();
        if token.is_empty() {
            return Err(ExeDevError::MissingToken(TOKEN_ENV));
        }
        let endpoint = std::env::var(EXEC_URL_ENV)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_EXEC_URL.to_string());
        Ok(Self::new(endpoint, token))
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Run one lobby command. JSON is always enabled on this endpoint (docs).
    pub fn exec(&self, command: &str) -> Result<ExecResponse, ExeDevError> {
        if command.is_empty() {
            return Err(ExeDevError::Message("empty exe.dev command".into()));
        }
        if command.len() > EXEC_BODY_LIMIT {
            return Err(ExeDevError::Message(format!(
                "command exceeds documented POST /exec body limit of {EXEC_BODY_LIMIT} bytes"
            )));
        }
        tracing::debug!(endpoint = %self.endpoint, command_kind = command_kind(command), "exe.dev exec");
        let resp = self
            .transport
            .post(&self.endpoint, &self.token.0, command)?;
        match resp.status {
            200..=299 => {
                if resp.body.contains("Please complete registration by running: ssh exe.dev")
                {
                    return Err(ExeDevError::Message(
                        "exe.dev account is not registered; run `ssh exe.dev` interactively first"
                            .into(),
                    ));
                }
                Ok(resp)
            }
            401 => Err(ExeDevError::Http {
                status: 401,
                message: "invalid or expired token (HTTP 401). Mint a new one with `ssh exe.dev ssh-key generate-api-key`".into(),
            }),
            403 => Err(ExeDevError::Http {
                status: 403,
                message: format!(
                    "command not allowed by token --cmds (HTTP 403). Provision needs ls,new,ssh. {}",
                    truncate(&resp.body, 200)
                ),
            }),
            404 => Err(ExeDevError::Http {
                status: 404,
                message: format!("not found (HTTP 404): {}", truncate(&resp.body, 200)),
            }),
            413 => Err(ExeDevError::Http {
                status: 413,
                message: "request too large (HTTP 413); POST /exec is limited to 64KiB".into(),
            }),
            422 => Err(ExeDevError::Http {
                status: 422,
                message: format!("command failed (HTTP 422): {}", truncate(&resp.body, 400)),
            }),
            429 => Err(ExeDevError::Http {
                status: 429,
                message: "rate limited (HTTP 429)".into(),
            }),
            504 => Err(ExeDevError::Http {
                status: 504,
                message: "exe.dev command timed out (HTTP 504; 30s limit)".into(),
            }),
            status => Err(ExeDevError::Http {
                status,
                message: truncate(&resp.body, 400),
            }),
        }
    }
}

fn command_kind(command: &str) -> &str {
    command.split_whitespace().next().unwrap_or("unknown")
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let clipped: String = s.chars().take(max).collect();
        format!("{clipped}…")
    }
}

/// Parsed VM row. `ls --json` is fully documented; `new --json` is only partially
/// shown in public docs, so we accept the `ls` shape plus obvious aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmInfo {
    pub name: String,
    pub status: Option<String>,
    pub https_url: Option<String>,
    pub ssh_dest: Option<String>,
}

pub fn parse_vms(body: &str) -> Result<Vec<VmInfo>, ExeDevError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if trimmed.contains("Please complete registration") {
        return Err(ExeDevError::Message(
            "exe.dev account is not registered; run `ssh exe.dev` interactively first".into(),
        ));
    }
    let value: Value = serde_json::from_str(trimmed).map_err(|e| {
        ExeDevError::Message(format!(
            "exe.dev returned non-JSON ({e}): {}",
            truncate(trimmed, 200)
        ))
    })?;
    let objects: Vec<Value> = if let Some(arr) = value.get("vms").and_then(|v| v.as_array()) {
        arr.clone()
    } else if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if value.get("vm").is_some() {
        vec![value.get("vm").cloned().unwrap_or(Value::Null)]
    } else if value.get("vm_name").is_some() || value.get("name").is_some() {
        vec![value]
    } else {
        Vec::new()
    };
    Ok(objects.into_iter().filter_map(vm_from_value).collect())
}

fn vm_from_value(value: Value) -> Option<VmInfo> {
    let name = value
        .get("vm_name")
        .or_else(|| value.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string)?;
    Some(VmInfo {
        name,
        status: value
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        https_url: value
            .get("https_url")
            .or_else(|| value.get("url"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        ssh_dest: value
            .get("ssh_dest")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

pub fn find_vm<'a>(vms: &'a [VmInfo], name: &str) -> Option<&'a VmInfo> {
    vms.iter()
        .find(|vm| vm.name == name || vm.ssh_dest.as_deref() == Some(name))
}

/// Queued HTTP responses for tests (records commands, never talks to exe.dev).
#[derive(Debug, Default)]
pub struct MockExecTransport {
    inner: Mutex<MockInner>,
}

#[derive(Debug, Default)]
struct MockInner {
    recorded: Vec<String>,
    responses: Vec<ExecResponse>,
}

impl MockExecTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, status: u16, body: impl Into<String>) {
        self.inner
            .lock()
            .expect("mock mutex")
            .responses
            .push(ExecResponse {
                status,
                body: body.into(),
            });
    }

    pub fn recorded(&self) -> Vec<String> {
        self.inner.lock().expect("mock mutex").recorded.clone()
    }
}

impl ExecTransport for MockExecTransport {
    fn post(
        &self,
        _url: &str,
        _bearer_token: &str,
        body: &str,
    ) -> Result<ExecResponse, ExeDevError> {
        let mut inner = self.inner.lock().expect("mock mutex");
        inner.recorded.push(body.to_string());
        if inner.responses.is_empty() {
            return Err(ExeDevError::Message(format!(
                "mock exe.dev transport has no remaining responses (command: {})",
                command_kind(body)
            )));
        }
        Ok(inner.responses.remove(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_documented_ls_shape() {
        let body = r#"{
            "vms": [{
                "https_url": "https://bloggy.exe.xyz",
                "region": "lon",
                "ssh_dest": "bloggy.exe.xyz",
                "ssh_host": "bloggy.exe.xyz",
                "status": "running",
                "vm_name": "bloggy"
            }]
        }"#;
        let vms = parse_vms(body).unwrap();
        assert_eq!(vms[0].name, "bloggy");
        assert_eq!(vms[0].status.as_deref(), Some("running"));
    }

    #[test]
    fn parse_new_object_without_vms_wrapper() {
        let body = r#"{"vm_name":"example-vm","https_url":"https://example-vm.exe.xyz","status":"running"}"#;
        let vms = parse_vms(body).unwrap();
        assert_eq!(vms[0].name, "example-vm");
    }
}
