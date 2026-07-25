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

pub(crate) use hub::AgentRunHub;
pub use hub::AgentRunSubscription;
