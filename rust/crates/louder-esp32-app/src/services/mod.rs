//! Device service registry and lifecycle contract (F4 Task 1).
//!
//! Each device network or background feature implements [`DeviceService`] and
//! is registered exactly once inside [`register_all`].

mod registry {
    extern crate alloc;

    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec::Vec;

    #[cfg(target_arch = "xtensa")]
    use smoltcp::iface::{SocketHandle, SocketSet};
    #[cfg(target_arch = "xtensa")]
    use smoltcp::socket::AnySocket;

    use crate::config_handle::ConfigHandle;
    use crate::coordinator::AudioSource;
    use crate::coordinator::PlayerCoordinator;
    use crate::io_traits::AudioBackend;
    use crate::pipeline::ActiveDecoder;
    use crate::pipeline::StreamPipeline;
    use squeezelite_audio_core::AudioOutputSink;
    use squeezelite_decoders::PcmDecoder;

    /// Typed value appended to [`StatusExtras`] by a [`DeviceService`].
    #[non_exhaustive]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum StatusValue {
        /// UTF-8 string value.
        Str(String),
        /// Boolean flag value.
        Bool(bool),
        /// Unsigned 64-bit integer value.
        U64(u64),
        /// Signed 64-bit integer value.
        I64(i64),
    }

    /// Key/value entry collected for `/status.json` extensions.
    #[non_exhaustive]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct StatusField {
        /// JSON field name.
        pub key: String,
        /// JSON field value.
        pub value: StatusValue,
    }

    /// Accumulator for service-contributed fields in `/status.json`.
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    pub struct StatusExtras {
        fields: Vec<StatusField>,
    }

    impl StatusExtras {
        /// Creates an empty status extension accumulator on the heap.
        #[must_use]
        pub fn new() -> Self {
            Self { fields: Vec::new() }
        }

        /// Appends a string status field.
        pub fn push_str(&mut self, key: &str, value: &str) {
            self.fields.push(StatusField {
                key: String::from(key),
                value: StatusValue::Str(String::from(value)),
            });
        }

        /// Appends a boolean status field.
        pub fn push_bool(&mut self, key: &str, value: bool) {
            self.fields.push(StatusField {
                key: String::from(key),
                value: StatusValue::Bool(value),
            });
        }

        /// Appends an unsigned integer status field.
        pub fn push_u64(&mut self, key: &str, value: u64) {
            self.fields.push(StatusField {
                key: String::from(key),
                value: StatusValue::U64(value),
            });
        }

        /// Appends a signed integer status field.
        pub fn push_i64(&mut self, key: &str, value: i64) {
            self.fields.push(StatusField {
                key: String::from(key),
                value: StatusValue::I64(value),
            });
        }

        /// Returns all accumulated status fields in insertion order.
        #[must_use]
        pub fn fields(&self) -> &[StatusField] {
            &self.fields
        }

        /// Returns the number of accumulated fields.
        #[must_use]
        pub fn len(&self) -> usize {
            self.fields.len()
        }

        /// Returns `true` when no fields have been recorded.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.fields.is_empty()
        }

        /// Clears all accumulated fields while retaining heap capacity.
        pub fn clear(&mut self) {
            self.fields.clear();
        }
    }

    /// Whether claiming external PCM should also power on the amplifier.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum AmpPower {
        /// Ensure the amplifier clears faults and enters `Play` mode.
        EnsurePlay,
        /// Leave amplifier power state unchanged.
        KeepCurrent,
    }

    /// Host-testable audio and volume control surface attached to [`ServiceCtx`].
    pub trait ServiceAudioHost {
        /// Claims external 16-bit stereo PCM playback for `source`.
        ///
        /// Returns `true` when `source` holds pipeline ownership.
        fn claim_external_pcm(
            &mut self,
            source: AudioSource,
            sample_rate_hz: Option<u32>,
            amp_power: AmpPower,
        ) -> bool;

        /// Releases `source` and restores any suspended source or stops playback.
        fn release_external(&mut self, source: AudioSource);

        /// Clamps `percent` to `0..=100`, applies volume, and notifies CYD.
        fn set_local_volume(&mut self, percent: u8) -> u8;
    }

    /// Concrete [`ServiceAudioHost`] bridging [`PlayerCoordinator`],
    /// [`StreamPipeline`], and [`AudioBackend`] with an optional CYD volume hook.
    pub struct ServiceAudioAdapter<'a, 'p, S: AudioOutputSink, B: AudioBackend, F = fn(u8)> {
        coordinator: &'a mut PlayerCoordinator,
        pipeline: &'a mut StreamPipeline<'p, S>,
        backend: &'a mut B,
        on_cyd_volume: Option<F>,
    }

    impl<'a, 'p, S: AudioOutputSink, B: AudioBackend> ServiceAudioAdapter<'a, 'p, S, B, fn(u8)> {
        /// Creates an adapter without a CYD volume notification callback.
        #[must_use]
        pub fn new(
            coordinator: &'a mut PlayerCoordinator,
            pipeline: &'a mut StreamPipeline<'p, S>,
            backend: &'a mut B,
        ) -> Self {
            Self {
                coordinator,
                pipeline,
                backend,
                on_cyd_volume: None,
            }
        }
    }

    impl<'a, 'p, S: AudioOutputSink, B: AudioBackend, F: FnMut(u8)>
        ServiceAudioAdapter<'a, 'p, S, B, F>
    {
        /// Creates an adapter with a CYD volume notification callback.
        #[must_use]
        pub fn with_cyd_volume(
            coordinator: &'a mut PlayerCoordinator,
            pipeline: &'a mut StreamPipeline<'p, S>,
            backend: &'a mut B,
            on_cyd_volume: F,
        ) -> Self {
            Self {
                coordinator,
                pipeline,
                backend,
                on_cyd_volume: Some(on_cyd_volume),
            }
        }
    }

    impl<S: AudioOutputSink, B: AudioBackend, F: FnMut(u8)> ServiceAudioHost
        for ServiceAudioAdapter<'_, '_, S, B, F>
    {
        fn claim_external_pcm(
            &mut self,
            source: AudioSource,
            sample_rate_hz: Option<u32>,
            amp_power: AmpPower,
        ) -> bool {
            if !self.coordinator.request_source(source) {
                return false;
            }
            if let Some(hz) = sample_rate_hz {
                if self.coordinator.current_sample_rate != hz || self.pipeline.sample_rate != hz {
                    self.coordinator.current_sample_rate = hz;
                    let _ = self.pipeline.set_sample_rate(hz);
                }
            }
            self.pipeline
                .set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)));
            self.pipeline.begin_external_audio();
            self.pipeline.set_mute(false);
            self.coordinator
                .begin_external_playback_with_pipeline(self.pipeline);
            if matches!(amp_power, AmpPower::EnsurePlay) {
                self.backend.set_power(true);
            }
            true
        }

        fn release_external(&mut self, source: AudioSource) {
            self.coordinator.release_source(source);
            match self.coordinator.active_source() {
                AudioSource::SlimProto => {
                    self.coordinator
                        .end_external_playback_and_restore(self.pipeline);
                    self.pipeline.set_mute(false);
                }
                AudioSource::None => {
                    self.pipeline.use_mp3_decoder();
                    self.coordinator.end_external_playback();
                }
                AudioSource::AirPlay | AudioSource::Spotify | AudioSource::Bluetooth => {
                    self.pipeline
                        .set_decoder(ActiveDecoder::Pcm(PcmDecoder::new(16, 2, false)));
                    let _ = self.pipeline.set_sample_rate(44_100);
                    self.pipeline.set_mute(false);
                    self.pipeline.stream_buffer.flush();
                }
            }
        }

        fn set_local_volume(&mut self, percent: u8) -> u8 {
            let clamped = percent.min(100);
            self.coordinator
                .set_volume_percent(clamped, self.backend, self.pipeline);
            if let Some(cb) = self.on_cyd_volume.as_mut() {
                cb(clamped);
            }
            clamped
        }
    }

    /// Per-call runtime context passed to [`DeviceService`] lifecycle hooks.
    pub struct ServiceCtx<'a> {
        now_ms: u64,
        link_ip: Option<[u8; 4]>,
        config: &'a ConfigHandle,
        audio: Option<&'a mut dyn ServiceAudioHost>,
        #[cfg(target_arch = "xtensa")]
        sockets: &'a mut SocketSet<'static>,
    }

    impl<'a> ServiceCtx<'a> {
        /// Creates a new service context for device execution.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn new(
            now_ms: u64,
            link_ip: Option<[u8; 4]>,
            config: &'a ConfigHandle,
            sockets: &'a mut SocketSet<'static>,
        ) -> Self {
            Self {
                now_ms,
                link_ip,
                config,
                audio: None,
                sockets,
            }
        }

        /// Creates a new service context for host tests.
        #[cfg(not(target_arch = "xtensa"))]
        #[must_use]
        pub fn new(now_ms: u64, link_ip: Option<[u8; 4]>, config: &'a ConfigHandle) -> Self {
            Self {
                now_ms,
                link_ip,
                config,
                audio: None,
            }
        }

        /// Attaches an audio/volume host to this service context.
        #[must_use]
        pub fn with_audio(mut self, audio: &'a mut dyn ServiceAudioHost) -> Self {
            self.audio = Some(audio);
            self
        }

        /// Returns `true` when an audio host is attached to this context.
        #[must_use]
        pub fn has_audio(&self) -> bool {
            self.audio.is_some()
        }

        /// Claims external 16-bit stereo PCM playback for `source`.
        ///
        /// Returns `false` if no audio host is attached or if a higher-priority
        /// source currently owns the pipeline.
        pub fn claim_external_pcm(
            &mut self,
            source: AudioSource,
            sample_rate_hz: Option<u32>,
            amp_power: AmpPower,
        ) -> bool {
            self.audio
                .as_deref_mut()
                .is_some_and(|audio| audio.claim_external_pcm(source, sample_rate_hz, amp_power))
        }

        /// Releases `source` and restores any suspended source or stops playback.
        ///
        /// Returns `false` when no audio host is attached.
        pub fn release_external(&mut self, source: AudioSource) -> bool {
            match self.audio.as_deref_mut() {
                Some(audio) => {
                    audio.release_external(source);
                    true
                }
                None => false,
            }
        }

        /// Applies a clamped `0..=100` local volume percentage via the attached
        /// audio host, returning the normalized percentage or `None` if detached.
        pub fn set_local_volume(&mut self, percent: u8) -> Option<u8> {
            self.audio
                .as_deref_mut()
                .map(|audio| audio.set_local_volume(percent))
        }

        /// Monotonic time in milliseconds since boot.
        #[must_use]
        pub fn now_ms(&self) -> u64 {
            self.now_ms
        }

        /// Current IPv4 address when the network link is up.
        #[must_use]
        pub fn link_ip(&self) -> Option<[u8; 4]> {
            self.link_ip
        }

        /// Read access to the shared configuration handle.
        #[must_use]
        pub fn config(&self) -> &'a ConfigHandle {
            self.config
        }

        /// Registers a smoltcp socket in the shared socket set.
        #[cfg(target_arch = "xtensa")]
        pub fn add_socket<T: AnySocket<'static>>(&mut self, socket: T) -> SocketHandle {
            self.sockets.add(socket)
        }

        /// Returns a shared reference to a registered socket by handle.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn socket<T: AnySocket<'static>>(&self, handle: SocketHandle) -> &T {
            self.sockets.get::<T>(handle)
        }

        /// Returns a mutable reference to a registered socket by handle.
        #[cfg(target_arch = "xtensa")]
        pub fn socket_mut<T: AnySocket<'static>>(&mut self, handle: SocketHandle) -> &mut T {
            self.sockets.get_mut::<T>(handle)
        }

        /// Returns a shared reference to the underlying smoltcp socket set.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn sockets(&self) -> &SocketSet<'static> {
            self.sockets
        }

        /// Returns a mutable reference to the underlying smoltcp socket set.
        #[cfg(target_arch = "xtensa")]
        pub fn sockets_mut(&mut self) -> &mut SocketSet<'static> {
            self.sockets
        }
    }

    /// Lifecycle interface implemented by every plug-in device service.
    pub trait DeviceService {
        /// Static human-readable service name for diagnostics.
        fn name(&self) -> &'static str;
        /// Called once in bring-up. Adds this service's sockets.
        fn init(&mut self, ctx: &mut ServiceCtx<'_>);
        /// Called when the link gains or loses its address.
        fn on_link(&mut self, ctx: &mut ServiceCtx<'_>, up: bool);
        /// Called once per loop lap. Must return without blocking.
        fn poll(&mut self, ctx: &mut ServiceCtx<'_>);
        /// Appends this service's fields to the status report.
        fn status(&self, out: &mut StatusExtras);
    }

    /// Heap-backed registry driving all wired [`DeviceService`] instances.
    #[derive(Default)]
    pub struct ServiceRegistry {
        services: Vec<Box<dyn DeviceService>>,
    }

    impl ServiceRegistry {
        /// Creates an empty service registry.
        #[must_use]
        pub fn new() -> Self {
            Self {
                services: Vec::new(),
            }
        }

        /// Registers a service instance on the heap.
        pub fn register<S: DeviceService + 'static>(&mut self, service: S) {
            self.services.push(Box::new(service));
        }

        /// Registers an already-boxed service instance.
        pub fn register_boxed(&mut self, service: Box<dyn DeviceService>) {
            self.services.push(service);
        }

        /// Number of registered services.
        #[must_use]
        pub fn len(&self) -> usize {
            self.services.len()
        }

        /// Returns `true` when no services are registered.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.services.is_empty()
        }

        /// Invokes `init` on every service in registration order.
        pub fn init_all(&mut self, ctx: &mut ServiceCtx<'_>) {
            for service in &mut self.services {
                service.init(ctx);
            }
        }

        /// Invokes `on_link` on every service in registration order.
        pub fn on_link_all(&mut self, ctx: &mut ServiceCtx<'_>, up: bool) {
            for service in &mut self.services {
                service.on_link(ctx, up);
            }
        }

        /// Invokes `poll` on every service in registration order.
        pub fn poll_all(&mut self, ctx: &mut ServiceCtx<'_>) {
            for service in &mut self.services {
                service.poll(ctx);
            }
        }

        /// Invokes `status` on every service in registration order.
        pub fn status_all(&self, out: &mut StatusExtras) {
            for service in &self.services {
                service.status(out);
            }
        }
    }

    /// Registers every device service in exactly one place.
    ///
    /// Task 1 leaves this empty until Task 4 migrates the inline protocol
    /// services out of `main.rs`.
    pub fn register_all(reg: &mut ServiceRegistry) {
        let _ = reg;
    }
}

pub use registry::{
    register_all, AmpPower, DeviceService, ServiceAudioAdapter, ServiceAudioHost, ServiceCtx,
    ServiceRegistry, StatusExtras, StatusField, StatusValue,
};

#[cfg(test)]
mod tests {
    extern crate alloc;

    use core::cell::RefCell;

    use alloc::rc::Rc;
    use alloc::vec;
    use alloc::vec::Vec;
    use squeezelite_config::AudioConfig;
    use squeezelite_decoders::PcmDecoder;

    use super::*;
    use crate::config_handle::ConfigHandle;
    use crate::coordinator::AudioSource;
    use crate::coordinator::PlayerCoordinator;
    use crate::coordinator::PlayerState;
    use crate::io_traits::MockBackend;
    use crate::pipeline::ActiveDecoder;
    use crate::pipeline::StreamPipeline;
    use crate::sink::SimulatedSink;

    struct RecordingService {
        name: &'static str,
        events: Rc<RefCell<Vec<(&'static str, &'static str)>>>,
    }

    impl DeviceService for RecordingService {
        fn name(&self) -> &'static str {
            self.name
        }

        fn init(&mut self, _ctx: &mut ServiceCtx<'_>) {
            self.events.borrow_mut().push((self.name, "init"));
        }

        fn on_link(&mut self, _ctx: &mut ServiceCtx<'_>, up: bool) {
            let label = if up { "link_up" } else { "link_down" };
            self.events.borrow_mut().push((self.name, label));
        }

        fn poll(&mut self, _ctx: &mut ServiceCtx<'_>) {
            self.events.borrow_mut().push((self.name, "poll"));
        }

        fn status(&self, out: &mut StatusExtras) {
            out.push_bool(self.name, true);
        }
    }

    #[test]
    fn registry_drives_two_services_in_registration_order() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut reg = ServiceRegistry::new();
        reg.register(RecordingService {
            name: "alpha",
            events: Rc::clone(&log),
        });
        reg.register(RecordingService {
            name: "beta",
            events: Rc::clone(&log),
        });
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());

        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut init_ctx = ServiceCtx::new(10, None, &cfg);
        reg.init_all(&mut init_ctx);

        let mut up_ctx = ServiceCtx::new(20, Some([172, 22, 1, 99]), &cfg);
        reg.on_link_all(&mut up_ctx, true);

        let mut poll1_ctx = ServiceCtx::new(30, Some([172, 22, 1, 99]), &cfg);
        reg.poll_all(&mut poll1_ctx);

        let mut down_ctx = ServiceCtx::new(40, None, &cfg);
        reg.on_link_all(&mut down_ctx, false);

        let mut poll2_ctx = ServiceCtx::new(50, None, &cfg);
        reg.poll_all(&mut poll2_ctx);

        let mut extras = StatusExtras::new();
        reg.status_all(&mut extras);

        assert_eq!(
            *log.borrow(),
            vec![
                ("alpha", "init"),
                ("beta", "init"),
                ("alpha", "link_up"),
                ("beta", "link_up"),
                ("alpha", "poll"),
                ("beta", "poll"),
                ("alpha", "link_down"),
                ("beta", "link_down"),
                ("alpha", "poll"),
                ("beta", "poll"),
            ]
        );
        assert_eq!(extras.len(), 2);
        assert_eq!(extras.fields()[0].key, "alpha");
        assert_eq!(extras.fields()[0].value, StatusValue::Bool(true));
        assert_eq!(extras.fields()[1].key, "beta");
        assert_eq!(extras.fields()[1].value, StatusValue::Bool(true));
    }

    #[test]
    fn service_ctx_detached_audio_returns_defaults() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut ctx = ServiceCtx::new(0, None, &cfg);
        assert!(!ctx.has_audio());
        assert!(!ctx.claim_external_pcm(AudioSource::Spotify, Some(44_100), AmpPower::EnsurePlay));
        assert!(!ctx.release_external(AudioSource::Spotify));
        assert_eq!(ctx.set_local_volume(55), None);
    }

    #[test]
    fn service_ctx_claim_and_release_external_pcm_transitions_pipeline_and_arbiter() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut coord = PlayerCoordinator::new([0x24, 0x0A, 0xC4, 0, 0, 1], "test");
        let mut stream_mem = vec![0u8; 4096];
        let mut out_mem = vec![0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            SimulatedSink::new(44_100),
            44_100,
        );
        let mut backend = MockBackend::new();

        // Simulate suspended SlimProto stream before Bluetooth preempts it.
        assert!(coord.request_source(AudioSource::SlimProto));

        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(100, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.has_audio());

            // KeepCurrent leaves backend.powered false.
            assert!(ctx.claim_external_pcm(
                AudioSource::Bluetooth,
                Some(48_000),
                AmpPower::KeepCurrent,
            ));
        }

        assert_eq!(coord.active_source(), AudioSource::Bluetooth);
        assert_eq!(coord.current_sample_rate, 48_000);
        assert_eq!(pipeline.sample_rate, 48_000);
        assert!(coord.external_playback);
        assert_eq!(coord.state, PlayerState::Playing);
        assert!(!backend.powered);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));

        // Higher-priority AirPlay preempts Bluetooth and powers the amp.
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(200, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.claim_external_pcm(
                AudioSource::AirPlay,
                Some(44_100),
                AmpPower::EnsurePlay,
            ));
            // Lower-priority Spotify request is rejected while AirPlay is active.
            assert!(!ctx.claim_external_pcm(
                AudioSource::Spotify,
                Some(44_100),
                AmpPower::EnsurePlay,
            ));
        }
        assert_eq!(coord.active_source(), AudioSource::AirPlay);
        assert!(backend.powered);

        // Releasing AirPlay drops back to suspended Spotify (stays on 44.1 kHz PCM).
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(300, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.release_external(AudioSource::AirPlay));
        }
        assert_eq!(coord.active_source(), AudioSource::Spotify);
        assert_eq!(pipeline.sample_rate, 44_100);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Pcm(_)));

        // Releasing Spotify and Bluetooth drops to SlimProto -> none.
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(400, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.release_external(AudioSource::Spotify));
        }
        assert_eq!(coord.active_source(), AudioSource::Bluetooth);
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(450, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.release_external(AudioSource::Bluetooth));
        }
        assert_eq!(coord.active_source(), AudioSource::SlimProto);
        assert!(!coord.external_playback);

        // Releasing SlimProto transitions to AudioSource::None and MP3 decoder.
        {
            let mut adapter = ServiceAudioAdapter::new(&mut coord, &mut pipeline, &mut backend);
            let mut ctx = ServiceCtx::new(500, Some([10, 0, 0, 2]), &cfg).with_audio(&mut adapter);
            assert!(ctx.release_external(AudioSource::SlimProto));
        }
        assert_eq!(coord.active_source(), AudioSource::None);
        assert_eq!(coord.state, PlayerState::Stopped);
        assert!(matches!(pipeline.decoder, ActiveDecoder::Mp3(_)));
    }

    #[test]
    fn service_ctx_set_local_volume_clamps_and_notifies_cyd() {
        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut coord = PlayerCoordinator::new([0x24, 0x0A, 0xC4, 0, 0, 1], "test");
        let mut stream_mem = vec![0u8; 4096];
        let mut out_mem = vec![0u8; 4096];
        let mut pipeline = StreamPipeline::new(
            &mut stream_mem,
            &mut out_mem,
            ActiveDecoder::Pcm(PcmDecoder::new(16, 2, true)),
            SimulatedSink::new(44_100),
            44_100,
        );
        let mut backend = MockBackend::new();
        let mut notified = Vec::new();

        {
            let mut adapter = ServiceAudioAdapter::with_cyd_volume(
                &mut coord,
                &mut pipeline,
                &mut backend,
                |pct| notified.push(pct),
            );
            let mut ctx = ServiceCtx::new(600, None, &cfg).with_audio(&mut adapter);
            assert_eq!(ctx.set_local_volume(150), Some(100));
            assert_eq!(ctx.set_local_volume(42), Some(42));
        }

        assert_eq!(notified, vec![100, 42]);
        assert_ne!(backend.hw_vol, (0, 0));
    }
}
