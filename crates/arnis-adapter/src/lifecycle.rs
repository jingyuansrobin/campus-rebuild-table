use super::{discover_java_world, ArnisAdapter, ArnisError, ArnisRunResult, ArnisRunSpec};
use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct ArnisCancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl ArnisCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArnisLogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArnisStage {
    PreparingData,
    ProcessingMap,
    GeneratingWorld,
    SavingWorld,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArnisEvent {
    Stage(ArnisStage),
    Log {
        stream: ArnisLogStream,
        line: String,
    },
}

enum PipeMessage {
    Line {
        stream: ArnisLogStream,
        line: String,
    },
    ReadError {
        stream: ArnisLogStream,
        source: std::io::Error,
    },
}

impl ArnisAdapter {
    /// Runs Arnis with piped stdout/stderr so callers can observe coarse stages and raw logs.
    ///
    /// The stage mapping is deliberately coarse. Arnis CLI does not expose its GUI's precise
    /// progress protocol, and its numbered human-readable stages can arrive out of order while
    /// data/elevation fetches run in parallel. Consumers must not turn these stages into a fake
    /// numeric percentage.
    ///
    /// Cancellation currently kills the direct Arnis child. Some platforms/downloaders may spawn
    /// descendants; process-tree cancellation can be added later if real-world evidence requires it.
    pub fn run_with_events<F>(
        &self,
        spec: &ArnisRunSpec,
        cancellation: &ArnisCancellationToken,
        mut on_event: F,
    ) -> Result<ArnisRunResult, ArnisError>
    where
        F: FnMut(ArnisEvent),
    {
        if cancellation.is_cancelled() {
            return Err(ArnisError::Cancelled);
        }

        std::fs::create_dir_all(&spec.output_dir).map_err(|source| ArnisError::PrepareOutput {
            output_dir: spec.output_dir.clone(),
            source,
        })?;

        let plan = self.plan(spec);
        let mut child = Command::new(plan.executable())
            .args(plan.args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| ArnisError::Launch {
                executable: plan.executable().to_path_buf(),
                source,
            })?;

        let stdout = child
            .stdout
            .take()
            .ok_or(ArnisError::MissingOutputPipe { stream: "stdout" })?;
        let stderr = child
            .stderr
            .take()
            .ok_or(ArnisError::MissingOutputPipe { stream: "stderr" })?;

        let (tx, rx) = mpsc::channel();
        let stdout_reader = spawn_pipe_reader(stdout, ArnisLogStream::Stdout, tx.clone());
        let stderr_reader = spawn_pipe_reader(stderr, ArnisLogStream::Stderr, tx.clone());
        drop(tx);

        let mut latest_stage = None;
        let mut read_error = None;
        let status = loop {
            drain_messages(&rx, &mut latest_stage, &mut read_error, &mut on_event);

            if cancellation.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                join_reader(stdout_reader);
                join_reader(stderr_reader);
                drain_messages(&rx, &mut latest_stage, &mut read_error, &mut on_event);
                return Err(ArnisError::Cancelled);
            }

            match child.try_wait().map_err(ArnisError::Wait)? {
                Some(status) => break status,
                None => thread::sleep(Duration::from_millis(25)),
            }
        };

        join_reader(stdout_reader);
        join_reader(stderr_reader);
        drain_messages(&rx, &mut latest_stage, &mut read_error, &mut on_event);

        if let Some((stream, source)) = read_error {
            return Err(ArnisError::ReadOutput { stream, source });
        }

        if !status.success() {
            return Err(ArnisError::NonZeroExit {
                code: status.code(),
            });
        }

        let world_dir = discover_java_world(&spec.output_dir)?;
        Ok(ArnisRunResult { world_dir })
    }
}

fn spawn_pipe_reader<R>(
    reader: R,
    stream: ArnisLogStream,
    tx: mpsc::Sender<PipeMessage>,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    if tx.send(PipeMessage::Line { stream, line }).is_err() {
                        break;
                    }
                }
                Err(source) => {
                    let _ = tx.send(PipeMessage::ReadError { stream, source });
                    break;
                }
            }
        }
    })
}

fn join_reader(handle: thread::JoinHandle<()>) {
    let _ = handle.join();
}

fn drain_messages<F>(
    rx: &mpsc::Receiver<PipeMessage>,
    latest_stage: &mut Option<ArnisStage>,
    read_error: &mut Option<(ArnisLogStream, std::io::Error)>,
    on_event: &mut F,
) where
    F: FnMut(ArnisEvent),
{
    while let Ok(message) = rx.try_recv() {
        match message {
            PipeMessage::Line { stream, line } => {
                if let Some(stage) = stage_for_line(&line) {
                    let should_emit = latest_stage.is_none_or(|current| stage > current);
                    if should_emit {
                        *latest_stage = Some(stage);
                        on_event(ArnisEvent::Stage(stage));
                    }
                }
                on_event(ArnisEvent::Log { stream, line });
            }
            PipeMessage::ReadError { stream, source } => {
                if read_error.is_none() {
                    *read_error = Some((stream, source));
                }
            }
        }
    }
}

fn stage_for_line(line: &str) -> Option<ArnisStage> {
    if line.contains("[1/7]") || line.contains("[2/7]") || line.contains("[3/7]") {
        Some(ArnisStage::PreparingData)
    } else if line.contains("[4/7]") {
        Some(ArnisStage::ProcessingMap)
    } else if line.contains("[5/7]") || line.contains("[6/7]") {
        Some(ArnisStage::GeneratingWorld)
    } else if line.contains("[7/7]") {
        Some(ArnisStage::SavingWorld)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coarse_stage_mapping_collapses_parallel_early_steps() {
        assert_eq!(
            stage_for_line("[1/7] Fetching data..."),
            Some(ArnisStage::PreparingData)
        );
        assert_eq!(
            stage_for_line("[3/7] Fetching elevation..."),
            Some(ArnisStage::PreparingData)
        );
        assert_eq!(
            stage_for_line("[2/7] Parsing data..."),
            Some(ArnisStage::PreparingData)
        );
        assert_eq!(
            stage_for_line("[4/7] Transforming map..."),
            Some(ArnisStage::ProcessingMap)
        );
        assert_eq!(
            stage_for_line("[5/7] Generating area..."),
            Some(ArnisStage::GeneratingWorld)
        );
        assert_eq!(
            stage_for_line("[6/7] Generating ground..."),
            Some(ArnisStage::GeneratingWorld)
        );
        assert_eq!(
            stage_for_line("[7/7] Saving world..."),
            Some(ArnisStage::SavingWorld)
        );
    }

    #[test]
    fn cancellation_token_is_clone_shared() {
        let token = ArnisCancellationToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
    }
}
