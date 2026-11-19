//! Debounced configuration persistence service (`100 ms` poll cadence).
//!
//! Delegates actual flash writes to the [`SettingsPersistHost`] attached to
//! [`ServiceCtx`] so all `runtime-flash-persist` feature gating stays in
//! `main.rs` and all throttling/counting logic is host-testable.

use crate::services::DeviceService;
use crate::services::ServiceCtx;
use crate::services::StatusExtras;

/// Minimum interval between configuration writer polls in milliseconds.
pub const SETTINGS_POLL_INTERVAL_MS: u64 = 100;

/// Device service driving debounced configuration persistence on a 100 ms tick.
pub struct SettingsService {
    last_poll_ms: u64,
    persist_count: u32,
    last_persisted: bool,
}

impl Default for SettingsService {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsService {
    /// Creates a new [`SettingsService`] with zero recorded flash writes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last_poll_ms: 0,
            persist_count: 0,
            last_persisted: false,
        }
    }

    /// Total number of flash persistence cycles completed since boot.
    #[must_use]
    pub const fn persist_count(&self) -> u32 {
        self.persist_count
    }

    /// Returns `true` when the most recent 100 ms poll flushed to storage.
    #[must_use]
    pub const fn last_persisted(&self) -> bool {
        self.last_persisted
    }
}

impl DeviceService for SettingsService {
    fn name(&self) -> &'static str {
        "settings"
    }

    fn init(&mut self, _ctx: &mut ServiceCtx<'_>) {}

    fn on_link(&mut self, _ctx: &mut ServiceCtx<'_>, _up: bool) {}

    fn poll(&mut self, ctx: &mut ServiceCtx<'_>) {
        let now_ms = ctx.now_ms();
        if now_ms.saturating_sub(self.last_poll_ms) >= SETTINGS_POLL_INTERVAL_MS {
            self.last_poll_ms = now_ms;
            let persisted = ctx.poll_settings_persist();
            self.last_persisted = persisted;
            if persisted {
                self.persist_count = self.persist_count.saturating_add(1);
            }
        }
    }

    fn status(&self, out: &mut StatusExtras) {
        out.push_u64("settings_writes", u64::from(self.persist_count));
    }
}

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::vec;
    use alloc::vec::Vec;
    use squeezelite_config::AudioConfig;

    use super::*;
    use crate::config_handle::ConfigHandle;
    use crate::services::{SettingsPersistHost, StatusValue};

    struct MockSettingsPersistHost {
        calls: Vec<u64>,
        results: Vec<bool>,
    }

    impl MockSettingsPersistHost {
        fn new(results: Vec<bool>) -> Self {
            Self {
                calls: Vec::new(),
                results,
            }
        }
    }

    impl SettingsPersistHost for MockSettingsPersistHost {
        fn poll_and_persist(&mut self, _config: &ConfigHandle, now_ms: u64) -> bool {
            let idx = self.calls.len();
            self.calls.push(now_ms);
            self.results.get(idx).copied().unwrap_or(false)
        }
    }

    #[test]
    fn settings_service_throttles_polls_to_100ms() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut host = MockSettingsPersistHost::new(vec![false, true]);
        let mut svc = SettingsService::new();
        assert_eq!(svc.name(), "settings");

        // Before 100 ms elapsed from boot (0 ms): throttled, host not called.
        {
            let mut ctx = ServiceCtx::new(99, None, &cfg).with_settings_persist(&mut host);
            svc.poll(&mut ctx);
        }
        assert!(host.calls.is_empty());

        // At 100 ms: first poll runs.
        {
            let mut ctx = ServiceCtx::new(100, None, &cfg).with_settings_persist(&mut host);
            svc.poll(&mut ctx);
        }
        assert_eq!(host.calls, vec![100]);
        assert!(!svc.last_persisted());
        assert_eq!(svc.persist_count(), 0);

        // At 199 ms (99 ms since 100 ms): throttled.
        {
            let mut ctx = ServiceCtx::new(199, None, &cfg).with_settings_persist(&mut host);
            svc.poll(&mut ctx);
        }
        assert_eq!(host.calls, vec![100]);

        // At 200 ms (100 ms since 100 ms): second poll runs and records write.
        {
            let mut ctx = ServiceCtx::new(200, None, &cfg).with_settings_persist(&mut host);
            svc.poll(&mut ctx);
        }
        assert_eq!(host.calls, vec![100, 200]);
        assert!(svc.last_persisted());
        assert_eq!(svc.persist_count(), 1);
    }

    #[test]
    fn settings_service_counts_persists_and_reports_status() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut host = MockSettingsPersistHost::new(vec![true, false, true]);
        let mut svc = SettingsService::new();

        for (idx, now_ms) in [100u64, 200, 300].into_iter().enumerate() {
            let mut ctx = ServiceCtx::new(now_ms, Some([172, 22, 1, 10]), &cfg)
                .with_settings_persist(&mut host);
            svc.poll(&mut ctx);
            let expected_last = idx != 1;
            assert_eq!(svc.last_persisted(), expected_last);
        }

        assert_eq!(svc.persist_count(), 2);
        let mut extras = StatusExtras::new();
        svc.status(&mut extras);
        assert_eq!(extras.len(), 1);
        assert_eq!(extras.fields()[0].key, "settings_writes");
        assert_eq!(extras.fields()[0].value, StatusValue::U64(2));
    }

    #[test]
    fn settings_service_detached_host_is_noop() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut svc = SettingsService::new();

        let mut init_ctx = ServiceCtx::new(0, None, &cfg);
        svc.init(&mut init_ctx);
        svc.on_link(&mut init_ctx, true);

        let mut poll_ctx = ServiceCtx::new(100, Some([10, 0, 0, 2]), &cfg);
        svc.poll(&mut poll_ctx);
        svc.on_link(&mut poll_ctx, false);

        assert_eq!(svc.persist_count(), 0);
        assert!(!svc.last_persisted());

        let mut extras = StatusExtras::new();
        svc.status(&mut extras);
        assert_eq!(extras.len(), 1);
        assert_eq!(extras.fields()[0].key, "settings_writes");
        assert_eq!(extras.fields()[0].value, StatusValue::U64(0));
    }
}
