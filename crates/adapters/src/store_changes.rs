use super::*;
use std::sync::{Arc, Weak};

#[derive(Default)]
pub(super) struct StoreChanges(Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>);
impl StoreChanges {
    pub fn subscribe(&self, listener: Weak<dyn Fn() + Send + Sync>) {
        let mut listeners = self.0.lock().unwrap();
        listeners.retain(|value| value.strong_count() > 0);
        listeners.push(listener);
    }
    pub fn notify(&self) {
        let callbacks: Vec<Arc<dyn Fn() + Send + Sync>> = {
            let mut listeners = self.0.lock().unwrap();
            let callbacks = listeners.iter().filter_map(Weak::upgrade).collect();
            listeners.retain(|listener| listener.strong_count() > 0);
            callbacks
        };
        for callback in callbacks {
            callback();
        }
    }
}
pub(super) fn events_after(conn: &Connection, cursor: u64, limit: usize) -> Result<Vec<Event>> {
    let _span = performance::Span::new("events.indexed_read");
    let cursor = i64::try_from(cursor).unwrap_or(i64::MAX);
    let mut query = conn
        .prepare_cached(
            "SELECT id,target,kind,detail,time FROM events WHERE id>?1 ORDER BY id LIMIT ?2",
        )
        .map_err(storage)?;
    query
        .query_map(rusqlite::params![cursor, limit.clamp(1, 100)], |r| {
            Ok(Event {
                id: r.get(0)?,
                target: r.get(1)?,
                kind: r.get(2)?,
                detail: r.get(3)?,
                time: r.get(4)?,
            })
        })
        .map_err(storage)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(storage)
}
pub(super) fn cursor(conn: &Connection) -> Result<u64> {
    conn.query_row("SELECT coalesce(max(id),0) FROM events", [], |row| {
        row.get(0)
    })
    .map_err(storage)
}
pub(super) fn tail(
    conn: &Connection,
    cursor: u64,
    limit: usize,
) -> Result<Vec<devdesk_core::read_models::TailEvent>> {
    let _span = performance::Span::new("events.bounded_tail_read");
    let mut statement = conn.prepare_cached("SELECT id,CASE WHEN length(target)<=1024 THEN target ELSE '' END,CASE WHEN length(kind)<=128 THEN kind ELSE '' END,CASE WHEN length(detail)<=8192 THEN detail ELSE '' END,time,length(target)>1024 OR length(kind)>128 OR length(detail)>8192 FROM events WHERE id>?1 ORDER BY id LIMIT ?2").map_err(storage)?;
    statement
        .query_map(
            rusqlite::params![
                i64::try_from(cursor).unwrap_or(i64::MAX),
                limit.clamp(1, 100)
            ],
            |row| {
                Ok(devdesk_core::read_models::TailEvent {
                    event: Event {
                        id: row.get(0)?,
                        target: row.get(1)?,
                        kind: row.get(2)?,
                        detail: row.get(3)?,
                        time: row.get(4)?,
                    },
                    omitted: row.get(5)?,
                })
            },
        )
        .map_err(storage)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(storage)
}
pub(super) fn pending_tools(conn: &Connection) -> Result<bool> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM governance_records WHERE type='tool_call' AND (json_extract(data,'$.status') IN ('awaiting_approval','prepared','dispatched','needs_reconciliation') OR (json_extract(data,'$.status') IN ('succeeded','failed','cancelled') AND coalesce(json_extract(data,'$.reply_sent'),0)=0 AND json_extract(data,'$.invocation.session_id') IS NOT NULL)))",[],|r|r.get(0)).map_err(storage)
}
