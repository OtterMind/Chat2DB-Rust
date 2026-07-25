mod catalog;
#[allow(
    dead_code,
    reason = "the next staged agent-run transport slice consumes this foundation"
)]
mod hub;
#[allow(
    dead_code,
    reason = "the next staged agent-run transport slice consumes this foundation"
)]
mod provider;
#[allow(
    dead_code,
    reason = "the next staged agent-run worker consumes this durable event bridge"
)]
mod runtime;
#[allow(
    dead_code,
    reason = "the next staged agent-run transport slice consumes this foundation"
)]
mod transcript;

pub(crate) use hub::AgentRunHub;
pub use hub::AgentRunSubscription;
