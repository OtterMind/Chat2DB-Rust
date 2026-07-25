use std::ops::Range;

use chat2db_agent::CompactionStrategy;

use crate::AppError;

/// Exact durable-ordinal provenance for the mutable provider context.
///
/// Transient slots represent replacement summaries or run messages that do not
/// yet have a durable ordinal. A compaction range containing one fails closed.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct CompactionOrdinalMap {
    message_ordinals: Vec<Option<u64>>,
}

impl CompactionOrdinalMap {
    pub(super) fn new(message_ordinals: impl IntoIterator<Item = u64>) -> Self {
        Self {
            message_ordinals: message_ordinals.into_iter().map(Some).collect(),
        }
    }

    pub(super) fn append_transient_messages(&mut self, count: usize) -> Result<(), AppError> {
        let length = self
            .message_ordinals
            .len()
            .checked_add(count)
            .ok_or_else(AppError::internal)?;
        self.message_ordinals.resize(length, None);
        Ok(())
    }

    /// Applies the same splice or drain performed by the agent context manager.
    ///
    /// The returned coverage is the greatest actual durable ordinal in the
    /// pre-compaction range. It is never inferred from indexes or turn counts.
    pub(super) fn apply_compaction(
        &mut self,
        range: Option<Range<usize>>,
        strategy: CompactionStrategy,
    ) -> Result<Option<u64>, AppError> {
        let Some(range) = range else {
            return Ok(None);
        };
        if range.is_empty() || range.end > self.message_ordinals.len() {
            return Err(AppError::internal());
        }

        let mut coverage = None;
        for ordinal in &self.message_ordinals[range.clone()] {
            let ordinal = ordinal.ok_or_else(AppError::internal)?;
            coverage = Some(coverage.map_or(ordinal, |current: u64| current.max(ordinal)));
        }
        let coverage = coverage.ok_or_else(AppError::internal)?;

        match strategy {
            CompactionStrategy::Summary => {
                self.message_ordinals.splice(range, [None]);
            }
            CompactionStrategy::DeterministicTrim => {
                self.message_ordinals.drain(range);
            }
        }
        Ok(Some(coverage))
    }
}

#[cfg(test)]
mod tests {
    use chat2db_agent::CompactionStrategy;

    use super::CompactionOrdinalMap;
    use crate::AppErrorKind;

    #[test]
    fn summary_coverage_uses_actual_ordinals_and_splices_one_transient_slot() {
        let mut ordinals = CompactionOrdinalMap::new([0, 7, 3, 9, 12]);

        let coverage = ordinals
            .apply_compaction(Some(1..4), CompactionStrategy::Summary)
            .expect("range maps");

        assert_eq!(coverage, Some(9));
        assert_eq!(ordinals.message_ordinals, vec![Some(0), None, Some(12)]);
    }

    #[test]
    fn deterministic_trim_tracks_indexes_after_an_earlier_summary() {
        let mut ordinals = CompactionOrdinalMap::new([0, 4, 8, 10, 12]);
        assert_eq!(
            ordinals
                .apply_compaction(Some(1..3), CompactionStrategy::Summary)
                .expect("summary maps"),
            Some(8)
        );

        let coverage = ordinals
            .apply_compaction(Some(2..3), CompactionStrategy::DeterministicTrim)
            .expect("trim maps");

        assert_eq!(coverage, Some(10));
        assert_eq!(ordinals.message_ordinals, vec![Some(0), None, Some(12)]);
    }

    #[test]
    fn no_op_compaction_preserves_the_mapping_without_fabricating_coverage() {
        let mut ordinals = CompactionOrdinalMap::new([2, 4, 6]);

        assert_eq!(
            ordinals
                .apply_compaction(None, CompactionStrategy::DeterministicTrim)
                .expect("no-op maps"),
            None
        );
        assert_eq!(ordinals.message_ordinals, vec![Some(2), Some(4), Some(6)]);
    }

    #[test]
    fn invalid_or_transient_ranges_fail_closed() {
        let mut empty = CompactionOrdinalMap::new([0, 1]);
        let error = empty
            .apply_compaction(Some(1..1), CompactionStrategy::DeterministicTrim)
            .expect_err("empty range fails");
        assert_eq!(error.kind(), AppErrorKind::Internal);

        let mut out_of_bounds = CompactionOrdinalMap::new([0, 1]);
        let error = out_of_bounds
            .apply_compaction(Some(1..3), CompactionStrategy::Summary)
            .expect_err("out-of-bounds range fails");
        assert_eq!(error.kind(), AppErrorKind::Internal);

        let mut transient = CompactionOrdinalMap::new([0, 1, 2]);
        transient
            .apply_compaction(Some(1..2), CompactionStrategy::Summary)
            .expect("summary creates a transient slot");
        let error = transient
            .apply_compaction(Some(1..2), CompactionStrategy::DeterministicTrim)
            .expect_err("transient coverage fails");
        assert_eq!(error.kind(), AppErrorKind::Internal);
    }

    #[test]
    fn appended_run_messages_remain_explicitly_transient() {
        let mut ordinals = CompactionOrdinalMap::new([0, 1]);
        ordinals
            .append_transient_messages(2)
            .expect("transient messages append");

        assert_eq!(
            ordinals.message_ordinals,
            vec![Some(0), Some(1), None, None]
        );
    }
}
