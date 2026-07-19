#![forbid(unsafe_code)]

use pulsehub_core::Environment;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionPolicy {
    Auto,
    Fixed(Environment),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRule {
    pub environment: Environment,
    pub process_names: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentTransition {
    pub previous: Option<Environment>,
    pub target: Environment,
}

#[derive(Debug, Default)]
pub struct EnvironmentTracker {
    current: Option<Environment>,
}

#[derive(Debug, Default)]
pub struct RetryBackoff {
    failures: u32,
}

impl RetryBackoff {
    pub fn record_failure(&mut self) -> Duration {
        const DELAYS_MS: [u64; 6] = [250, 500, 1_000, 2_000, 5_000, 10_000];
        let index = usize::try_from(self.failures)
            .unwrap_or(usize::MAX)
            .min(DELAYS_MS.len() - 1);
        self.failures = self.failures.saturating_add(1);
        Duration::from_millis(DELAYS_MS[index])
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
    }

    pub fn failures(&self) -> u32 {
        self.failures
    }
}

impl EnvironmentTracker {
    pub fn current(&self) -> Option<Environment> {
        self.current
    }

    pub fn observe(&mut self, target: Environment) -> Option<EnvironmentTransition> {
        if self.current == Some(target) {
            return None;
        }
        let transition = EnvironmentTransition {
            previous: self.current,
            target,
        };
        self.current = Some(target);
        Some(transition)
    }

    pub fn invalidate(&mut self) {
        self.current = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyToken {
    pub connection_generation: u64,
    pub invalidation_epoch: u64,
}

pub fn select_environment(executable_name: Option<&str>) -> Environment {
    select_environment_with_rules(
        SelectionPolicy::Auto,
        executable_name,
        &[ProcessRule {
            environment: Environment::Cs2,
            process_names: vec!["cs2.exe".to_owned()],
        }],
    )
}

pub fn select_environment_with_rules(
    policy: SelectionPolicy,
    executable_name: Option<&str>,
    rules: &[ProcessRule],
) -> Environment {
    match policy {
        SelectionPolicy::Fixed(environment) => environment,
        SelectionPolicy::Auto => executable_name
            .and_then(|name| {
                rules.iter().find_map(|rule| {
                    rule.process_names
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(name))
                        .then_some(rule.environment)
                })
            })
            .unwrap_or(Environment::Office),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cs2_is_matched_case_insensitively() {
        assert_eq!(select_environment(Some("CS2.EXE")), Environment::Cs2);
        assert_eq!(select_environment(Some("steam.exe")), Environment::Office);
    }

    #[test]
    fn configured_rules_and_fixed_policy_are_respected() {
        let rules = [ProcessRule {
            environment: Environment::Cs2,
            process_names: vec!["custom-game.exe".to_owned()],
        }];
        assert_eq!(
            select_environment_with_rules(SelectionPolicy::Auto, Some("CUSTOM-GAME.EXE"), &rules),
            Environment::Cs2
        );
        assert_eq!(
            select_environment_with_rules(
                SelectionPolicy::Fixed(Environment::Cs2),
                Some("explorer.exe"),
                &[]
            ),
            Environment::Cs2
        );
    }

    #[test]
    fn tracker_deduplicates_events_until_environment_changes() {
        let mut tracker = EnvironmentTracker::default();
        assert_eq!(
            tracker.observe(Environment::Office),
            Some(EnvironmentTransition {
                previous: None,
                target: Environment::Office
            })
        );
        assert_eq!(tracker.observe(Environment::Office), None);
        assert_eq!(
            tracker.observe(Environment::Cs2),
            Some(EnvironmentTransition {
                previous: Some(Environment::Office),
                target: Environment::Cs2
            })
        );
        tracker.invalidate();
        assert_eq!(tracker.observe(Environment::Cs2).unwrap().previous, None);
    }

    #[test]
    fn retry_backoff_is_bounded_and_resets_after_success() {
        let mut backoff = RetryBackoff::default();
        assert_eq!(backoff.record_failure(), Duration::from_millis(250));
        assert_eq!(backoff.record_failure(), Duration::from_millis(500));
        for _ in 0..10 {
            let delay = backoff.record_failure();
            assert!(delay <= Duration::from_secs(10));
        }
        backoff.record_success();
        assert_eq!(backoff.failures(), 0);
        assert_eq!(backoff.record_failure(), Duration::from_millis(250));
    }
}
