use crate::*;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HistoryRecord<'a> {
    Event {
        time: u64,
        event: &'a Event,
    },
    ToolCall {
        time: u64,
        call: ConfigurationRecord,
    },
}

impl HistoryRecord<'_> {
    fn time(&self) -> u64 {
        match self {
            Self::Event { time, .. } | Self::ToolCall { time, .. } => *time,
        }
    }
}

pub(crate) fn query(s: &EngineeringState, p: &Value, offset: usize, limit: usize) -> Value {
    let target = p["target"].as_str();
    let mut events: Vec<_> = s
        .events
        .iter()
        .filter(|e| target.is_none_or(|id| e.target == id))
        .collect();
    events.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| b.id.cmp(&a.id)));
    if p["include_tool_calls"] != true {
        return json!({"events":events.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":events.len()});
    }

    let mut records: Vec<_> = events
        .into_iter()
        .map(|event| HistoryRecord::Event {
            time: event.time,
            event,
        })
        .chain(
            crate::integrations::integration_records(s, "query.tool_calls", p)
                .into_iter()
                .filter(|call| target.is_none_or(|id| call.id == id))
                .map(|call| HistoryRecord::ToolCall {
                    time: call.created_at,
                    call,
                }),
        )
        .collect();
    records.sort_by(|a, b| {
        b.time().cmp(&a.time()).then_with(|| match (a, b) {
            (HistoryRecord::Event { event: a, .. }, HistoryRecord::Event { event: b, .. }) => {
                b.id.cmp(&a.id)
            }
            (HistoryRecord::ToolCall { call: a, .. }, HistoryRecord::ToolCall { call: b, .. }) => {
                b.id.cmp(&a.id)
            }
            (HistoryRecord::Event { .. }, HistoryRecord::ToolCall { .. }) => {
                std::cmp::Ordering::Less
            }
            _ => std::cmp::Ordering::Greater,
        })
    });
    json!({"records":records.iter().skip(offset).take(limit).collect::<Vec<_>>(),"total":records.len()})
}
