//! Shared, bounded invalidation tails. Exact historical replay remains on the legacy stream.
use super::*;
use devdesk_core::Engine;
use std::sync::{Weak, atomic::AtomicUsize};
use tokio::sync::{Notify, watch};

const PROCESS_BYTES: usize = 16 * 1024 * 1024;
const SCOPE_BYTES: usize = 1024 * 1024;
const EVENT_BYTES: usize = 64 * 1024;
const SCOPE_EVENTS: usize = 512;
static RETAINED_BYTES: AtomicUsize = AtomicUsize::new(0);
struct CachedEvent {
    id: u64,
    json: String,
}
impl Drop for CachedEvent {
    fn drop(&mut self) {
        RETAINED_BYTES.fetch_sub(self.json.len(), Ordering::Relaxed);
    }
}
impl CachedEvent {
    fn retain(event: devdesk_domain::Event, scope: &str) -> Option<Arc<Self>> {
        let mut value = json!(event);
        value["project_id"] = json!(scope);
        let json = value.to_string();
        if json.len() > EVENT_BYTES
            || RETAINED_BYTES
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                    used.checked_add(json.len())
                        .filter(|next| *next <= PROCESS_BYTES)
                })
                .is_err()
        {
            return None;
        }
        Some(Arc::new(Self { id: event.id, json }))
    }
}
#[derive(Clone, Default)]
struct Window {
    initialized: bool,
    /// Every cursor below this boundary requires a fresh authoritative read.
    floor: u64,
    cursor: u64,
    bytes: usize,
    events: VecDeque<Arc<CachedEvent>>,
    configuration_version: Value,
}
impl Window {
    fn omit(&mut self, cursor: u64) {
        self.events.clear();
        self.bytes = 0;
        self.cursor = cursor;
        self.floor = cursor;
    }
    fn append(&mut self, event: devdesk_domain::Event, scope: &str) {
        self.cursor = event.id;
        if let Some(event) = CachedEvent::retain(event, scope) {
            self.bytes += event.json.len();
            self.events.push_back(event);
            while self.events.len() > SCOPE_EVENTS || self.bytes > SCOPE_BYTES {
                let old = self.events.pop_front().unwrap();
                self.floor = old.id;
                self.bytes -= old.json.len();
            }
        } else {
            // Never advertise a contiguous cache across an omitted/oversized event.
            self.omit(self.cursor);
        }
    }
}
pub(crate) struct EventHub {
    scope: String,
    engine: Arc<Engine>,
    workspace: Weak<Workspace>,
    notify: Arc<Notify>,
    _listener: Arc<dyn Fn() + Send + Sync>,
    window: watch::Sender<Arc<Window>>,
    #[cfg(test)]
    reads: AtomicUsize,
}
impl EventHub {
    pub(crate) fn new(workspace: &Arc<Workspace>, engine: Arc<Engine>, scope: String) -> Arc<Self> {
        let notify = Arc::new(Notify::new());
        let listener: Arc<dyn Fn() + Send + Sync> = {
            let notify = notify.clone();
            Arc::new(move || notify.notify_one())
        };
        engine.store.subscribe_changes(Arc::downgrade(&listener));
        workspace
            .global
            .subscribe_changes(Arc::downgrade(&listener));
        workspace.subscribe_configuration_changes(Arc::downgrade(&listener));
        let (window, _) = watch::channel(Arc::new(Window::default()));
        let hub = Arc::new(Self {
            scope,
            engine,
            workspace: Arc::downgrade(workspace),
            notify,
            _listener: listener,
            window,
            #[cfg(test)]
            reads: AtomicUsize::new(0),
        });
        tokio::spawn(pump(Arc::downgrade(&hub)));
        hub
    }
}
async fn pump(weak: Weak<EventHub>) {
    loop {
        let Some(hub) = weak.upgrade() else { return };
        let Some(workspace) = hub.workspace.upgrade() else {
            return;
        };
        let previous = hub.window.borrow().clone();
        let engine = hub.engine.clone();
        let initialized = previous.initialized;
        let cursor = previous.cursor;
        #[cfg(test)]
        hub.reads.fetch_add(1, Ordering::Relaxed);
        let read = tokio::task::spawn_blocking(move || -> devdesk_domain::Result<_> {
            let _span = devdesk_adapters::performance::Span::new("events.shared_read");
            let (head, events) = if initialized {
                (cursor, engine.store.event_hints_after(cursor, 100)?)
            } else {
                (engine.store.event_cursor()?, vec![])
            };
            Ok((head, events, workspace.configuration_version(&engine)?))
        })
        .await;
        let mut caught_up = true;
        match read {
            Ok(Ok((head, events, version))) => {
                caught_up = events.len() < 100;
                if !initialized || !events.is_empty() || previous.configuration_version != version {
                    let mut next = previous.as_ref().clone();
                    if !initialized {
                        next.initialized = true;
                        next.cursor = head;
                        next.floor = head;
                    }
                    for hint in events {
                        if hint.omitted {
                            next.omit(hint.event.id);
                        } else {
                            next.append(hint.event, &hub.scope);
                        }
                    }
                    next.configuration_version = version;
                    hub.window.send_replace(Arc::new(next));
                }
            }
            Ok(Err(error)) => eprintln!("event tail read failed: {}", error.code),
            Err(_) => eprintln!("event tail reader task failed"),
        }
        let notify = hub.notify.clone();
        drop(previous);
        drop(hub);
        if !caught_up {
            // Bounded batches yield between reads so a hot stream cannot monopolize SQL.
            tokio::task::yield_now().await;
            continue;
        }
        tokio::select! {
            _ = notify.notified() => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {},
        }
    }
}

struct Subscriber {
    app: Arc<App>,
    hub: Arc<EventHub>,
    receiver: watch::Receiver<Arc<Window>>,
    cursor: Option<u64>,
    last_version: Option<Value>,
    ready: bool,
    configuration: bool,
}
fn payload(kind: &str, scope: &str, cursor: u64, version: Option<&Value>) -> String {
    let mut value = json!({"kind":kind,"project_id":scope,"cursor":cursor});
    if let Some(version) = version {
        value["configuration_version"] = version.clone();
    }
    value.to_string()
}
pub(super) fn events(
    app: Arc<App>,
    hub: Arc<EventHub>,
    cursor: Option<u64>,
    configuration: bool,
) -> Response {
    let receiver = hub.window.subscribe();
    let subscriber = Subscriber {
        app,
        hub,
        receiver,
        cursor,
        last_version: None,
        ready: false,
        configuration,
    };
    let stream = stream::unfold(subscriber, |mut state| async move {
        loop {
            if state.app.stopping.load(Ordering::Relaxed) {
                return None;
            }
            let window = state.receiver.borrow_and_update().clone();
            if window.initialized {
                let version = state.configuration.then_some(&window.configuration_version);
                let current = *state.cursor.get_or_insert(window.cursor);
                let mut event = None;
                if current < window.floor || current > window.cursor {
                    state.cursor = Some(window.cursor);
                    state.ready = true;
                    state.last_version = version.cloned();
                    event = Some(
                        Event::default()
                            .id(format!("{}:{}", state.hub.scope, window.cursor))
                            .data(payload(
                                "stream.reset",
                                &state.hub.scope,
                                window.cursor,
                                version,
                            )),
                    );
                } else if let Some(value) = window.events.iter().find(|event| event.id > current) {
                    state.cursor = Some(value.id);
                    // Ordinary tail events carry exact persisted content. Configuration
                    // clocks have their own shared notification and do not alter cursors.
                    event = Some(
                        Event::default()
                            .id(format!("{}:{}", state.hub.scope, value.id))
                            .data(value.json.clone()),
                    );
                } else if !state.ready {
                    state.ready = true;
                    state.last_version = version.cloned();
                    event = Some(Event::default().data(payload(
                        "stream.ready",
                        &state.hub.scope,
                        window.cursor,
                        version,
                    )));
                } else if version.is_some() && state.last_version.as_ref() != version {
                    state.last_version = version.cloned();
                    event = Some(Event::default().data(payload(
                        "configuration.changed",
                        &state.hub.scope,
                        window.cursor,
                        version,
                    )));
                }
                if let Some(event) = event {
                    return Some((Ok::<_, std::convert::Infallible>(event), state));
                }
            }
            drop(window);
            tokio::select! {
                changed = state.receiver.changed() => { if changed.is_err() { return None } },
                _ = state.app.shutdown.notified() => {},
                // No per-client SQL poll; this bound only ensures shutdown EOF.
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {},
            }
        }
    });
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use devdesk_core::concurrency::*;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_hundred_tail_subscribers_share_reads_and_release_their_hub() {
        let root = tempfile::tempdir().unwrap();
        let workspace =
            Workspace::open_with_adapters(root.path(), crate::test_runtime::adapters()).unwrap();
        let (_, engine) = workspace.event_engine(None).unwrap();
        let hub = workspace.event_hub("global", engine.clone()).unwrap();
        let weak = Arc::downgrade(&hub);
        let mut receivers = Vec::new();
        for _ in 0..100 {
            let next = workspace.event_hub("global", engine.clone()).unwrap();
            assert!(Arc::ptr_eq(&hub, &next));
            receivers.push(next.window.subscribe());
        }
        tokio::time::timeout(
            Duration::from_secs(2),
            receivers[0].wait_for(|window| window.initialized),
        )
        .await
        .unwrap()
        .unwrap();
        let initial = hub.window.borrow().cursor;
        let before = hub.reads.load(Ordering::Relaxed);
        let key = ResourceKey::new("setting", "test.event_hub");
        let receipt = workspace
            .global
            .commit_changes(&PreparedChangeSet {
                command_id: "hub-commit".into(),
                fingerprint: "test-actor-original-parameters".into(),
                read_set: ReadSet::new(vec![ResourceVersion {
                    resource: key.clone(),
                    revision: 0,
                }])
                .unwrap(),
                changes: vec![ResourceChange {
                    resource: key,
                    value: json!("committed"),
                }],
                result: json!({"ok":true}),
                time: 1,
            })
            .unwrap();
        assert!(receipt.cursor > initial);
        for receiver in &mut receivers {
            let window = tokio::time::timeout(
                Duration::from_secs(2),
                receiver.wait_for(|window| window.cursor >= receipt.cursor),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(
                window
                    .events
                    .iter()
                    .filter(|event| event.id == receipt.cursor)
                    .count(),
                1
            );
        }
        assert!(
            hub.reads.load(Ordering::Relaxed) - before <= 4,
            "100 subscribers caused independent SQL reads"
        );
        drop(receivers);
        drop(hub);
        tokio::time::timeout(Duration::from_secs(2), async {
            while weak.strong_count() > 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        // The registry contains only a Weak reference; an inactive scope releases
        // its cache/listener and the next subscription captures a new tail.
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn omitted_and_evicted_events_create_explicit_reset_boundaries() {
        let mut window = Window {
            initialized: true,
            ..Default::default()
        };
        for id in 1..=600 {
            window.append(
                devdesk_domain::Event {
                    id,
                    target: "target".into(),
                    kind: "changed".into(),
                    detail: "small".into(),
                    time: 1,
                },
                "project",
            );
        }
        assert_eq!(window.events.len(), SCOPE_EVENTS);
        assert_eq!(window.floor, 88);
        assert_eq!(window.cursor, 600);
        window.append(
            devdesk_domain::Event {
                id: 601,
                target: "target".into(),
                kind: "changed".into(),
                detail: "x".repeat(EVENT_BYTES + 1),
                time: 1,
            },
            "project",
        );
        assert_eq!(window.floor, 601);
        assert!(window.events.is_empty());
        window.append(
            devdesk_domain::Event {
                id: 602,
                target: "target".into(),
                kind: "changed".into(),
                detail: "small".into(),
                time: 1,
            },
            "project",
        );
        assert_eq!(window.floor, 601);
        assert_eq!(window.events[0].id, 602);
        assert!(window.bytes <= SCOPE_BYTES);
    }
}
