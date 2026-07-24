use std::{collections::VecDeque, future, pin::Pin};

use bytes::Bytes;
use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::{Stream, StreamExt, stream};
use tokio_util::sync::CancellationToken;

use crate::{ProviderError, ProviderEvent, ProviderEventStream, ProviderKind};

pub(crate) trait SseAssembler: Send + 'static {
    fn provider(&self) -> ProviderKind;

    fn push(&mut self, event: Event) -> Result<Vec<ProviderEvent>, ProviderError>;

    fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError>;
}

type ParsedSseStream =
    Pin<Box<dyn Stream<Item = Result<Event, EventStreamError<ProviderError>>> + Send + 'static>>;

struct DecodeState<A> {
    source: ParsedSseStream,
    assembler: A,
    cancellation: CancellationToken,
    pending: VecDeque<Result<ProviderEvent, ProviderError>>,
    done: bool,
}

pub(crate) fn decode_sse<S, A>(
    source: S,
    assembler: A,
    cancellation: CancellationToken,
    max_response_bytes: usize,
) -> ProviderEventStream
where
    S: Stream<Item = Result<Bytes, ProviderError>> + Send + 'static,
    A: SseAssembler,
{
    let provider = assembler.provider();
    let state = DecodeState {
        source: Box::pin(limit_response_bytes(source, provider, max_response_bytes).eventsource()),
        assembler,
        cancellation,
        pending: VecDeque::new(),
        done: false,
    };

    Box::pin(stream::unfold(state, |mut state| async move {
        loop {
            if let Some(item) = state.pending.pop_front() {
                return Some((item, state));
            }
            if state.done {
                return None;
            }

            let next = tokio::select! {
                () = state.cancellation.cancelled() => {
                    state.done = true;
                    return Some((Err(ProviderError::Cancelled), state));
                }
                item = state.source.next() => item,
            };

            match next {
                Some(Ok(event)) => match state.assembler.push(event) {
                    Ok(events) => state.pending.extend(events.into_iter().map(Ok)),
                    Err(error) => {
                        state.done = true;
                        return Some((Err(error), state));
                    }
                },
                Some(Err(EventStreamError::Transport(error))) => {
                    state.done = true;
                    return Some((Err(error), state));
                }
                Some(Err(error)) => {
                    let provider = state.assembler.provider();
                    state.done = true;
                    return Some((
                        Err(ProviderError::protocol(
                            provider,
                            format!("invalid SSE framing: {error}"),
                        )),
                        state,
                    ));
                }
                None => {
                    state.done = true;
                    match state.assembler.finish() {
                        Ok(events) => state.pending.extend(events.into_iter().map(Ok)),
                        Err(error) => return Some((Err(error), state)),
                    }
                }
            }
        }
    }))
}

fn limit_response_bytes<S>(
    source: S,
    provider: ProviderKind,
    limit: usize,
) -> impl Stream<Item = Result<Bytes, ProviderError>> + Send + 'static
where
    S: Stream<Item = Result<Bytes, ProviderError>> + Send + 'static,
{
    source.scan((0_usize, false), move |state, item| {
        let output = if state.1 {
            None
        } else {
            Some(match item {
                Ok(bytes) => match state.0.checked_add(bytes.len()) {
                    Some(total) if total <= limit => {
                        state.0 = total;
                        Ok(bytes)
                    }
                    Some(_) | None => {
                        state.1 = true;
                        Err(ProviderError::ResponseTooLarge { provider, limit })
                    }
                },
                Err(error) => {
                    state.1 = true;
                    Err(error)
                }
            })
        };
        future::ready(output)
    })
}

#[cfg(test)]
pub(crate) fn fixture_stream<A: SseAssembler>(
    fixture: &'static str,
    cuts: &[usize],
    assembler: A,
) -> ProviderEventStream {
    let mut chunks = Vec::new();
    let mut start = 0;
    for &end in cuts {
        chunks.push(Ok(Bytes::copy_from_slice(&fixture.as_bytes()[start..end])));
        start = end;
    }
    chunks.push(Ok(Bytes::copy_from_slice(&fixture.as_bytes()[start..])));
    decode_sse(
        stream::iter(chunks),
        assembler,
        CancellationToken::new(),
        fixture.len().max(1),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use bytes::Bytes;
    use futures_util::{StreamExt, stream};
    use tokio_util::sync::CancellationToken;

    use super::{SseAssembler, decode_sse};
    use crate::{ProviderError, ProviderEvent, ProviderKind};

    struct CountingAssembler(Arc<AtomicUsize>);

    impl SseAssembler for CountingAssembler {
        fn provider(&self) -> ProviderKind {
            ProviderKind::OpenAi
        }

        fn push(
            &mut self,
            _event: eventsource_stream::Event,
        ) -> Result<Vec<ProviderEvent>, ProviderError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }

        fn finish(&mut self) -> Result<Vec<ProviderEvent>, ProviderError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn raw_response_limit_fails_before_eventsource_can_accumulate_an_event() {
        let pushes = Arc::new(AtomicUsize::new(0));
        let source = stream::iter([
            Ok(Bytes::from_static(b"data:")),
            Ok(Bytes::from_static(b" oversized-without-delimiter")),
        ]);
        let mut decoded = decode_sse(
            source,
            CountingAssembler(pushes.clone()),
            CancellationToken::new(),
            8,
        );

        let error = decoded
            .next()
            .await
            .expect("stream reports the limit")
            .expect_err("oversized response must fail");
        assert!(matches!(
            error,
            ProviderError::ResponseTooLarge {
                provider: ProviderKind::OpenAi,
                limit: 8
            }
        ));
        assert_eq!(pushes.load(Ordering::SeqCst), 0);
        assert!(decoded.next().await.is_none());
    }
}
