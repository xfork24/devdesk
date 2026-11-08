//! Project selection pins every request to one Engine and therefore one database.
use devdesk_adapters::*;
use devdesk_core::*;
use devdesk_domain::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

type Active = Option<(String, Arc<Engine>)>;
pub struct Workspace {
    pub global: Arc<GlobalStore>,
    configuration: Arc<Engine>,
    adapters: ports::ExecutionAdapters,
    active: RwLock<Active>,
    opened: Mutex<BTreeMap<String, Arc<Engine>>>,
    switching: Mutex<()>,
}
fn engine(store: Arc<dyn EngineeringStore>, adapters: &ports::ExecutionAdapters) -> Arc<Engine> {
    let artifacts = Arc::new(FileArtifacts::new(PathBuf::from(store.root())));
    Arc::new(Engine::new(store, artifacts, adapters.clone()))
}
impl Workspace {
    pub fn open(root: &Path) -> Result<Arc<Self>> {
        Self::open_with_adapters(root, simulation::adapters())
    }
    pub fn open_with_adapters(
        root: &Path,
        adapters: ports::ExecutionAdapters,
    ) -> Result<Arc<Self>> {
        let global = Arc::new(GlobalStore::open(root)?);
        let configuration = engine(global.clone(), &adapters);
        configuration.bootstrap()?;
        let workspace = Arc::new(Self {
            global,
            configuration,
            adapters,
            active: RwLock::new(None),
            opened: Mutex::new(BTreeMap::new()),
            switching: Mutex::new(()),
        });
        if let Some(id) = workspace.global.active_project()? {
            let e = workspace.load_project(&id, false)?;
            *workspace.active.write().unwrap() = Some((id, e));
        }
        Ok(workspace)
    }
    pub fn active_id(&self) -> Option<String> {
        self.active
            .read()
            .unwrap()
            .as_ref()
            .map(|(id, _)| id.clone())
    }
    pub fn active_engine(&self, expected: Option<&str>) -> Result<Arc<Engine>> {
        let active = self.active.read().unwrap();
        if let Some(expected) = expected {
            if active.as_ref().is_none_or(|(id, _)| id != expected) {
                return Err(err("project_changed", "活动项目已改变，请刷新后重试"));
            }
        }
        active
            .as_ref()
            .map(|(_, e)| e.clone())
            .ok_or_else(|| err("project_required", "请先创建或打开项目"))
    }
    fn sync_configuration(&self, e: &Engine) -> Result<()> {
        let shared = self.global.snapshot()?;
        e.store.transact(None, &mut |s| {
            // Revisions are immutable. Copies keep historical foreign keys self-contained.
            for d in &shared.definitions {
                if let Some(old) = s.definitions.iter().find(|old| old.id == d.id) {
                    require(
                        serde_json::to_value(old).unwrap() == serde_json::to_value(d).unwrap(),
                        "共享定义修订发生冲突",
                    )?;
                } else {
                    s.definitions.push(d.clone());
                }
            }
            for h in &shared.harnesses {
                if !s.harnesses.iter().any(|old| old.id == h.id) {
                    s.harnesses.push(h.clone());
                }
            }
            for b in &shared.sandboxes {
                if !s.sandboxes.iter().any(|old| old.id == b.id) {
                    s.sandboxes.push(b.clone());
                }
            }
            Ok(json!({"ok":true}))
        })?;
        Ok(())
    }
    fn load_project(&self, id: &str, creating: bool) -> Result<Arc<Engine>> {
        let root = PathBuf::from(self.global.root());
        if !creating {
            require(self.global.project(id)?.is_some(), "项目不存在")?;
            require(
                SqliteStore::project_path(&root, id)?
                    .join("engineering.sqlite3")
                    .is_file(),
                "项目数据库缺失，不能自动重建",
            )?;
        }
        if let Some(e) = self.opened.lock().unwrap().get(id).cloned() {
            return Ok(e);
        }
        let e = engine(
            Arc::new(SqliteStore::open_project(&root, id)?),
            &self.adapters,
        );
        if !creating {
            require(
                e.store.snapshot()?.projects.len() == 1,
                "项目数据库未包含有效 Project",
            )?;
        }
        self.sync_configuration(&e)?;
        e.bootstrap()?;
        // Repair a stale directory summary after a crash between the two independent commits.
        if let Some(project) = e.store.snapshot()?.projects.first() {
            self.global.publish_project(project)?;
        }
        self.opened.lock().unwrap().insert(id.into(), e.clone());
        Ok(e)
    }
    fn select(&self, id: &str, e: Arc<Engine>) -> Result<()> {
        self.global.activate(id)?;
        *self.active.write().unwrap() = Some((id.into(), e));
        Ok(())
    }
    pub fn dispatch(&self, method: &str, params: Value) -> Result<Value> {
        if method == "settings.update" {
            let n = params["global_concurrency"]
                .as_u64()
                .ok_or_else(|| err("invalid_params", "需要并发额度"))?;
            require((1..=32).contains(&n), "全局并发额度需为 1—32")?;
            self.global.set_concurrency(n as usize)?;
            return Ok(json!({"global_concurrency":n}));
        }
        if method == "project.create" {
            let command: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            let _switch = self.switching.lock().unwrap();
            let id = self.global.reserve_project(&command)?;
            let e = self.load_project(&id, true)?;
            self.sync_configuration(&e)?;
            let result = e.execute(method, command)?;
            self.global.publish_project(
                e.store
                    .snapshot()?
                    .projects
                    .first()
                    .ok_or_else(|| err("storage", "项目未创建"))?,
            )?;
            self.select(&id, e)?;
            return Ok(result);
        }
        if method == "project.open" {
            let c: Command =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            require(!c.command_id.is_empty(), "command_id 不能为空")?;
            let id = c
                .target
                .as_deref()
                .ok_or_else(|| err("invalid_params", "需要 Project target"))?;
            let _switch = self.switching.lock().unwrap();
            self.global.validate_open_command(&c)?;
            let e = self.load_project(id, false)?;
            self.sync_configuration(&e)?;
            self.global.activate_command(&c)?;
            *self.active.write().unwrap() = Some((id.into(), e));
            return Ok(json!({"id":id}));
        }
        if method == "definition.revise" {
            let c =
                serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
            return self.configuration.execute(method, c);
        }
        let expected = params["project_id"]
            .as_str()
            .or_else(|| params["payload"]["project_id"].as_str());
        let e = match self.active_engine(expected) {
            Ok(e) => e,
            Err(error)
                if error.code == "project_required"
                    && [
                        "query.workspace",
                        "query.history",
                        "query.events",
                        "query.runtimes",
                    ]
                    .contains(&method) =>
            {
                self.configuration.clone()
            }
            Err(error) => return Err(error),
        };
        if e.store.project_id().is_some() {
            self.sync_configuration(&e)?;
        }
        let global_limit = self.global.concurrency()?;
        if method.starts_with("query.") {
            let mut result = e.query(method, &params, self.project_limit(&e, global_limit)?)?;
            if method == "query.workspace" {
                let offset = params["project_offset"].as_u64().unwrap_or(0) as usize;
                let limit = params["limit"].as_u64().unwrap_or(50).clamp(1, 100) as usize;
                let (mut projects, total) = self.global.projects(offset, limit)?;
                let current = e.store.snapshot()?.projects.first().cloned();
                if let Some(project) = &current {
                    self.global.publish_project(project)?;
                    if let Some(row) = projects.iter_mut().find(|p| p.id == project.id) {
                        *row = project.clone();
                    }
                }
                result["projects"] = json!(projects);
                result["total_projects"] = json!(total);
                result["active_project_id"] = json!(e.store.project_id());
                result["active_project"] = json!(current);
                result["project_data_root"] = if e.store.project_id().is_some() {
                    json!(e.store.root())
                } else {
                    Value::Null
                };
                result["project_database"] = if e.store.project_id().is_some() {
                    json!(PathBuf::from(e.store.root()).join("engineering.sqlite3"))
                } else {
                    Value::Null
                };
                result["global_database"] =
                    json!(PathBuf::from(self.global.root()).join("global.sqlite3"));
                result["data_root"] = json!(self.global.root());
                result["global_concurrency"] = json!(global_limit);
            }
            return Ok(result);
        }
        let c = serde_json::from_value(params).map_err(|e| err("invalid_params", e.to_string()))?;
        let result = e.execute(method, c)?;
        if let Some(project) = e.store.snapshot()?.projects.first() {
            self.global.publish_project(project)?;
        }
        Ok(result)
    }
    pub fn event_engine(&self, expected: Option<&str>) -> Result<(String, Arc<Engine>)> {
        match self.active_engine(expected) {
            Ok(e) => Ok((e.store.project_id().unwrap(), e)),
            Err(error) if error.code == "project_required" => {
                Ok(("global".into(), self.configuration.clone()))
            }
            Err(error) => Err(error),
        }
    }
    pub fn tick(&self) -> Result<()> {
        let engines: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let limit = self.global.concurrency()?;
        let mut failure = None;
        let mut running: Vec<usize> = engines
            .iter()
            .map(|e| {
                Ok(e.store
                    .snapshot()?
                    .runs
                    .iter()
                    .filter(|r| r.status == Status::Running)
                    .count())
            })
            .collect::<Result<_>>()?;
        let mut total: usize = running.iter().sum();
        // The scheduler is single-threaded across projects. Other running projects consume quota.
        for (index, e) in engines.iter().enumerate() {
            if let Err(error) = e.tick(limit.saturating_sub(total - running[index])) {
                failure.get_or_insert(error);
            }
            let next = e
                .store
                .snapshot()?
                .runs
                .iter()
                .filter(|r| r.status == Status::Running)
                .count();
            total = total - running[index] + next;
            running[index] = next;
        }
        failure.map_or(Ok(()), Err)
    }
    fn project_limit(&self, engine: &Arc<Engine>, limit: usize) -> Result<usize> {
        let engines: Vec<_> = self.opened.lock().unwrap().values().cloned().collect();
        let mut others = 0;
        for other in engines {
            if !Arc::ptr_eq(engine, &other) {
                others += other
                    .store
                    .snapshot()?
                    .runs
                    .iter()
                    .filter(|run| run.status == Status::Running)
                    .count();
            }
        }
        Ok(limit.saturating_sub(others))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cmd(
        w: &Workspace,
        method: &str,
        scope: Option<&str>,
        target: Option<&str>,
        key: &str,
        payload: Value,
    ) -> Value {
        w.dispatch(
            method,
            json!({"project_id":scope,"command_id":key,"target":target,"payload":payload}),
        )
        .unwrap()
    }
    fn create(w: &Workspace, title: &str) -> String {
        cmd(
            w,
            "project.create",
            None,
            None,
            &devdesk_domain::id(),
            json!({"title":title}),
        )["id"]
            .as_str()
            .unwrap()
            .into()
    }
    fn select(w: &Workspace, id: &str) {
        cmd(
            w,
            "project.open",
            None,
            Some(id),
            &devdesk_domain::id(),
            json!({}),
        );
    }
    #[test]
    fn switching_projects_isolates_state_artifacts_and_idempotency() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "项目 A");
        let engine_a = w.active_engine(Some(&a)).unwrap();
        let fa = cmd(
            &w,
            "feature.create",
            Some(&a),
            None,
            "same-feature-command",
            json!({"project_id":a,"title":"A Feature"}),
        );
        let fid = fa["id"].as_str().unwrap();
        w.dispatch("document.revise",json!({"project_id":a,"command_id":"doc-a","target":fid,"expected_revision":1,"payload":{"kind":"requirement","title":"A 需求","body":"项目 A 专属内容"}})).unwrap();
        let artifact = engine_a.store.snapshot().unwrap().artifacts[0].clone();
        let b = create(&w, "项目 B");
        let engine_b = w.active_engine(Some(&b)).unwrap();
        let fb = cmd(
            &w,
            "feature.create",
            Some(&b),
            None,
            "same-feature-command",
            json!({"project_id":b,"title":"B Feature"}),
        );
        assert_ne!(fa, fb);
        assert_ne!(engine_a.store.root(), engine_b.store.root());
        let view = w
            .dispatch("query.workspace", json!({"project_id":b}))
            .unwrap();
        assert_eq!(view["active_project_id"], b);
        assert_eq!(view["features"].as_array().unwrap().len(), 1);
        assert_eq!(view["features"][0]["title"], "B Feature");
        assert_eq!(view["total_projects"], 2);
        let page = w
            .dispatch("query.workspace", json!({"project_id":b,"limit":1}))
            .unwrap();
        assert_eq!(page["projects"].as_array().unwrap().len(), 1);
        assert_eq!(page["projects"][0]["id"], a);
        assert_eq!(page["active_project"]["id"], b);
        assert!(
            engine_b
                .query("query.artifact", &json!({"id":artifact.id}), 4)
                .is_err()
        );
        assert!(
            w.dispatch("query.workspace", json!({"project_id":a}))
                .is_err()
        );
        assert!(w.dispatch("feature.create",json!({"project_id":a,"command_id":"stale","payload":{"project_id":a,"title":"不能串写"}})).is_err());
        // A previously pinned request still writes to A after the active pointer changes to B.
        engine_a
            .execute(
                "feature.create",
                Command {
                    command_id: "in-flight-a".into(),
                    target: None,
                    expected_revision: None,
                    payload: json!({"project_id":a,"title":"已绑定的 A 请求"}),
                },
            )
            .unwrap();
        assert_eq!(engine_b.store.snapshot().unwrap().features.len(), 1);
        select(&w, &a);
        assert_eq!(
            cmd(
                &w,
                "feature.create",
                Some(&a),
                None,
                "same-feature-command",
                json!({"project_id":a,"title":"A Feature"})
            ),
            fa
        );
        assert_eq!(
            engine_a.artifacts.read(&artifact).unwrap(),
            "项目 A 专属内容".as_bytes()
        );
        assert_eq!(engine_a.store.snapshot().unwrap().projects.len(), 1);
        assert_eq!(
            w.dispatch("query.workspace", json!({})).unwrap()["features"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
    #[test]
    fn global_settings_definitions_and_active_project_survive_restart() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let old = w.global.snapshot().unwrap().definitions[0].clone();
        let b = create(&w, "B");
        let new = cmd(
            &w,
            "definition.revise",
            Some(&b),
            Some(&old.id),
            "global-definition",
            json!({"body":"共享新修订"}),
        );
        w.dispatch("settings.update", json!({"global_concurrency":3}))
            .unwrap();
        select(&w, &a);
        let a_state = w.active_engine(None).unwrap().store.snapshot().unwrap();
        assert!(
            a_state
                .definitions
                .iter()
                .any(|d| d.id == new["id"].as_str().unwrap())
        );
        assert_eq!(
            a_state
                .definitions
                .iter()
                .find(|d| d.id == old.id)
                .unwrap()
                .body,
            old.body
        );
        drop(w);
        let w = Workspace::open(dir.path()).unwrap();
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
        assert_eq!(w.global.concurrency().unwrap(), 3);
        assert_eq!(
            w.dispatch("query.workspace", json!({})).unwrap()["total_projects"],
            2
        );
        select(&w, &b);
        assert!(
            w.active_engine(None)
                .unwrap()
                .store
                .snapshot()
                .unwrap()
                .definitions
                .iter()
                .any(|d| d.id == new["id"].as_str().unwrap())
        );
        assert!(!dir.path().join("runtime.json").exists());
        assert!(!dir.path().join("engineering.sqlite3").exists());
    }
    #[test]
    fn failed_open_preserves_active_project_and_creation_resumes_reserved_identity() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        let a = create(&w, "A");
        let c = Command {
            command_id: "reserved".into(),
            target: None,
            expected_revision: None,
            payload: json!({"title":"恢复创建"}),
        };
        let reserved = w.global.reserve_project(&c).unwrap();
        drop(w);
        let w = Workspace::open(dir.path()).unwrap();
        assert_eq!(
            cmd(
                &w,
                "project.create",
                None,
                None,
                &c.command_id,
                c.payload.clone()
            )["id"],
            reserved
        );
        select(&w, &a);
        assert!(
            w.dispatch(
                "project.open",
                json!({"command_id":"missing","target":devdesk_domain::id(),"payload":{}})
            )
            .is_err()
        );
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
        std::fs::rename(
            dir.path()
                .join("projects")
                .join(&reserved)
                .join("engineering.sqlite3"),
            dir.path().join("missing.sqlite3"),
        )
        .unwrap();
        assert!(
            w.dispatch(
                "project.open",
                json!({"command_id":"deleted","target":reserved,"payload":{}})
            )
            .is_err()
        );
        assert_eq!(w.active_id().as_deref(), Some(a.as_str()));
    }
    #[test]
    fn switching_keeps_inactive_execution_and_shared_quota() {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::open(dir.path()).unwrap();
        w.global.set_concurrency(1).unwrap();
        let a = create(&w, "A");
        let fa = cmd(
            &w,
            "feature.create",
            Some(&a),
            None,
            "fa",
            json!({"project_id":a,"title":"A"}),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        w.dispatch("attempt.start",json!({"project_id":a,"command_id":"run-a","target":fa,"expected_revision":1,"payload":{"phase":"planning","scenario":"hold"}})).unwrap();
        w.tick().unwrap();
        let ea = w.active_engine(None).unwrap();
        let b = create(&w, "B");
        let fb = cmd(
            &w,
            "feature.create",
            Some(&b),
            None,
            "fb",
            json!({"project_id":b,"title":"B"}),
        )["id"]
            .as_str()
            .unwrap()
            .to_owned();
        w.dispatch("attempt.start",json!({"project_id":b,"command_id":"run-b","target":fb,"expected_revision":1,"payload":{"phase":"planning","scenario":"hold"}})).unwrap();
        let eb = w.active_engine(None).unwrap();
        w.tick().unwrap();
        let count = |e: &Engine| {
            e.store
                .snapshot()
                .unwrap()
                .runs
                .iter()
                .filter(|r| r.status == Status::Running)
                .count()
        };
        assert_eq!(count(&ea), 1);
        assert_eq!(count(&eb), 0);
        select(&w, &a);
        assert!(
            ea.store
                .snapshot()
                .unwrap()
                .runs
                .iter()
                .any(|r| r.status == Status::Running)
        );
        let (_, pinned) = w.event_engine(Some(&a)).unwrap();
        select(&w, &b);
        assert!(Arc::ptr_eq(&ea, &pinned));
    }
}

#[cfg(test)]
mod adapter_injection_tests {
    use super::*;
    struct RecordingHarness(Arc<Mutex<Vec<String>>>);
    impl ports::Harness for RecordingHarness {
        fn identity(&self) -> String {
            "fixture:workspace-harness:1".into()
        }
        fn configure(&self, node: &str, scenario: &str) -> String {
            SimHarness.configure(node, scenario)
        }
        fn poll(&self, run: &Run, input: &Value, now: u64) -> Result<Option<Value>> {
            self.0.lock().unwrap().push(run.id.clone());
            SimHarness.poll(run, input, now + 10_000)
        }
    }
    #[test]
    fn injected_providers_are_used_after_opening_another_project() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(vec![]));
        let providers = ports::ExecutionAdapters {
            harness: Arc::new(RecordingHarness(calls.clone())),
            ..simulation::adapters()
        };
        let w = Workspace::open_with_adapters(dir.path(), providers).unwrap();
        let mut projects = vec![];
        for title in ["A", "B"] {
            let pid = w
                .dispatch(
                    "project.create",
                    json!({"command_id":id(),"payload":{"title":title}}),
                )
                .unwrap()["id"]
                .as_str()
                .unwrap()
                .to_string();
            let fid=w.dispatch("feature.create",json!({"project_id":pid,"command_id":id(),"payload":{"project_id":pid,"title":title}})).unwrap()["id"].as_str().unwrap().to_string();
            w.dispatch("attempt.start",json!({"project_id":pid,"command_id":id(),"target":fid,"expected_revision":1,"payload":{"phase":"planning"}})).unwrap();
            w.tick().unwrap();
            w.tick().unwrap();
            projects.push(w.active_engine(Some(&pid)).unwrap());
        }
        let called = calls.lock().unwrap();
        for e in projects {
            assert!(
                e.store
                    .snapshot()
                    .unwrap()
                    .runs
                    .iter()
                    .any(|run| called.contains(&run.id))
            );
            assert_eq!(e.harness.identity(), "fixture:workspace-harness:1");
        }
    }
}
