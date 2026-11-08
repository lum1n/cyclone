use cyclone_core::CycloneError;
use thiserror::Error;

/// Errors from the exe.dev HTTPS client and provision flow.
///
/// Messages never include bearer tokens or decrypted file bytes.
#[derive(Debug, Error)]
pub enum ExeDevError {
    #[error(
        "missing {0}; generate a token with `ssh exe.dev ssh-key generate-api-key --exp=30d --cmds=ls,new,ssh --label cyclone` and export it (the official env var used by the exe.dev Python SDK)"
    )]
    MissingToken(&'static str),

    #[error(
        "invalid target VM name `{0}`; use a lowercase DNS label (e.g. my-vm), 1–63 characters"
    )]
    InvalidTarget(String),

    #[error("exe.dev HTTP {status}: {message}")]
    Http { status: u16, message: String },

    #[error("exe.dev transport: {0}")]
    Transport(String),

    #[error("{0}")]
    Message(String),
}

impl From<ExeDevError> for CycloneError {
    fn from(err: ExeDevError) -> Self {
        CycloneError::User(err.to_string())
    }
}

/// Replace a known secret in an error string. Never log the original.
pub(crate) fn sanitize(message: &str, token: &str) -> String {
    let mut out = message.to_string();
    if !token.is_empty() {
        out = out.replace(token, "***");
    }
    if let Some(rest) = out.split_once("Bearer ") {
        // Collapse `Bearer <anything>` fragments that might appear in HTTP dumps.
        if rest.1.starts_with("***") {
            return out;
        }
        let after = rest.1.split(|c: char| c.is_whitespace() || c == '"').next();
        if let Some(tok) = after {
            if tok.len() > 8 {
                out = out.replace(tok, "***");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_token_and_bearer() {
        let token = "exe1.super-secret-fixture";
        let msg = format!("Authorization: Bearer {token} failed");
        let out = sanitize(&msg, token);
        assert!(!out.contains("super-secret"));
        assert!(out.contains("***"));
    }
}
