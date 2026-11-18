//! SQL selects the page before any source restoration or call-body decoding.
use crate::{connection_pool::ConnectionPool, *};
use rusqlite::{OptionalExtension, params_from_iter};

fn page(p: &Value) -> Result<(u64, u64)> {
    let offset = p["offset"].as_u64().unwrap_or(0);
    require(offset <= 1_000_000, "分页偏移过大")?;
    Ok((offset, p["limit"].as_u64().unwrap_or(50).clamp(1, 100)))
}

fn call_filter(p: &Value, project: Option<&str>, engineering: bool) -> (String, Vec<SqlValue>) {
    let mut predicate = "r.type IN ('tool_call','platform_call','agent_context_call')".to_owned();
    let mut values = vec![];
    let mut equals = |expression: &str, value: &str| {
        predicate.push_str(&format!(" AND {expression}=?{}", values.len() + 1));
        values.push(SqlValue::Text(value.into()));
    };
    if let Some(project) = project {
        equals("coalesce(r.owner_project,?1)", project);
    }
    if let Some(id) = p["id"].as_str().or_else(|| p["target"].as_str()) {
        equals("r.id", id);
    }
    for key in [
        "session_id",
        "turn_id",
        "feature_id",
        "task_id",
        "agent_instance_id",
    ] {
        if let Some(value) = p[key].as_str() {
            let expression = if !engineering {
                format!("json_extract(r.data,'$.invocation.{key}')")
            } else {
                match key {
                "session_id" | "turn_id" => format!("coalesce(json_extract(r.data,'$.invocation.{key}'),json_extract(r.data,'$.request.caller.{key}'))"),
                "task_id" => "coalesce(json_extract(r.data,'$.invocation.task_id'),(SELECT id FROM tasks WHERE id=json_extract(r.data,'$.request.owner_id')))".into(),
                "feature_id" => "coalesce(json_extract(r.data,'$.invocation.feature_id'),(SELECT feature_id FROM tasks WHERE id=json_extract(r.data,'$.request.owner_id')),(SELECT id FROM features WHERE id=json_extract(r.data,'$.request.owner_id')))".into(),
                _ => "coalesce(json_extract(r.data,'$.invocation.agent_instance_id'),(SELECT json_extract(runtime_binding,'$.agent_configuration.agent_instance_id') FROM sessions WHERE id=json_extract(r.data,'$.request.caller.session_id')))".into(),
            }
            };
            equals(&expression, value);
        }
    }
    if let Some(value) = p["tool_ref"].as_str() {
        equals(
            "coalesce(json_extract(r.data,'$.invocation.tool.revision_id'),json_extract(r.data,'$.invocation.tool.tool_ref'),json_extract(r.data,'$.request.caller.tool_ref'),json_extract(r.data,'$.request.definition.ref'))",
            value,
        );
    }
    if let Some(value) = p["integration_ref"].as_str() {
        equals(
            "coalesce(json_extract(r.data,'$.invocation.tool.definition.integration_ref'),json_extract(r.data,'$.request.definition.integration_ref'),(SELECT coalesce(json_extract(refs,'$.integration_ref'),json_extract(data,'$.integration_ref')) FROM source_definitions WHERE reference=json_extract(r.data,'$.request.definition.ref')))",
            value,
        );
    }
    if p["include_history"] != true {
        predicate.push_str(" AND (r.type='tool_call' OR NOT EXISTS(SELECT 1 FROM governance_records n WHERE n.type=r.type AND n.definition_id=r.definition_id AND (n.version>r.version OR (n.version=r.version AND n.rowid>r.rowid))))");
    }
    (predicate, values)
}

fn call(
    conn: &Connection,
    catalog: &ConfigurationCatalogSnapshot,
    id: &str,
    summary: bool,
    engineering: bool,
) -> Result<Value> {
    if summary {
        return conn.query_row("SELECT json_object('id',id,'type',type,'name',substr(name,1,128),'revision',revision,'created_at',created_at,'metadata_only',json('true'),'data',json_object('status',json_extract(data,'$.status'),'remote_effect',json_extract(data,'$.remote_effect'),'invocation',json_object('session_id',coalesce(json_extract(data,'$.invocation.session_id'),json_extract(data,'$.request.caller.session_id')),'agent_instance_id',json_extract(data,'$.invocation.agent_instance_id')))) FROM governance_records WHERE id=?1",[id],|r|r.get::<_,String>(0)).map_err(storage).and_then(|s|serde_json::from_str(&s).map_err(storage));
    }
    let raw = resource_commits::read_on(conn, &[concurrency::ResourceKey::new("governance", id)])?
        .remove(0);
    let mut record = ConfigurationRecord::from_storage(
        raw.value.ok_or_else(|| err("not_found", "调用不存在"))?,
    )?;
    record.data = source_views::restore(&catalog.state, &record.data)?;
    if record.r#type == "platform_call" {
        let request = &record.data["request"];
        let caller = &request["caller"];
        let owner = request["owner_id"].as_str().unwrap_or("");
        let task = if engineering {
            conn.query_row("SELECT feature_id FROM tasks WHERE id=?1", [owner], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(storage)?
        } else {
            None
        };
        let feature = match &task {
            Some(feature) => Some(feature.clone()),
            None if engineering => conn
                .query_row("SELECT id FROM features WHERE id=?1", [owner], |r| r.get(0))
                .optional()
                .map_err(storage)?,
            None => None,
        };
        let instance = if engineering {
            conn.query_row("SELECT json_extract(runtime_binding,'$.agent_configuration.agent_instance_id') FROM sessions WHERE id=?1",[caller["session_id"].as_str().unwrap_or("")],|r|r.get::<_,Option<String>>(0)).optional().map_err(storage)?.flatten()
        } else {
            None
        };
        let reference = caller["tool_ref"].as_str().map(str::to_owned).or_else(|| {
            catalog
                .state
                .governance
                .iter()
                .find(|r| r.r#type == "tool" && r.data == request["definition"])
                .map(|r| r.id.clone())
        });
        let invocation = json!({"id":record.id,"project_id":request["project_id"],"feature_id":feature,"task_id":task.as_ref().map(|_|owner),"agent_instance_id":instance,"session_id":caller["session_id"],"turn_id":caller["turn_id"],"operation_id":caller["operation_id"].as_str().unwrap_or(&record.id),"tool":{"revision_id":reference,"definition":request["definition"]},"arguments":request["arguments"],"input_hash":configuration_hash(&request["arguments"])});
        let reply = load_table_filtered_params::<ConfigurationRecord>(conn,"governance_records","id IN (SELECT id FROM governance_records WHERE type='platform_tool_reply' AND definition_id=?1 ORDER BY rowid DESC LIMIT 1)",&[&id])?.pop();
        let receipt=reply.map(|r|source_views::restore(&catalog.state,&r.data).map(|v|v["receipt"].clone())).transpose()?.unwrap_or_else(||json!({"success":record.data["status"]=="succeeded","structured_content":record.data["receipt"],"error":record.data["error"]}));
        record.data["platform_receipt"] = record.data["receipt"].clone();
        record.data["receipt"] = receipt;
        record.data["invocation"] = invocation;
    }
    if record.r#type == "tool_call" {
        let pointers: Vec<String> = serde_json::from_value(
            record.data["invocation"]["tool"]["definition"]["sensitive_parameters"].clone(),
        )
        .unwrap_or_default();
        for pointer in pointers {
            if let Some(value) = record.data["invocation"]["arguments"].pointer_mut(&pointer) {
                *value = json!("[redacted]");
            }
        }
    }
    Ok(json!(record))
}

pub(super) fn read(
    pool: &ConnectionPool,
    catalog: &ConfigurationCatalogSnapshot,
    project: Option<&str>,
    method: &str,
    p: &Value,
) -> Result<Value> {
    let _span = performance::Span::new("read_model.operational_page");
    if method == "query.runtimes" {
        return Ok(
            json!({"harnesses":catalog.state.harnesses,"sandboxes":catalog.state.sandboxes}),
        );
    }
    let (offset, limit) = page(p)?;
    let conn = pool.read()?;
    let tx = conn.unchecked_transaction().map_err(storage)?;
    let result = if method == "query.repository" {
        let project = project.ok_or_else(|| err("project_required", "仓库读取需要项目"))?;
        let current = execution_read::project(&tx, project)?;
        let mut result = json!({"project":current});
        for (table, key, total) in [
            ("repository_operations", "operations", "total_operations"),
            ("repository_worktrees", "worktrees", "total_worktrees"),
        ] {
            let sql = format!(
                "id IN (SELECT id FROM {table} WHERE project_id=?1 ORDER BY rowid DESC LIMIT ?2 OFFSET ?3)"
            );
            let mut rows = load_table_filtered_params::<Value>(
                &tx,
                table,
                &sql,
                &[&project, &limit, &offset],
            )?;
            rows.reverse();
            result[key] = json!(rows);
            result[total] = json!(
                tx.query_row(
                    &format!("SELECT count(*) FROM {table} WHERE project_id=?1"),
                    [project],
                    |r| r.get::<_, u64>(0)
                )
                .map_err(storage)?
            );
        }
        result
    } else {
        let combined = method == "query.history" && p["include_tool_calls"] == true;
        let events = method == "query.history";
        let calls = !events || combined;
        let engineering: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sessions')",
                [],
                |r| r.get(0),
            )
            .map_err(storage)?;
        let (filter, mut values) = if calls {
            call_filter(p, project, engineering)
        } else {
            (String::new(), vec![])
        };
        let mut pieces = vec![];
        if calls {
            pieces.push(format!("SELECT 'tool_call' AS kind,r.id AS identity,r.created_at AS time,1 AS rank,0 AS event_order,r.id AS call_order,r.rowid AS position FROM governance_records r WHERE {filter}"));
        }
        if events {
            let predicate = if let Some(target) = p["target"].as_str() {
                values.push(SqlValue::Text(target.into()));
                format!("WHERE target=?{}", values.len())
            } else {
                String::new()
            };
            let predicate = if let Some(identity) = p.get("event_id") {
                let identity = identity
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                    .ok_or_else(|| err("invalid_params", "无效事件标识"))?;
                values.push(SqlValue::Integer(identity as i64));
                format!(
                    "{predicate} {} id=?{}",
                    if predicate.is_empty() { "WHERE" } else { "AND" },
                    values.len()
                )
            } else {
                predicate
            };
            pieces.push(format!("SELECT 'event' AS kind,CAST(id AS TEXT) AS identity,time,0 AS rank,id AS event_order,'' AS call_order,id AS position FROM events {predicate}"));
        }
        let selection = pieces.join(" UNION ALL ");
        let total: u64 = tx
            .query_row(
                &format!("SELECT count(*) FROM ({selection})"),
                params_from_iter(values.iter()),
                |r| r.get(0),
            )
            .map_err(storage)?;
        values.push(SqlValue::Integer(limit as i64));
        values.push(SqlValue::Integer(offset as i64));
        let order = if events {
            "time DESC,rank,event_order DESC,call_order DESC"
        } else {
            "position DESC"
        };
        let mut q=tx.prepare_cached(&format!("SELECT kind,identity,time FROM ({selection}) ORDER BY {order} LIMIT ?{} OFFSET ?{}",values.len()-1,values.len())).map_err(storage)?;
        let selected = q
            .query_map(params_from_iter(values.iter()), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, u64>(2)?,
                ))
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(storage)?;
        let mut rows = vec![];
        for (kind, id, time) in selected {
            if kind == "event" {
                let event = tx
                    .query_row(
                        if p["view"] == "summary" {"SELECT id,target,kind,substr(detail,1,512),time FROM events WHERE id=?1"} else {"SELECT id,target,kind,detail,time FROM events WHERE id=?1"},
                        [id],
                        |r| {
                            Ok(Event {
                                id: r.get(0)?,
                                target: r.get(1)?,
                                kind: r.get(2)?,
                                detail: r.get(3)?,
                                time: r.get(4)?,
                            })
                        },
                    )
                    .map_err(storage)?;
                let mut event = json!(event);
                if p["view"] == "summary" {
                    event["metadata_only"] = json!(true);
                }
                rows.push(if combined {
                    json!({"type":"event","time":time,"event":event})
                } else {
                    event
                });
            } else {
                let value = call(&tx, catalog, &id, p["view"] == "summary", engineering)?;
                rows.push(if combined {
                    json!({"type":"tool_call","time":time,"call":value})
                } else {
                    value
                });
            }
        }
        if events && !combined {
            json!({"events":rows,"total":total})
        } else {
            json!({"records":rows,"total":total})
        }
    };
    tx.commit().map_err(storage)?;
    Ok(result)
}
