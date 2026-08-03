//! Run-level sanitization statistics: files processed, bytes sanitized, PII
//! counts categorised by entity, and throughput. Aggregated per-file by the
//! CLI/daemon and surfaced in the completion summary.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::pii::PiiFinding;

/// A single `(entity, count)` row in the per-category PII breakdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryCount {
    /// Entity slug (e.g. `"email"`, `"ssn"`, `"credit_card"`).
    pub entity: String,
    /// Number of instances scrubbed for this entity across the run.
    pub count: u64,
}

/// Aggregate statistics for a sanitization run (one or more files).
///
/// Build incrementally with [`SanitizationStats::record_file`] as each file is
/// processed, then call [`SanitizationStats::finalize`] to stamp the elapsed
/// duration.
#[derive(Debug, Clone, Default)]
pub struct SanitizationStats {
    files_processed: u64,
    bytes_sanitized: u64,
    pii_by_type: BTreeMap<String, u64>,
    duration: Option<Duration>,
}

impl SanitizationStats {
    /// Creates an empty stats accumulator.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one processed file: its byte length and the PII findings
    /// detected within it.
    pub fn record_file(&mut self, byte_len: u64, findings: &[PiiFinding]) {
        self.files_processed += 1;
        self.bytes_sanitized += byte_len;
        for f in findings {
            *self.pii_by_type.entry(f.entity.clone()).or_insert(0) += 1;
        }
    }

    /// Records a destructively-redacted PDF: its byte length and the number of
    /// PII spans scrubbed (categorized under `pdf_redacted`, since the pdfium
    /// path does not classify by entity).
    pub fn record_pdf_file(&mut self, byte_len: u64, matches_scrubbed: usize) {
        self.files_processed += 1;
        self.bytes_sanitized += byte_len;
        if matches_scrubbed > 0 {
            *self
                .pii_by_type
                .entry("pdf_redacted".to_string())
                .or_insert(0) += matches_scrubbed as u64;
        }
    }

    /// Stamps the wall-clock duration of the run (call once at the end).
    pub fn finalize(&mut self, duration: Duration) {
        self.duration = Some(duration);
    }

    /// Number of files processed.
    #[must_use]
    pub fn files_processed(&self) -> u64 {
        self.files_processed
    }

    /// Total bytes sanitized across all files.
    #[must_use]
    pub fn bytes_sanitized(&self) -> u64 {
        self.bytes_sanitized
    }

    /// Total PII instances scrubbed (sum across all entities).
    #[must_use]
    pub fn total_pii(&self) -> u64 {
        self.pii_by_type.values().sum()
    }

    /// Per-entity breakdown, sorted by entity slug for stable display.
    #[must_use]
    pub fn categories(&self) -> Vec<CategoryCount> {
        self.pii_by_type
            .iter()
            .map(|(entity, &count)| CategoryCount {
                entity: entity.clone(),
                count,
            })
            .collect()
    }

    /// Throughput in MiB/s, or `0.0` if the duration is unset/zero.
    #[must_use]
    pub fn throughput_mib_s(&self) -> f64 {
        let secs = self.duration.map_or(0.0, |d| d.as_secs_f64());
        if secs <= 0.0 {
            return 0.0;
        }
        #[allow(clippy::cast_precision_loss)]
        let mib = self.bytes_sanitized as f64 / 1_048_576.0;
        mib / secs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pii::PiiFinding;

    fn finding(entity: &str) -> PiiFinding {
        PiiFinding {
            entity: entity.to_string(),
            offset_start: 0,
            offset_end: 1,
            confidence: 0.9,
            anchors_hit: vec![],
        }
    }

    #[test]
    fn record_file_accumulates() {
        let mut s = SanitizationStats::new();
        s.record_file(100, &[finding("email"), finding("ssn")]);
        s.record_file(250, &[finding("email"), finding("email")]);
        assert_eq!(s.files_processed(), 2);
        assert_eq!(s.bytes_sanitized(), 350);
        assert_eq!(s.total_pii(), 4);
        let cats = s.categories();
        assert_eq!(cats.len(), 2);
        assert!(cats.iter().any(|c| c.entity == "email" && c.count == 3));
        assert!(cats.iter().any(|c| c.entity == "ssn" && c.count == 1));
    }

    #[test]
    fn throughput_is_bytes_over_seconds() {
        let mut s = SanitizationStats::new();
        // 2 MiB in 0.5 s → 4.0 MiB/s
        s.record_file(2 * 1_048_576, &[]);
        s.finalize(Duration::from_millis(500));
        let t = s.throughput_mib_s();
        assert!((t - 4.0).abs() < 1e-6, "throughput={t}");
    }

    #[test]
    fn throughput_zero_without_duration() {
        let mut s = SanitizationStats::new();
        s.record_file(1024, &[]);
        assert!(s.throughput_mib_s() <= 0.0);
    }

    #[test]
    fn empty_stats_are_zero() {
        let s = SanitizationStats::new();
        assert_eq!(s.files_processed(), 0);
        assert_eq!(s.bytes_sanitized(), 0);
        assert_eq!(s.total_pii(), 0);
        assert!(s.categories().is_empty());
    }
}
