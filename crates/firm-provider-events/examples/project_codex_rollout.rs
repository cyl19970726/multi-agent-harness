use std::path::PathBuf;

use firm_provider_events::{
    read_transcript_batch, DecodeContext, DecodeOutcome, ProviderEventFold, ProviderKind,
    TranscriptReadBoundary, TransientReadPosition,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(
        args.next()
            .ok_or("usage: project_codex_rollout <root> <file>")?,
    );
    let file = PathBuf::from(
        args.next()
            .ok_or("usage: project_codex_rollout <root> <file>")?,
    );
    let context = DecodeContext {
        provider: ProviderKind::Codex,
        native_source_ref: "provider-source:local-codex-dogfood:redacted".into(),
        agent_identity_id: "dogfood-agent".into(),
        agent_session_id: "dogfood-session".into(),
        agent_session_generation: 1,
        node_daemon_id: "dogfood-daemon".into(),
        node_daemon_generation: 1,
        provider_thread_id: None,
        runtime_command_id: None,
        observed_at: "dogfood-observed".into(),
    };
    let batch = read_transcript_batch(
        &context,
        &TranscriptReadBoundary {
            allowed_root: root,
            transcript_path: file,
        },
        TransientReadPosition::default(),
        10_000,
    )?;
    let mut fold = ProviderEventFold::new("dogfood-session", 1, "dogfood-daemon", 1);
    let mut dropped = 0usize;
    let mut unsupported = 0usize;
    for outcome in batch.outcomes {
        match outcome {
            DecodeOutcome::Observation(observation) => {
                fold.ingest(*observation)?;
            }
            DecodeOutcome::DroppedPrivate => dropped += 1,
            DecodeOutcome::Unsupported => unsupported += 1,
        }
    }
    let projection = fold.session_projection(300);
    let observations = projection
        .episodes
        .iter()
        .map(|episode| episode.observations.len())
        .sum::<usize>();
    println!(
        "provider=codex observations={observations} episodes={} dropped_private={dropped} unsupported={unsupported} truncated={} incomplete_tail={} source_snapshot_fingerprint={}",
        projection.episodes.len(),
        projection.truncated,
        batch.incomplete_tail,
        projection.source_snapshot_fingerprint
    );
    Ok(())
}
