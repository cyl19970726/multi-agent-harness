use super::*;

pub(super) struct HttpResponseWriter<W> {
    inner: W,
    bytes_written: Arc<std::sync::atomic::AtomicUsize>,
}

impl<W> HttpResponseWriter<W> {
    pub(super) fn new(inner: W) -> Self {
        Self {
            inner,
            bytes_written: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    pub(super) fn bytes_written(&self) -> usize {
        self.bytes_written.load(Ordering::Relaxed)
    }
}

impl<W: Write> Write for HttpResponseWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.inner.write(buf)?;
        self.bytes_written.fetch_add(written, Ordering::Relaxed);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl HttpResponseWriter<TcpStream> {
    pub(super) fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.inner.local_addr()
    }

    pub(super) fn peer_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        self.inner.peer_addr()
    }

    pub(super) fn try_clone(&self) -> std::io::Result<Self> {
        Ok(Self {
            inner: self.inner.try_clone()?,
            bytes_written: Arc::clone(&self.bytes_written),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AllowedDocPathKind {
    DocsTree,
    RootDoc,
}

pub(super) fn allowed_doc_path_kind(decoded: &str) -> Result<AllowedDocPathKind, String> {
    if decoded.contains("..") {
        return Err(format!("path must contain no ..: {decoded}"));
    }
    if decoded.starts_with("docs/") {
        return Ok(AllowedDocPathKind::DocsTree);
    }
    if matches!(decoded, "README.md" | "AGENTS.md") {
        return Ok(AllowedDocPathKind::RootDoc);
    }
    Err(format!(
        "path must be under docs/ or be README.md/AGENTS.md: {decoded}"
    ))
}

/// Resolve a `GET /v1/docs?path=...` request to a repository doc body. The route
/// serves the `docs/` tree plus root `README.md` / `AGENTS.md`, and rejects path
/// traversal so Docs can expose project entrypoints without exposing arbitrary
/// repository files.
pub(super) fn read_allowed_doc(request_target: &str) -> Result<(String, String), String> {
    let query = request_target.split('?').nth(1).unwrap_or("");
    let raw = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("path="))
        .ok_or_else(|| "missing ?path= parameter".to_string())?;
    // Minimal percent-decoding (paths are simple: slashes + alnum + .-_).
    let decoded = raw
        .replace("%2F", "/")
        .replace("%2f", "/")
        .replace("%20", " ");
    let path_kind = allowed_doc_path_kind(&decoded)?;
    let base = std::env::current_dir()
        .and_then(|dir| dir.canonicalize())
        .map_err(|error| format!("cannot resolve working dir: {error}"))?;
    let full = base
        .join(&decoded)
        .canonicalize()
        .map_err(|error| format!("doc not found: {decoded} ({error})"))?;
    match path_kind {
        AllowedDocPathKind::DocsTree => {
            let docs_root = base
                .join("docs")
                .canonicalize()
                .map_err(|error| format!("cannot resolve docs/: {error}"))?;
            if !full.starts_with(&docs_root) {
                return Err(format!("resolved path escapes docs/: {decoded}"));
            }
        }
        AllowedDocPathKind::RootDoc => {
            if full.parent() != Some(base.as_path()) {
                return Err(format!("resolved path escapes repository root: {decoded}"));
            }
        }
    }
    let content =
        std::fs::read_to_string(&full).map_err(|error| format!("read failed: {error}"))?;
    Ok((decoded, content))
}

pub(super) fn write_http_json<T: serde::Serialize, W: Write>(
    stream: &mut W,
    status: &str,
    value: &T,
) -> CliResult<()> {
    let body = serde_json::to_vec_pretty(value).expect("serialize http json");
    write_http_response(stream, status, "application/json", &body)
}

pub(super) fn write_http_response<W: Write>(
    stream: &mut W,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> CliResult<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

pub(super) fn write_http_error_if_unstarted<W: Write>(
    stream: &mut HttpResponseWriter<W>,
    error: &CliError,
) -> CliResult<bool> {
    if stream.bytes_written() != 0 {
        return Ok(false);
    }
    let (status, body) = http_action_error_response(error);
    write_http_json(stream, status, &body)?;
    Ok(true)
}

#[cfg(test)]
mod tests_http_response_writer {
    use super::*;

    #[test]
    fn handler_error_after_response_started_does_not_write_a_second_status_line() {
        let mut stream = HttpResponseWriter::new(Vec::new());
        stream
            .write_all(b"HTTP/1.1 200 OK\r\n")
            .expect("write partial response");

        let error = CliError::Store(StoreError::LockTimeout("/tmp/store/.store.lock".into()));
        let answered = write_http_error_if_unstarted(&mut stream, &error).expect("guard response");

        assert!(!answered);
        assert_eq!(stream.inner, b"HTTP/1.1 200 OK\r\n");
    }
}
