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

    /// Per-call runtime context passed to [`DeviceService`] lifecycle hooks.
    pub struct ServiceCtx<'a> {
        now_ms: u64,
        link_ip: Option<[u8; 4]>,
        config: &'a ConfigHandle,
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
            }
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
    register_all, DeviceService, ServiceCtx, ServiceRegistry, StatusExtras, StatusField,
    StatusValue,
};

#[cfg(test)]
mod tests {
    extern crate alloc;

    use core::cell::RefCell;

    use alloc::rc::Rc;
    use alloc::vec;
    use alloc::vec::Vec;
    use squeezelite_config::AudioConfig;

    use super::*;
    use crate::config_handle::ConfigHandle;

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
}
