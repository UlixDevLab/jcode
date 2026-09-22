use crate::protocol::{ServerEvent, encode_event};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

pub(super) const CLIENT_WRITE_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub(super) enum PeerWriteError {
    TimedOut { timeout: Duration },
    Io(std::io::Error),
}

impl std::fmt::Display for PeerWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut { timeout } => write!(
                f,
                "client peer write exceeded {}ms deadline",
                timeout.as_millis()
            ),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for PeerWriteError {}

/// Serialize access to one client's socket and bound both waiting for its
/// writer lock and the actual write. A timeout may have written a partial JSON
/// frame, so callers must tear down that connection instead of reusing it.
pub(super) async fn write_encoded(
    writer: &Arc<Mutex<crate::transport::WriteHalf>>,
    bytes: &[u8],
) -> std::result::Result<(), PeerWriteError> {
    match tokio::time::timeout(CLIENT_WRITE_DEADLINE, async {
        let mut writer = writer.lock().await;
        writer.write_all(bytes).await
    })
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(PeerWriteError::Io(error)),
        Err(_) => Err(PeerWriteError::TimedOut {
            timeout: CLIENT_WRITE_DEADLINE,
        }),
    }
}

/// Project wire copies only. Persisted state and opted-in API bridges retain
/// PDF bytes, while old native clients can decode the complete History event.
pub(super) fn side_panel_for_client(
    mut snapshot: crate::side_panel::SidePanelSnapshot,
    supports_pdf_panels: bool,
) -> crate::side_panel::SidePanelSnapshot {
    if !supports_pdf_panels {
        for page in &mut snapshot.pages {
            if page.format == crate::side_panel::SidePanelPageFormat::Pdf {
                page.format = crate::side_panel::SidePanelPageFormat::Markdown;
                // This is a generated fallback, not a linked Markdown file.
                // Otherwise old TUIs re-read binary PDF bytes as text on change.
                page.source = crate::side_panel::SidePanelPageSource::Ephemeral;
            }
            page.pdf_data = None;
        }
    }
    snapshot
}

pub(super) async fn write_direct_event(
    writer: &Arc<Mutex<crate::transport::WriteHalf>>,
    event: &ServerEvent,
) -> Result<()> {
    let json = encode_event(event);
    write_encoded(writer, json.as_bytes()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{Listener, Stream};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::sync::oneshot;

    /// Public wire contract: a peer that stops reading cannot keep a server
    /// writer open indefinitely, and a timed-out partial frame is torn down.
    #[cfg(unix)]
    #[tokio::test]
    async fn non_draining_peer_write_times_out_and_teardown_reaches_eof() {
        let temp = tempfile::tempdir().expect("temp socket dir");
        let path = temp.path().join("non-draining-peer.sock");
        let listener = Listener::bind(&path).expect("bind owned listener");
        let payload = vec![b'x'; 16 * 1024 * 1024];

        // This is the pre-repair primitive exactly: a free per-peer writer
        // with no deadline. Prove the socket buffer fills before exercising
        // the bounded helper, so the later timeout cannot pass just because
        // the whole payload fit in a buffer.
        let baseline_path = path.clone();
        let baseline_peer = tokio::spawn(async move {
            let stream = Stream::connect(&baseline_path)
                .await
                .expect("connect baseline peer");
            let (_reader, _writer) = stream.into_split();
            tokio::time::sleep(Duration::from_secs(3)).await;
        });
        let (baseline_stream, _) = listener.accept().await.expect("accept baseline peer");
        let (_baseline_reader, mut baseline_writer) = baseline_stream.into_split();
        assert!(
            tokio::time::timeout(
                Duration::from_millis(200),
                baseline_writer.write_all(&payload),
            )
            .await
            .is_err(),
            "unbounded baseline write unexpectedly completed; backpressure was not established"
        );
        drop(baseline_writer);
        baseline_peer.abort();
        let _ = baseline_peer.await;

        let (start_read_tx, start_read_rx) = oneshot::channel();

        let client_path = path.clone();
        let client = tokio::spawn(async move {
            let stream = Stream::connect(&client_path).await.expect("connect peer");
            let (mut reader, _writer) = stream.into_split();
            start_read_rx.await.expect("start read after teardown");
            let mut received = Vec::new();
            tokio::time::timeout(Duration::from_secs(2), reader.read_to_end(&mut received))
                .await
                .expect("teardown read should finish")
                .expect("teardown read should not fail");
            received.len()
        });

        let (stream, _) = listener.accept().await.expect("accept peer");
        let (_reader, writer) = stream.into_split();
        let writer = Arc::new(Mutex::new(writer));

        let started = std::time::Instant::now();
        let result = write_encoded(&writer, &payload).await;
        assert!(
            matches!(result, Err(PeerWriteError::TimedOut { .. })),
            "non-draining peer must hit the delivery deadline, got {result:?}"
        );
        assert!(
            started.elapsed() >= CLIENT_WRITE_DEADLINE,
            "deadline must not fire before its configured bound"
        );

        // A timed-out write might have emitted part of the payload. Dropping
        // the owned write half is deliberate teardown, not a retry on a
        // potentially malformed stream.
        drop(writer);
        start_read_tx.send(()).expect("release peer reader");
        let received = client.await.expect("peer task");
        assert!(
            received < payload.len(),
            "peer should observe the torn-down partial frame, not a full payload"
        );
    }

    use crate::side_panel::{
        SidePanelPage, SidePanelPageFormat, SidePanelPageSource, SidePanelSnapshot,
    };

    #[test]
    fn pdf_panels_live_projection_preserves_fallback_and_opted_in_payload() {
        let snapshot = SidePanelSnapshot {
            focus_revision: 123,
            focused_page_id: Some("report".into()),
            pages: vec![SidePanelPage {
                id: "report".into(),
                title: "Report".into(),
                format: SidePanelPageFormat::Pdf,
                source: SidePanelPageSource::LinkedFile,
                content: "PDF document fallback".into(),
                pdf_data: Some("JVBERi0xLjQKJSVFT0Y=".into()),
                ..Default::default()
            }],
        };
        let opted = side_panel_for_client(snapshot.clone(), true);
        assert_eq!(opted, snapshot);
        let mut projected = side_panel_for_client(snapshot.clone(), false);
        assert_eq!(projected.focus_revision, 123);
        assert_eq!(projected.focused_page_id, snapshot.focused_page_id);
        assert_eq!(projected.pages[0].content, snapshot.pages[0].content);
        assert_eq!(projected.pages[0].format, SidePanelPageFormat::Markdown);
        assert!(projected.pages[0].pdf_data.is_none());
        assert_eq!(projected.pages[0].source, SidePanelPageSource::Ephemeral);
        assert!(!crate::side_panel::refresh_linked_page_content(
            &mut projected,
            None
        ));

        // The shipped native reader has this closed enum. Unknown fields are
        // tolerated, but a new value in this existing field is not.
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum LegacyFormat {
            Markdown,
        }
        #[derive(serde::Deserialize)]
        struct LegacyPage {
            format: LegacyFormat,
        }
        #[derive(serde::Deserialize)]
        struct LegacySnapshot {
            pages: Vec<LegacyPage>,
        }
        #[derive(serde::Deserialize)]
        struct LegacyEvent {
            snapshot: LegacySnapshot,
        }
        let wire = encode_event(&ServerEvent::SidePanelState {
            snapshot: projected,
        });
        let legacy: LegacyEvent = serde_json::from_str(&wire).unwrap();
        assert!(matches!(
            legacy.snapshot.pages[0].format,
            LegacyFormat::Markdown
        ));
        assert!(!wire.contains("pdf_data"));
        let wire = encode_event(&ServerEvent::SidePanelState { snapshot: opted });
        assert!(serde_json::from_str::<LegacyEvent>(&wire).is_err());
        assert!(wire.contains("JVBERi0xLjQKJSVFT0Y="));
    }
}
