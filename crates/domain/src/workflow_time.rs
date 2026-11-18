//! Explicit v1 limits for logical execution wall time and Human waiting.
//! Legacy activity time remains a separate budget. No clock or environment I/O.
use crate::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTimeLimits {
    pub version: u64,
    pub wall_seconds: u64,
    pub human_wait_seconds: u64,
}
impl ExecutionTimeLimits {
    pub fn validate(&self) -> Result<()> {
        require(
            self.version == 1
                && (1..=2_592_000).contains(&self.wall_seconds)
                && (1..=self.wall_seconds).contains(&self.human_wait_seconds),
            "执行时间政策版本或有限期限无效",
        )
    }
}

/// A process epoch is never compared with another process's monotonic counter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionClockAnchor {
    pub wall_ms: u64,
    pub process_epoch: Option<Id>,
    pub monotonic_ms: Option<u64>,
}
impl ExecutionClockAnchor {
    pub fn effective_now(&self, clock: &Self) -> Result<u64> {
        if self.process_epoch.is_some() && self.process_epoch == clock.process_epoch {
            let elapsed = clock
                .monotonic_ms
                .zip(self.monotonic_ms)
                .and_then(|(now, then)| now.checked_sub(then))
                .ok_or_else(|| err("execution_clock_uncertain", "同进程单调时钟倒退或缺失"))?;
            return self
                .wall_ms
                .checked_add(elapsed)
                .ok_or_else(|| err("execution_clock_uncertain", "执行时钟超出范围"));
        }
        if clock.wall_ms < self.wall_ms {
            return Err(err(
                "execution_clock_uncertain",
                "重启后的时间早于已持久化时钟，原执行须核对",
            ));
        }
        Ok(clock.wall_ms)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionTimeBudget {
    pub origin_execution_id: Id,
    pub started_at: u64,
    pub clock: ExecutionClockAnchor,
    pub human_wait_started_at: Option<u64>,
    pub human_wait_closed: bool,
    pub next_deadline_at: Option<u64>,
}
impl ExecutionTimeBudget {
    pub fn new(execution: &str, clock: ExecutionClockAnchor) -> Self {
        Self {
            origin_execution_id: execution.into(),
            started_at: clock.wall_ms,
            clock,
            human_wait_started_at: None,
            human_wait_closed: false,
            next_deadline_at: None,
        }
    }
    pub fn deadline(&self, limits: &ExecutionTimeLimits) -> u64 {
        let wall = self
            .started_at
            .saturating_add(limits.wall_seconds.saturating_mul(1000));
        self.human_wait_started_at.map_or(wall, |start| {
            wall.min(start.saturating_add(limits.human_wait_seconds.saturating_mul(1000)))
        })
    }
    pub fn for_successor(&self, limits: &ExecutionTimeLimits) -> Self {
        let mut successor = self.clone();
        successor.human_wait_started_at = None;
        successor.human_wait_closed = false;
        successor.next_deadline_at = Some(successor.deadline(limits));
        successor
    }
    pub fn check(&self, limits: &ExecutionTimeLimits, clock: &ExecutionClockAnchor) -> Result<u64> {
        let now = self.clock.effective_now(clock)?;
        if now
            >= self
                .started_at
                .saturating_add(limits.wall_seconds.saturating_mul(1000))
        {
            return Err(err(
                "execution_wall_timeout",
                "逻辑执行总墙钟期限已到，等待和技术重试不重置期限",
            ));
        }
        if self.human_wait_started_at.is_some_and(|start| {
            now >= start.saturating_add(limits.human_wait_seconds.saturating_mul(1000))
        }) {
            return Err(err("human_wait_timeout", "当前人类等待期限已到"));
        }
        Ok(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn clock(wall: u64, epoch: &str, mono: u64) -> ExecutionClockAnchor {
        ExecutionClockAnchor {
            wall_ms: wall,
            process_epoch: Some(epoch.into()),
            monotonic_ms: Some(mono),
        }
    }
    #[test]
    fn monotonic_time_prevents_wall_jumps_from_refreshing_a_live_budget() {
        let limits = ExecutionTimeLimits {
            version: 1,
            wall_seconds: 10,
            human_wait_seconds: 3,
        };
        let b = ExecutionTimeBudget::new("original", clock(10_000, "a", 0));
        assert_eq!(b.check(&limits, &clock(1, "a", 9999)).unwrap(), 19_999);
        assert_eq!(
            b.check(&limits, &clock(1, "a", 10_000)).unwrap_err().code,
            "execution_wall_timeout"
        );
        assert_eq!(b.check(&limits, &clock(1_000_000, "a", 1)).unwrap(), 10_001);
        assert_eq!(
            b.check(&limits, &clock(9999, "b", 0)).unwrap_err().code,
            "execution_clock_uncertain"
        );
        assert_eq!(
            b.check(&limits, &clock(20_000, "b", 0)).unwrap_err().code,
            "execution_wall_timeout"
        );
    }
    #[test]
    fn human_deadline_is_bounded_by_wall_time_and_uses_the_original_start() {
        let limits = ExecutionTimeLimits {
            version: 1,
            wall_seconds: 10,
            human_wait_seconds: 3,
        };
        let mut b = ExecutionTimeBudget::new("original", clock(10_000, "a", 0));
        b.human_wait_started_at = Some(11_000);
        assert_eq!(b.deadline(&limits), 14_000);
        assert_eq!(b.check(&limits, &clock(13_999, "b", 0)).unwrap(), 13_999);
        assert_eq!(
            b.check(&limits, &clock(14_000, "b", 0)).unwrap_err().code,
            "human_wait_timeout"
        );
        b.human_wait_started_at = Some(19_000);
        assert_eq!(b.deadline(&limits), 20_000);
        for bad in [
            ExecutionTimeLimits {
                version: 2,
                ..limits.clone()
            },
            ExecutionTimeLimits {
                wall_seconds: 0,
                ..limits.clone()
            },
            ExecutionTimeLimits {
                human_wait_seconds: 11,
                ..limits
            },
        ] {
            assert!(bad.validate().is_err());
        }
    }
}
