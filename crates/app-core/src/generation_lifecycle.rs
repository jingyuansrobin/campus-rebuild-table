use arnis_adapter::{ArnisCancellationToken, ArnisEvent, ArnisLogStream, ArnisStage};

#[derive(Debug, Clone, Default)]
pub struct GenerationCancellationToken {
    inner: ArnisCancellationToken,
}

impl GenerationCancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }

    pub(crate) fn arnis_token(&self) -> &ArnisCancellationToken {
        &self.inner
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationLogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GenerationStage {
    PreparingData,
    ProcessingMap,
    GeneratingWorld,
    SavingWorld,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationEvent {
    Stage(GenerationStage),
    Log {
        stream: GenerationLogStream,
        line: String,
    },
}

pub(crate) fn map_arnis_event(event: ArnisEvent) -> GenerationEvent {
    match event {
        ArnisEvent::Stage(stage) => GenerationEvent::Stage(match stage {
            ArnisStage::PreparingData => GenerationStage::PreparingData,
            ArnisStage::ProcessingMap => GenerationStage::ProcessingMap,
            ArnisStage::GeneratingWorld => GenerationStage::GeneratingWorld,
            ArnisStage::SavingWorld => GenerationStage::SavingWorld,
        }),
        ArnisEvent::Log { stream, line } => GenerationEvent::Log {
            stream: match stream {
                ArnisLogStream::Stdout => GenerationLogStream::Stdout,
                ArnisLogStream::Stderr => GenerationLogStream::Stderr,
            },
            line,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_token_is_clone_shared_without_exposing_provider_type() {
        let token = GenerationCancellationToken::new();
        let clone = token.clone();
        assert!(!token.is_cancelled());
        clone.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn arnis_events_map_to_provider_neutral_events() {
        assert_eq!(
            map_arnis_event(ArnisEvent::Stage(ArnisStage::GeneratingWorld)),
            GenerationEvent::Stage(GenerationStage::GeneratingWorld)
        );
        assert_eq!(
            map_arnis_event(ArnisEvent::Log {
                stream: ArnisLogStream::Stderr,
                line: "warning".into(),
            }),
            GenerationEvent::Log {
                stream: GenerationLogStream::Stderr,
                line: "warning".into(),
            }
        );
    }
}
