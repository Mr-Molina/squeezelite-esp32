//! Device service registry and lifecycle contract (F4 Task 1).
//!
//! Each device network or background feature implements [`DeviceService`] and
//! is registered exactly once inside [`register_all`].

pub mod airplay;
pub mod mdns;
pub mod settings;
pub mod spotify_facade;

mod registry {
    extern crate alloc;

    use alloc::boxed::Box;
    use alloc::collections::VecDeque;
    use alloc::string::String;
    use alloc::vec::Vec;

    #[cfg(target_arch = "xtensa")]
    use smoltcp::iface::{SocketHandle, SocketSet};
    #[cfg(target_arch = "xtensa")]
    use smoltcp::socket::AnySocket;

    use crate::config_handle::ConfigHandle;
    use crate::coordinator::AudioSource;
    use crate::coordinator::PlayerCoordinator;
    use crate::coordinator::SlimExtension;
    use crate::coordinator::SlimOutboundSink;
    use crate::io_traits::AudioBackend;
    use crate::io_traits::TransportError;
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

        /// Requests source ownership without reconfiguring decoder or amp.
        fn request_external_source(&mut self, source: AudioSource) -> bool;

        /// Ingests raw 16-bit stereo PCM bytes into the stream buffer.
        fn ingest_external_pcm(&mut self, bytes: &[u8]) -> bool;

        /// Flushes pending stream and output audio buffers.
        fn flush_external_buffers(&mut self);

        /// Returns `true` while external audio samples remain buffered.
        fn has_external_audio_queued(&self) -> bool;

        /// Returns the currently active audio source on the coordinator.
        fn active_source(&self) -> AudioSource;
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

        fn request_external_source(&mut self, source: AudioSource) -> bool {
            self.coordinator.request_source(source)
        }

        fn ingest_external_pcm(&mut self, bytes: &[u8]) -> bool {
            self.pipeline.ingest_stream_data(bytes).is_ok()
        }

        fn flush_external_buffers(&mut self) {
            self.pipeline.stream_buffer.flush();
            self.pipeline.output_buffer.flush();
        }

        fn has_external_audio_queued(&self) -> bool {
            self.pipeline.stream_buffer.available_read() > 0
                || self.pipeline.output_buffer.available_read() > 0
                || self.pipeline.has_pending_pcm()
        }

        fn active_source(&self) -> AudioSource {
            self.coordinator.active_source()
        }
    }

    /// Default maximum number of queued outbound SlimProto frames.
    pub const DEFAULT_SLIM_OUTBOUND_CAP: usize = 32;

    /// PSRAM-heap-backed outbound SlimProto frame queue shared between
    /// [`DeviceService`] callbacks and [`SlimExtension::poll_outbound`].
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SlimOutboundQueue {
        frames: VecDeque<([u8; 4], Vec<u8>)>,
        capacity: usize,
    }

    impl Default for SlimOutboundQueue {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SlimOutboundQueue {
        /// Creates an empty queue with [`DEFAULT_SLIM_OUTBOUND_CAP`].
        #[must_use]
        pub fn new() -> Self {
            Self::with_capacity(DEFAULT_SLIM_OUTBOUND_CAP)
        }

        /// Creates an empty queue capped at `capacity` frames (`>= 1`).
        #[must_use]
        pub fn with_capacity(capacity: usize) -> Self {
            Self {
                frames: VecDeque::new(),
                capacity: capacity.max(1),
            }
        }

        /// Queues `(opcode, payload)` onto the heap if below capacity.
        pub fn push(&mut self, opcode: [u8; 4], payload: &[u8]) -> bool {
            if self.frames.len() >= self.capacity {
                return false;
            }
            self.frames.push_back((opcode, payload.to_vec()));
            true
        }

        /// Number of queued outbound frames.
        #[must_use]
        pub fn len(&self) -> usize {
            self.frames.len()
        }

        /// Returns `true` when no frames are queued.
        #[must_use]
        pub fn is_empty(&self) -> bool {
            self.frames.is_empty()
        }

        /// Flushes queued frames to `sink`, preserving any unsent frame on
        /// error so it can be retried on the next tick.
        pub fn drain_to(&mut self, sink: &mut dyn SlimOutboundSink) -> Result<(), TransportError> {
            while let Some((opcode, payload)) = self.frames.front() {
                sink.send_slim_frame(*opcode, payload)?;
                let _ = self.frames.pop_front();
            }
            Ok(())
        }
    }

    /// Parsed HTTP request metadata passed to [`DeviceService::handle_http`].
    #[non_exhaustive]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct ServiceHttpRequest<'a> {
        /// HTTP method (e.g. `"GET"`, `"POST"`).
        pub method: &'a str,
        /// Request path without query string (e.g. `"/status.json"`).
        pub path: &'a str,
        /// Request body bytes following the header terminator.
        pub body: &'a [u8],
    }

    impl<'a> ServiceHttpRequest<'a> {
        /// Constructs a [`ServiceHttpRequest`].
        #[must_use]
        pub const fn new(method: &'a str, path: &'a str, body: &'a [u8]) -> Self {
            Self { method, path, body }
        }

        /// Parses method, path (query stripped), and body from raw HTTP bytes.
        ///
        /// Returns `None` when the request line is incomplete or non-UTF-8.
        #[must_use]
        pub fn parse(req_bytes: &'a [u8]) -> Option<Self> {
            let line_end = req_bytes.iter().position(|&b| b == b'\n')?;
            let first_line = core::str::from_utf8(&req_bytes[..line_end])
                .ok()?
                .trim_end_matches('\r');
            let mut parts = first_line.split_whitespace();
            let method = parts.next()?;
            let raw_path = parts.next()?;
            let path = raw_path.split('?').next().unwrap_or(raw_path);
            let body = req_bytes
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|p| &req_bytes[p + 4..])
                .or_else(|| {
                    req_bytes
                        .windows(2)
                        .position(|w| w == b"\n\n")
                        .map(|p| &req_bytes[p + 2..])
                })
                .unwrap_or(&[]);
            Some(Self { method, path, body })
        }
    }

    /// Outcome returned by [`DeviceService::handle_http`].
    #[non_exhaustive]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ServiceHttpOutcome {
        /// This service does not handle the request; continue dispatch.
        Unhandled,
        /// This service wrote an HTTP response into `resp_buf`.
        Responded {
            /// Number of bytes written to `resp_buf`.
            response_len: usize,
            /// Whether the socket should abort immediately rather than linger.
            close_immediately: bool,
        },
    }

    /// Host-testable settings persistence bridge attached to [`ServiceCtx`].
    pub trait SettingsPersistHost {
        /// Polls debounced configuration persistence and flushes dirty state to
        /// backing storage when ready.
        ///
        /// Returns `true` when an image was written to storage.
        fn poll_and_persist(&mut self, config: &ConfigHandle, now_ms: u64) -> bool;
    }

    /// Per-call runtime context passed to [`DeviceService`] lifecycle hooks.
    pub struct ServiceCtx<'a> {
        now_ms: u64,
        link_ip: Option<[u8; 4]>,
        sta_mac: [u8; 6],
        config: &'a ConfigHandle,
        audio: Option<&'a mut dyn ServiceAudioHost>,
        slim_outbound: Option<&'a mut SlimOutboundQueue>,
        settings_persist: Option<&'a mut dyn SettingsPersistHost>,
        #[cfg(target_arch = "xtensa")]
        sockets: Option<&'a mut SocketSet<'static>>,
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
                sta_mac: [0; 6],
                config,
                audio: None,
                slim_outbound: None,
                settings_persist: None,
                sockets: Some(sockets),
            }
        }

        /// Creates a device context without socket-set access (used during
        /// SlimProto frame callbacks while sockets are borrowed).
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn without_sockets(
            now_ms: u64,
            link_ip: Option<[u8; 4]>,
            config: &'a ConfigHandle,
        ) -> Self {
            Self {
                now_ms,
                link_ip,
                sta_mac: [0; 6],
                config,
                audio: None,
                slim_outbound: None,
                settings_persist: None,
                sockets: None,
            }
        }

        /// Creates a new service context for host tests.
        #[cfg(not(target_arch = "xtensa"))]
        #[must_use]
        pub fn new(now_ms: u64, link_ip: Option<[u8; 4]>, config: &'a ConfigHandle) -> Self {
            Self {
                now_ms,
                link_ip,
                sta_mac: [0; 6],
                config,
                audio: None,
                slim_outbound: None,
                settings_persist: None,
            }
        }

        /// Creates a context without socket-set access (host mirror of device
        /// SlimProto callback contexts).
        #[cfg(not(target_arch = "xtensa"))]
        #[must_use]
        pub fn without_sockets(
            now_ms: u64,
            link_ip: Option<[u8; 4]>,
            config: &'a ConfigHandle,
        ) -> Self {
            Self::new(now_ms, link_ip, config)
        }

        /// Attaches the station Wi-Fi MAC address to this service context.
        #[must_use]
        pub fn with_sta_mac(mut self, sta_mac: [u8; 6]) -> Self {
            self.sta_mac = sta_mac;
            self
        }

        /// Attaches an audio/volume host to this service context.
        #[must_use]
        pub fn with_audio(mut self, audio: &'a mut dyn ServiceAudioHost) -> Self {
            self.audio = Some(audio);
            self
        }

        /// Attaches an outbound SlimProto frame queue to this service context.
        #[must_use]
        pub fn with_slim_outbound(mut self, queue: &'a mut SlimOutboundQueue) -> Self {
            self.slim_outbound = Some(queue);
            self
        }

        /// Attaches a settings persistence host to this service context.
        #[must_use]
        pub fn with_settings_persist(mut self, host: &'a mut dyn SettingsPersistHost) -> Self {
            self.settings_persist = Some(host);
            self
        }

        /// Polls the attached settings persistence host, if any.
        ///
        /// Returns `true` when an image was flushed to backing storage.
        pub fn poll_settings_persist(&mut self) -> bool {
            let now_ms = self.now_ms;
            let config: &ConfigHandle = self.config;
            match self.settings_persist.as_deref_mut() {
                Some(host) => host.poll_and_persist(config, now_ms),
                None => false,
            }
        }

        /// Queues an outbound SlimProto frame `(opcode, payload)` for delivery
        /// on the active SlimProto control connection.
        ///
        /// Returns `false` if no outbound queue is attached or the queue is
        /// full.
        pub fn send_slim_frame(&mut self, opcode: [u8; 4], payload: &[u8]) -> bool {
            self.slim_outbound
                .as_deref_mut()
                .is_some_and(|queue| queue.push(opcode, payload))
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

        /// Requests coordinator ownership for `source` without reconfiguring
        /// the decoder or amplifier power state.
        pub fn request_external_source(&mut self, source: AudioSource) -> bool {
            self.audio
                .as_deref_mut()
                .is_some_and(|audio| audio.request_external_source(source))
        }

        /// Ingests raw PCM bytes into the pipeline stream buffer.
        pub fn ingest_external_pcm(&mut self, bytes: &[u8]) -> bool {
            self.audio
                .as_deref_mut()
                .is_some_and(|audio| audio.ingest_external_pcm(bytes))
        }

        /// Flushes both the stream and output ring buffers.
        pub fn flush_external_buffers(&mut self) {
            if let Some(audio) = self.audio.as_deref_mut() {
                audio.flush_external_buffers();
            }
        }

        /// Returns `true` when external PCM bytes remain queued in the
        /// pipeline.
        #[must_use]
        pub fn has_external_audio_queued(&self) -> bool {
            self.audio
                .as_deref()
                .is_some_and(ServiceAudioHost::has_external_audio_queued)
        }

        /// Currently active audio source when an audio host is attached.
        #[must_use]
        pub fn active_source(&self) -> Option<AudioSource> {
            self.audio.as_deref().map(ServiceAudioHost::active_source)
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

        /// Station Wi-Fi MAC address associated with the network interface.
        #[must_use]
        pub fn sta_mac(&self) -> [u8; 6] {
            self.sta_mac
        }

        /// Read access to the shared configuration handle.
        #[must_use]
        pub fn config(&self) -> &'a ConfigHandle {
            self.config
        }

        /// Returns `true` when socket-set access is attached to this context.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn has_sockets(&self) -> bool {
            self.sockets.is_some()
        }

        /// Registers a smoltcp socket in the shared socket set.
        #[cfg(target_arch = "xtensa")]
        pub fn add_socket<T: AnySocket<'static>>(&mut self, socket: T) -> SocketHandle {
            self.sockets
                .as_deref_mut()
                .expect("ServiceCtx has no SocketSet attached")
                .add(socket)
        }

        /// Returns a shared reference to a registered socket by handle.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn socket<T: AnySocket<'static>>(&self, handle: SocketHandle) -> &T {
            self.sockets
                .as_deref()
                .expect("ServiceCtx has no SocketSet attached")
                .get::<T>(handle)
        }

        /// Returns a mutable reference to a registered socket by handle.
        #[cfg(target_arch = "xtensa")]
        pub fn socket_mut<T: AnySocket<'static>>(&mut self, handle: SocketHandle) -> &mut T {
            self.sockets
                .as_deref_mut()
                .expect("ServiceCtx has no SocketSet attached")
                .get_mut::<T>(handle)
        }

        /// Returns a shared reference to the underlying smoltcp socket set.
        #[cfg(target_arch = "xtensa")]
        #[must_use]
        pub fn sockets(&self) -> &SocketSet<'static> {
            self.sockets
                .as_deref()
                .expect("ServiceCtx has no SocketSet attached")
        }

        /// Returns a mutable reference to the underlying smoltcp socket set.
        #[cfg(target_arch = "xtensa")]
        pub fn sockets_mut(&mut self) -> &mut SocketSet<'static> {
            self.sockets
                .as_deref_mut()
                .expect("ServiceCtx has no SocketSet attached")
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

        /// Handles an inbound SlimProto frame `(opcode, payload)`.
        ///
        /// Return `true` when handled exclusively (suppressing default
        /// coordinator handling), `false` otherwise.
        fn on_slim_frame(
            &mut self,
            _ctx: &mut ServiceCtx<'_>,
            _opcode: [u8; 4],
            _payload: &[u8],
        ) -> bool {
            false
        }

        /// Handles an incoming HTTP request if owned by this service.
        fn handle_http(
            &mut self,
            _ctx: &mut ServiceCtx<'_>,
            _req: &ServiceHttpRequest<'_>,
            _resp_buf: &mut [u8],
        ) -> ServiceHttpOutcome {
            ServiceHttpOutcome::Unhandled
        }
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

        /// Routes an inbound SlimProto frame across registered services,
        /// stopping at the first service that returns `true`.
        pub fn on_slim_frame_all(
            &mut self,
            ctx: &mut ServiceCtx<'_>,
            opcode: [u8; 4],
            payload: &[u8],
        ) -> bool {
            for service in &mut self.services {
                if service.on_slim_frame(ctx, opcode, payload) {
                    return true;
                }
            }
            false
        }

        /// Dispatches an HTTP request across registered services, stopping at
        /// the first service that returns [`ServiceHttpOutcome::Responded`].
        pub fn handle_http_all(
            &mut self,
            ctx: &mut ServiceCtx<'_>,
            req: &ServiceHttpRequest<'_>,
            resp_buf: &mut [u8],
        ) -> ServiceHttpOutcome {
            for service in &mut self.services {
                match service.handle_http(ctx, req, resp_buf) {
                    ServiceHttpOutcome::Unhandled => {}
                    outcome @ ServiceHttpOutcome::Responded { .. } => return outcome,
                }
            }
            ServiceHttpOutcome::Unhandled
        }
    }

    /// Bridges [`ServiceRegistry`] and [`SlimOutboundQueue`] to
    /// [`PlayerCoordinator::tick_with_extension`] via [`SlimExtension`].
    pub struct ServiceSlimExtension<'a> {
        services: &'a mut ServiceRegistry,
        outbound: &'a mut SlimOutboundQueue,
        now_ms: u64,
        link_ip: Option<[u8; 4]>,
        config: &'a ConfigHandle,
    }

    impl<'a> ServiceSlimExtension<'a> {
        /// Creates a [`SlimExtension`] bridge for a single coordinator tick.
        #[must_use]
        pub fn new(
            services: &'a mut ServiceRegistry,
            outbound: &'a mut SlimOutboundQueue,
            now_ms: u64,
            link_ip: Option<[u8; 4]>,
            config: &'a ConfigHandle,
        ) -> Self {
            Self {
                services,
                outbound,
                now_ms,
                link_ip,
                config,
            }
        }
    }

    impl SlimExtension for ServiceSlimExtension<'_> {
        fn on_frame(&mut self, opcode: [u8; 4], payload: &[u8]) -> bool {
            let mut ctx = ServiceCtx::without_sockets(self.now_ms, self.link_ip, self.config)
                .with_slim_outbound(self.outbound);
            self.services.on_slim_frame_all(&mut ctx, opcode, payload)
        }

        fn poll_outbound(
            &mut self,
            sink: &mut dyn SlimOutboundSink,
            _now_ms: u32,
        ) -> Result<(), TransportError> {
            self.outbound.drain_to(sink)
        }
    }

    /// Registers every device service in exactly one place.
    pub fn register_all(reg: &mut ServiceRegistry) {
        reg.register(super::mdns::MdnsService::new());
        reg.register(super::airplay::AirPlayService::new());
        reg.register(super::spotify_facade::SpotifyFacadeService::new());
        reg.register(super::settings::SettingsService::new());
    }
}

pub use airplay::AirPlayService;
pub use mdns::{
    can_send_mdns_multicast_reply, is_mdns_service_query, MdnsService, MDNS_MULTICAST_COOLDOWN_MS,
};
pub use registry::{
    register_all, AmpPower, DeviceService, ServiceAudioAdapter, ServiceAudioHost, ServiceCtx,
    ServiceHttpOutcome, ServiceHttpRequest, ServiceRegistry, ServiceSlimExtension,
    SettingsPersistHost, SlimOutboundQueue, StatusExtras, StatusField, StatusValue,
    DEFAULT_SLIM_OUTBOUND_CAP,
};
pub use settings::SettingsService;
pub use spotify_facade::SpotifyFacadeService;

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
    use crate::io_traits::{
        MockBackend, NetAddr, SlimTransport, StreamError, StreamSource, TransportError,
    };
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
        assert!(!ctx.request_external_source(AudioSource::AirPlay));
        assert!(!ctx.ingest_external_pcm(&[1, 2, 3, 4]));
        assert!(!ctx.has_external_audio_queued());
        ctx.flush_external_buffers();
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
            assert!(ctx.request_external_source(AudioSource::AirPlay));
            assert!(!ctx.has_external_audio_queued());
            assert!(ctx.ingest_external_pcm(&[0u8; 64]));
            assert!(ctx.has_external_audio_queued());
            ctx.flush_external_buffers();
            assert!(!ctx.has_external_audio_queued());
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

    struct MockSlimTransport {
        inbound: alloc::collections::VecDeque<Vec<u8>>,
        outbound: Vec<Vec<u8>>,
        fail_send: Option<TransportError>,
        connected: bool,
    }

    impl MockSlimTransport {
        fn new() -> Self {
            Self {
                inbound: alloc::collections::VecDeque::new(),
                outbound: Vec::new(),
                fail_send: None,
                connected: true,
            }
        }
    }

    impl SlimTransport for MockSlimTransport {
        fn poll_frame(&mut self, buf: &mut [u8]) -> Result<Option<usize>, TransportError> {
            let Some(frame) = self.inbound.pop_front() else {
                return Ok(None);
            };
            if frame.len() > buf.len() {
                return Err(TransportError::Io);
            }
            buf[..frame.len()].copy_from_slice(&frame);
            Ok(Some(frame.len()))
        }

        fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            if let Some(ref err) = self.fail_send {
                return Err(match err {
                    TransportError::Disconnected => TransportError::Disconnected,
                    TransportError::WouldBlock => TransportError::WouldBlock,
                    TransportError::Io => TransportError::Io,
                });
            }
            self.outbound.push(frame.to_vec());
            Ok(())
        }

        fn connected(&self) -> bool {
            self.connected
        }
    }

    struct DummyStreamSource;

    impl StreamSource for DummyStreamSource {
        fn connect(&mut self, _addr: NetAddr, _http_request: &[u8]) -> Result<(), StreamError> {
            Ok(())
        }

        fn poll_bytes(&mut self, _out: &mut [u8]) -> Result<usize, StreamError> {
            Ok(0)
        }

        fn take_response_headers(&mut self) -> Option<crate::io_traits::HeaderBuf> {
            None
        }

        fn take_icy_metadata(&mut self) -> Option<crate::io_traits::MetaBuf> {
            None
        }

        fn set_icy_metaint(&mut self, _interval: usize) {}

        fn is_eof(&self) -> bool {
            false
        }

        fn abort(&mut self) {}
    }

    type SeenFrameLog = Rc<RefCell<Vec<([u8; 4], Vec<u8>)>>>;

    struct CustomSlimService {
        seen_frames: SeenFrameLog,
        intercept_opcode: [u8; 4],
        reply_opcode: Option<[u8; 4]>,
    }

    impl DeviceService for CustomSlimService {
        fn name(&self) -> &'static str {
            "custom-slim"
        }

        fn init(&mut self, _ctx: &mut ServiceCtx<'_>) {}

        fn on_link(&mut self, _ctx: &mut ServiceCtx<'_>, _up: bool) {}

        fn poll(&mut self, _ctx: &mut ServiceCtx<'_>) {}

        fn status(&self, _out: &mut StatusExtras) {}

        fn on_slim_frame(
            &mut self,
            ctx: &mut ServiceCtx<'_>,
            opcode: [u8; 4],
            payload: &[u8],
        ) -> bool {
            self.seen_frames
                .borrow_mut()
                .push((opcode, payload.to_vec()));
            if opcode == self.intercept_opcode {
                if let Some(reply) = self.reply_opcode {
                    assert!(ctx.send_slim_frame(reply, b"pong"));
                }
                true
            } else {
                false
            }
        }
    }

    #[test]
    fn slim_extension_intercepts_custom_frame_bypasses_others_and_drains_outbound() {
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
        let mut source = DummyStreamSource;
        let mut transport = MockSlimTransport::new();

        // 6-byte header: len=8 (4 opcode + 4 payload), "XTND", payload="ping".
        transport
            .inbound
            .push_back(vec![0, 8, b'X', b'T', b'N', b'D', b'p', b'i', b'n', b'g']);
        // Standard "serv" frame falls through when on_frame returns false.
        transport
            .inbound
            .push_back(vec![0, 8, b's', b'e', b'r', b'v', 10, 0, 0, 99]);

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut reg = ServiceRegistry::new();
        reg.register(CustomSlimService {
            seen_frames: Rc::clone(&seen),
            intercept_opcode: *b"XTND",
            reply_opcode: Some(*b"PONG"),
        });
        let mut outbound = SlimOutboundQueue::new();

        // Tick 1: processes XTND exclusively and queues PONG reply.
        {
            let mut ext = ServiceSlimExtension::new(
                &mut reg,
                &mut outbound,
                100,
                Some([172, 22, 1, 15]),
                &cfg,
            );
            coord
                .tick_with_extension(
                    &mut transport,
                    &mut source,
                    &mut backend,
                    &mut pipeline,
                    &mut ext,
                    100,
                )
                .expect("tick 1 should succeed");
        }
        assert!(outbound.is_empty());
        assert_eq!(
            transport.outbound,
            vec![vec![
                b'P', b'O', b'N', b'G', 0, 0, 0, 4, b'p', b'o', b'n', b'g'
            ]]
        );

        // Tick 2: queue outbound from ServiceCtx; "vers" passes to coord.
        {
            let mut ctx = ServiceCtx::new(200, Some([172, 22, 1, 15]), &cfg)
                .with_slim_outbound(&mut outbound);
            assert!(ctx.send_slim_frame(*b"INFO", b"hi"));
        }
        {
            let mut ext = ServiceSlimExtension::new(
                &mut reg,
                &mut outbound,
                200,
                Some([172, 22, 1, 15]),
                &cfg,
            );
            coord
                .tick_with_extension(
                    &mut transport,
                    &mut source,
                    &mut backend,
                    &mut pipeline,
                    &mut ext,
                    200,
                )
                .expect("tick 2 should succeed");
        }
        assert_eq!(
            *seen.borrow(),
            vec![(*b"XTND", b"ping".to_vec()), (*b"serv", vec![10, 0, 0, 99]),]
        );
        assert_eq!(
            transport.outbound[1],
            vec![b'I', b'N', b'F', b'O', 0, 0, 0, 2, b'h', b'i']
        );
        assert_eq!(coord.take_server_switch(), Some([10, 0, 0, 99]));
    }

    struct HttpRouteService {
        prefix: &'static str,
        body: &'static [u8],
        close_immediately: bool,
        calls: Rc<RefCell<usize>>,
    }

    impl DeviceService for HttpRouteService {
        fn name(&self) -> &'static str {
            self.prefix
        }

        fn init(&mut self, _ctx: &mut ServiceCtx<'_>) {}

        fn on_link(&mut self, _ctx: &mut ServiceCtx<'_>, _up: bool) {}

        fn poll(&mut self, _ctx: &mut ServiceCtx<'_>) {}

        fn status(&self, _out: &mut StatusExtras) {}

        fn handle_http(
            &mut self,
            _ctx: &mut ServiceCtx<'_>,
            req: &ServiceHttpRequest<'_>,
            resp_buf: &mut [u8],
        ) -> ServiceHttpOutcome {
            *self.calls.borrow_mut() += 1;
            if req.path == self.prefix {
                let len = self.body.len().min(resp_buf.len());
                resp_buf[..len].copy_from_slice(&self.body[..len]);
                ServiceHttpOutcome::Responded {
                    response_len: len,
                    close_immediately: self.close_immediately,
                }
            } else {
                ServiceHttpOutcome::Unhandled
            }
        }
    }

    #[test]
    fn http_service_hook_dispatches_fallthrough_and_short_circuits() {
        let parsed = ServiceHttpRequest::parse(
            b"POST /api/pair?v=1 HTTP/1.1\r\nHost: 172.22.1.15\r\n\r\n{\"pin\":1234}",
        )
        .expect("valid HTTP request should parse");
        assert_eq!(parsed.method, "POST");
        assert_eq!(parsed.path, "/api/pair");
        assert_eq!(parsed.body, b"{\"pin\":1234}");

        let first_calls = Rc::new(RefCell::new(0usize));
        let second_calls = Rc::new(RefCell::new(0usize));
        let mut reg = ServiceRegistry::new();
        reg.register(HttpRouteService {
            prefix: "/api/first",
            body: b"HTTP/1.1 200 OK\r\n\r\nfirst",
            close_immediately: true,
            calls: Rc::clone(&first_calls),
        });
        reg.register(HttpRouteService {
            prefix: "/api/second",
            body: b"HTTP/1.1 200 OK\r\n\r\nsecond",
            close_immediately: false,
            calls: Rc::clone(&second_calls),
        });

        let cfg = ConfigHandle::new(AudioConfig::default());
        let mut ctx = ServiceCtx::new(10, Some([172, 22, 1, 15]), &cfg);
        let mut resp = [0u8; 128];

        // First returns Unhandled -> falls through to second, which responds.
        let req2 = ServiceHttpRequest::parse(b"GET /api/second HTTP/1.1\r\n\r\n")
            .expect("GET request parses");
        let out2 = reg.handle_http_all(&mut ctx, &req2, &mut resp);
        assert_eq!(
            out2,
            ServiceHttpOutcome::Responded {
                response_len: 25,
                close_immediately: false,
            }
        );
        assert_eq!(&resp[..25], b"HTTP/1.1 200 OK\r\n\r\nsecond");
        assert_eq!(*first_calls.borrow(), 1);
        assert_eq!(*second_calls.borrow(), 1);

        // First matches -> short-circuits before second is invoked.
        let req1 = ServiceHttpRequest::parse(b"GET /api/first HTTP/1.1\r\n\r\n")
            .expect("GET request parses");
        let out1 = reg.handle_http_all(&mut ctx, &req1, &mut resp);
        assert_eq!(
            out1,
            ServiceHttpOutcome::Responded {
                response_len: 24,
                close_immediately: true,
            }
        );
        assert_eq!(*first_calls.borrow(), 2);
        assert_eq!(*second_calls.borrow(), 1);

        // Unmatched route returns Unhandled after visiting both services.
        let req_miss = ServiceHttpRequest::parse(b"GET /status.json HTTP/1.1\r\n\r\n")
            .expect("GET request parses");
        assert_eq!(
            reg.handle_http_all(&mut ctx, &req_miss, &mut resp),
            ServiceHttpOutcome::Unhandled
        );
        assert_eq!(*first_calls.borrow(), 3);
        assert_eq!(*second_calls.borrow(), 2);
    }
}
