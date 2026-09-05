use super::*;
use harness_core::agentfirm_api::WorkspaceLifecycle;
use serde::Serialize;

pub(super) const SOURCE_VIEWER_MAX_BYTES: u64 = 512 * 1024;

#[derive(Debug, Serialize)]
pub(super) struct SourceViewerResponse {
    kind: &'static str,
    path: String,
    size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
}

impl SourceViewerResponse {
    fn unavailable(kind: &'static str, path: String, line: Option<u64>) -> Self {
        Self {
            kind,
            path,
            size: 0,
            line,
            content: None,
        }
    }
}

pub(super) fn source_viewer_response(
    request_target: &str,
    project: &ProjectContext,
    store: &HarnessStore,
    execution_space_id: &str,
) -> Result<SourceViewerResponse, String> {
    let query = request_target
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or("");
    let query_value = |name: &str| {
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then_some(value)
        })
    };
    let raw_path = query_value("path").ok_or_else(|| "missing ?path= parameter".to_string())?;
    let requested_path = percent_decode_query_value(raw_path)?;
    let line = query_value("line")
        .map(percent_decode_query_value)
        .transpose()?
        .map(|value| value.parse::<u64>())
        .transpose()
        .map_err(|_| "line must be a positive integer".to_string())?;
    if line == Some(0) {
        return Err("line must be a positive integer".to_string());
    }

    let mut roots = vec![project.project_root.clone()];
    roots.extend(
        store
            .trust_workspace_bindings(execution_space_id)
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|binding| {
                binding.project_binding_id == project.id
                    && binding.lifecycle == WorkspaceLifecycle::Attached
            })
            .map(|binding| PathBuf::from(binding.canonical_root)),
    );
    match resolve_workspace_file(Path::new(&requested_path), &project.project_root, &roots)? {
        WorkspaceFileResolution::Missing => Ok(SourceViewerResponse::unavailable(
            "missing",
            requested_path,
            line,
        )),
        WorkspaceFileResolution::OutsideWorkspace => Ok(SourceViewerResponse::unavailable(
            "outside_workspace",
            requested_path,
            line,
        )),
        WorkspaceFileResolution::File(path) => {
            let metadata = path
                .metadata()
                .map_err(|error| format!("read metadata: {error}"))?;
            let display_path = path.to_string_lossy().into_owned();
            if metadata.len() > SOURCE_VIEWER_MAX_BYTES {
                return Ok(SourceViewerResponse {
                    kind: "binary",
                    path: display_path,
                    size: metadata.len(),
                    line,
                    content: None,
                });
            }
            let mut bytes = Vec::with_capacity(metadata.len() as usize);
            std::fs::File::open(&path)
                .map_err(|error| format!("open source: {error}"))?
                .take(SOURCE_VIEWER_MAX_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("read source: {error}"))?;
            if bytes.len() as u64 > SOURCE_VIEWER_MAX_BYTES {
                return Ok(SourceViewerResponse {
                    kind: "binary",
                    path: display_path,
                    size: bytes.len() as u64,
                    line,
                    content: None,
                });
            }
            let size = bytes.len() as u64;
            if bytes.contains(&0) {
                return Ok(SourceViewerResponse {
                    kind: "binary",
                    path: display_path,
                    size,
                    line,
                    content: None,
                });
            }
            let Ok(content) = String::from_utf8(bytes) else {
                return Ok(SourceViewerResponse {
                    kind: "binary",
                    path: display_path,
                    size,
                    line,
                    content: None,
                });
            };
            let markdown = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(extension.to_ascii_lowercase().as_str(), "md" | "markdown")
                });
            Ok(SourceViewerResponse {
                kind: if markdown { "markdown" } else { "text" },
                path: display_path,
                size,
                line,
                content: Some(content),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "firm-source-viewer-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).expect("create test dir");
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn workspace_resolver_enforces_canonical_boundary_and_missing_semantics() {
        let root = TestDir::new();
        let outside = TestDir::new();
        std::fs::write(root.0.join("guide.md"), "one\ntwo\nthree\n").expect("write guide");
        std::fs::write(outside.0.join("secret.txt"), "secret").expect("write secret");

        assert!(matches!(
            resolve_workspace_file(
                Path::new("../secret.txt"),
                &root.0,
                std::slice::from_ref(&root.0)
            )
            .unwrap(),
            WorkspaceFileResolution::OutsideWorkspace
        ));
        assert!(matches!(
            resolve_workspace_file(
                &outside.0.join("secret.txt"),
                &root.0,
                std::slice::from_ref(&root.0)
            )
            .unwrap(),
            WorkspaceFileResolution::OutsideWorkspace
        ));
        assert!(matches!(
            resolve_workspace_file(
                Path::new("missing.md"),
                &root.0,
                std::slice::from_ref(&root.0)
            )
            .unwrap(),
            WorkspaceFileResolution::Missing
        ));
        assert!(matches!(
            resolve_workspace_file(
                Path::new("guide.md"),
                &root.0,
                std::slice::from_ref(&root.0)
            )
            .unwrap(),
            WorkspaceFileResolution::File(_)
        ));
        std::fs::create_dir(root.0.join("folder")).expect("create directory target");
        assert!(matches!(
            resolve_workspace_file(Path::new("folder"), &root.0, std::slice::from_ref(&root.0))
                .unwrap(),
            WorkspaceFileResolution::OutsideWorkspace
        ));

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.0.join("secret.txt"), root.0.join("escape.txt"))
                .expect("create symlink");
            assert!(matches!(
                resolve_workspace_file(
                    Path::new("escape.txt"),
                    &root.0,
                    std::slice::from_ref(&root.0)
                )
                .unwrap(),
                WorkspaceFileResolution::OutsideWorkspace
            ));
        }
    }

    #[test]
    fn source_response_preserves_line_and_bounds_content() {
        let root = TestDir::new();
        let store_root = TestDir::new();
        std::fs::write(root.0.join("guide.md"), "one\ntwo\nthree\n").expect("write guide");
        std::fs::write(
            root.0.join("oversized.txt"),
            vec![b'x'; SOURCE_VIEWER_MAX_BYTES as usize + 1],
        )
        .expect("write oversized source");
        std::fs::write(root.0.join("binary.dat"), b"text\0binary").expect("write binary source");
        let store = HarnessStore::new(store_root.0.clone());
        store.init().expect("init store");
        let project = ProjectContext {
            id: "project-one".into(),
            project_root: root.0.clone(),
            store_root: store_root.0.clone(),
            kind: ProjectKind::Repo,
            is_git_repo: false,
        };

        let guide = source_viewer_response(
            "/v1/projects/project-one/source?path=guide.md&line=2",
            &project,
            &store,
            "space-one",
        )
        .expect("resolve guide");
        assert_eq!(guide.kind, "markdown");
        assert_eq!(guide.line, Some(2));
        assert_eq!(guide.content.as_deref(), Some("one\ntwo\nthree\n"));

        let oversized = source_viewer_response(
            "/v1/projects/project-one/source?path=oversized.txt",
            &project,
            &store,
            "space-one",
        )
        .expect("resolve oversized source");
        assert_eq!(oversized.kind, "binary");
        assert_eq!(oversized.size, SOURCE_VIEWER_MAX_BYTES + 1);
        assert!(oversized.content.is_none());

        let binary = source_viewer_response(
            "/v1/projects/project-one/source?path=binary.dat",
            &project,
            &store,
            "space-one",
        )
        .expect("resolve binary source");
        assert_eq!(binary.kind, "binary");
        assert!(binary.content.is_none());
    }
}
