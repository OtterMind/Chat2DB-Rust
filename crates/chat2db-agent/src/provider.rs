use std::pin::Pin;

use async_trait::async_trait;
use futures_util::Stream;
use tokio_util::sync::CancellationToken;

use crate::{
    ContextBudget, ContextUsage, ProviderError, ProviderEvent, ProviderKind, ProviderRequest,
};

/// A normalized stream returned by every model provider.
pub type ProviderEventStream =
    Pin<Box<dyn Stream<Item = Result<ProviderEvent, ProviderError>> + Send + 'static>>;

/// Direct model-provider boundary used by the agent loop.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Identifies the wire family without exposing its DTOs.
    fn kind(&self) -> ProviderKind;

    /// Returns the configured model name.
    fn model(&self) -> &str;

    /// Returns the limits used for proactive context compaction.
    fn context_budget(&self) -> ContextBudget;

    /// Estimates tokens and the exact serialized request size for this provider.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError`] when normalized messages cannot be represented
    /// or the private request DTO cannot be serialized.
    fn estimate_context(&self, request: &ProviderRequest) -> Result<ContextUsage, ProviderError>;

    /// Starts one streaming model response.
    async fn stream(
        &self,
        request: ProviderRequest,
        cancellation: CancellationToken,
    ) -> Result<ProviderEventStream, ProviderError>;
}
