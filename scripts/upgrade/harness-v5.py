#!/usr/bin/env python3
"""Explicit offline upgrade to global 14 / project 45 and Workflow v5.

Dry-run by default. Acquires the same dataset locks as Host, backs up each SQLite
file, journals per-database commits, and never touches credentials or artifacts.
"""
import argparse
import contextlib
import fcntl
import hashlib
import json
import os
import re
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import uuid


def compact(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def digest(value):
    return hashlib.sha256(compact(value).encode()).hexdigest()


def stable(value):
    return str(uuid.uuid5(uuid.NAMESPACE_URL, "devdesk:harness-v5:" + value))


def remap(value, mapping):
    if isinstance(value, str):
        return mapping.get(value, value)
    if isinstance(value, list):
        return [remap(v, mapping) for v in value]
    if isinstance(value, dict):
        return {mapping.get(k, k): (v if k == "definition_id" else remap(v, mapping)) for k, v in value.items()}
    return value


def load_records(conn):
    result = []
    for row in conn.execute("SELECT * FROM governance_records ORDER BY rowid"):
        item = dict(row)
        item["data"] = json.loads(item["data"])
        item["scope"] = json.loads(item["scope"])
        item["immutable"] = bool(item["immutable"])
        result.append(item)
    return result


def insert_record(conn, record):
    value = dict(record)
    value["data"] = compact(value["data"])
    value["scope"] = compact(value["scope"])
    conn.execute("INSERT INTO governance_records VALUES (?,?,?,?,?,?,?,?,?,?,?,?)", [value[k] for k in ["id", "kind", "definition_id", "scope", "name", "version", "supersedes", "content_hash", "data", "immutable", "revision", "created_at"]])


def record(kind, identity, scope, data, name="v5 upgrade", immutable=True):
    return dict(id=stable(kind + ":" + identity), kind=kind, definition_id=identity, scope=scope,
                name=name, version=1, supersedes=None, content_hash=digest(data), data=data,
                immutable=immutable, revision=1, created_at=int(time.time() * 1000))


def tables(conn):
    return {row[0] for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}


def blockers(conn):
    present = tables(conn)
    result = []
    if "executions" in present:
        result.extend("worker:" + row[0] for row in conn.execute("SELECT id FROM executions WHERE writer_revoked=0"))
    if "turns" in present:
        result.extend("turn:" + row[0] for row in conn.execute("SELECT id FROM turns WHERE status IN ('dispatching','running','waiting_interaction','needs_reconciliation')"))
    for row in load_records(conn):
        if row["kind"] in ("platform_call", "tool_call") and row["data"].get("status") in ("running", "dispatched", "needs_reconciliation"):
            result.append("call:" + row["id"])
    if "repository_operations" in present:
        result.extend("repository:" + row[0] for row in conn.execute("SELECT id FROM repository_operations WHERE status IN ('dispatched','needs_reconciliation')"))
    return result


def normalize(binary, directory, value):
    source = directory / "normalize-input.json"
    target = directory / "normalize-output.json"
    source.write_text(compact(value))
    subprocess.run([str(binary), "--normalize-plan", str(source), "--output", str(target)], check=True, capture_output=True)
    return json.loads(target.read_text())


def conversion_mapping(all_records):
    return {r["id"]: stable("workflow:" + r["id"]) for r in all_records
            if r["kind"] == "workflow" and r["data"].get("schema_version") == 4}


def upgrade(conn, path, binary, temp, factory, mapping, source_version, shared=None, projections=None):
    current = load_records(conn)
    original = {r["id"]: r for r in current}
    now = int(time.time() * 1000)
    conn.execute("CREATE TABLE IF NOT EXISTS upgrade_archive (id TEXT PRIMARY KEY, table_name TEXT NOT NULL, row_json JSON NOT NULL CHECK(json_valid(row_json)))")
    conn.execute("CREATE TRIGGER IF NOT EXISTS upgrade_archive_no_update BEFORE UPDATE ON upgrade_archive BEGIN SELECT RAISE(ABORT,'immutable upgrade archive'); END")
    conn.execute("CREATE TRIGGER IF NOT EXISTS upgrade_archive_no_delete BEFORE DELETE ON upgrade_archive BEGIN SELECT RAISE(ABORT,'immutable upgrade archive'); END")
    for table in ("governance_records", "definitions", "sessions", "turns", "features", "tasks"):
        if table in tables(conn):
            for row in conn.execute("SELECT * FROM " + table).fetchall():
                conn.execute("INSERT OR IGNORE INTO upgrade_archive VALUES (?,?,?)", (table + ":" + row["id"], table, compact(dict(row))))

    guards=conn.execute("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name IN ('governance_identity_update','governance_immutable_update')").fetchall()
    for guard in guards:conn.execute('DROP TRIGGER "'+guard["name"]+'"')

    for item in shared or []:
        if not conn.execute("SELECT 1 FROM governance_records WHERE id=?", (item["id"],)).fetchone():
            insert_record(conn, item)
    for item in projections or []:
        if not conn.execute("SELECT 1 FROM definitions WHERE id=?", (item["id"],)).fetchone():
            conn.execute("INSERT INTO definitions (id,family,name,version,supersedes,body,agent_profile) VALUES (?,?,?,?,?,?,?)", (item["id"], item["family"], item["name"], item["version"], item["supersedes"], item["body"], item["agent_profile"]))
    next_versions = {}
    for r in load_records(conn):
        key = (r["kind"], r["definition_id"])
        next_versions[key] = max(next_versions.get(key, 0), r["version"])

    catalog_before = next((r for r in current if r["kind"] == "template_catalog_state"), None)
    generation = catalog_before["data"]["generation"] if catalog_before else 1
    members = {r["definition_id"]: r for r in current if r["kind"] == "template_catalog_member"}
    factory_members = {r["definition_id"]: r for r in factory["governance"] if r["kind"] == "template_catalog_member"}
    def add_member(item, source=None, member_generation=None):
        kind = "workflow" if item["kind"] == "workflow_draft" else item["kind"]
        if kind not in ("role","skill","spec","standard","workflow","delivery_type","capability","agent","model","harness","sandbox","tool","integration","integration_binding","permission_profile"):
            return
        if conn.execute("SELECT 1 FROM governance_records WHERE kind='template_catalog_member' AND definition_id=?", (item["id"],)).fetchone():
            return
        data = dict(source["data"]) if source else {"kind":kind,"template_key":None,"source_hash":None,"origin":"custom"}
        data["generation"] = generation if member_generation is None else member_generation
        member = record("template_catalog_member", item["id"], item["scope"], data, item["name"])
        member["id"] = "template-member:" + digest([kind,item["id"]])
        member["created_at"] = 0
        insert_record(conn, member)

    converted = []
    for old in current:
        if old["kind"] != "workflow" or old["data"].get("schema_version") != 4:
            continue
        existing = conn.execute("SELECT * FROM governance_records WHERE id=?", (mapping[old["id"]],)).fetchone()
        if existing:
            canonical = dict(existing)
            canonical["data"] = json.loads(canonical["data"])
            canonical["scope"] = json.loads(canonical["scope"])
            canonical["immutable"] = bool(canonical["immutable"])
            converted.append(canonical)
            continue
        new = dict(old)
        data = remap(old["data"], mapping)
        data["schema_version"] = 5
        data.setdefault("action_requirements", {})
        if any(s.get("executor", {}).get("tool_ref", "").startswith("builtin-tool-git-advance") for s in data["steps"]):
            data["action_requirements"]["git.advance"] = ["confirmation", "tests"]
        data = normalize(binary, temp, {"definition_id": old["definition_id"], "document": data})["document"]
        key = (old["kind"], old["definition_id"])
        next_versions[key] += 1
        new.update(id=mapping[old["id"]], version=next_versions[key], supersedes=old["id"], data=data, content_hash=digest(data), created_at=now)
        converted.append(new)
        if new["id"] not in original:
            insert_record(conn, new)
        old_member = members.get(old["id"])
        add_member(new, old_member, old_member["data"]["generation"] if old_member else 1)

    # Old automatic scheduler intents must never execute under the new authority model.
    for old in current:
        if old["kind"] == "workflow_intent":
            conn.execute("UPDATE governance_records SET kind='archived_workflow_intent',immutable=1 WHERE id=?",(old["id"],))
        if old["kind"] in ("platform_call","tool_call") and old["data"].get("status") == "prepared":
            data=dict(old["data"]);data["status"]="cancelled";data["cancel_requested"]=True
            conn.execute("UPDATE governance_records SET kind=?,immutable=1,data=?,content_hash=?,revision=revision+1 WHERE id=?",("archived_"+old["kind"],compact(data),digest(data),old["id"]))

    # Mutable authoring drafts are converted in place; exact originals remain in upgrade_archive.
    for old in current:
        if old["kind"] == "workflow_draft" and old["data"].get("document",{}).get("schema_version") == 4:
            data=remap(old["data"],mapping);data["document"]["schema_version"]=5
            data["document"]=normalize(binary,temp,{"definition_id":old["definition_id"],"document":data["document"]})["document"]
            conn.execute("UPDATE governance_records SET data=?,content_hash=?,revision=revision+1 WHERE id=?",(compact(data),digest(data),old["id"]))

    # Follow the active catalog identities, including a catalog reset before upgrade.
    # Converted adopted workflows keep their business graph; changed factory agents
    # and tools receive new revisions of the active logical definition.
    def origin_key(value):
        return re.sub(r"-v[0-9]+$", "", value or "")
    active_sources = {}
    for ident, member in members.items():
        if member["data"]["generation"] == generation and ident in original:
            key = (original[ident]["kind"], origin_key(member["data"].get("template_key")))
            if key[1] and (key not in active_sources or original[ident]["version"] > active_sources[key]["version"]):
                active_sources[key] = original[ident]
    factory_map, baselines = {}, {}
    for source in factory["governance"]:
        origin = factory_members.get(source["id"], {}).get("data", {}).get("template_key")
        baseline = active_sources.get((source["kind"], origin_key(origin))) or original.get(source["id"])
        if not baseline:
            continue
        baselines[source["id"]] = baseline
        factory_map[source["id"]] = mapping.get(baseline["id"], baseline["id"])
        if source["definition_id"] != baseline["definition_id"]:
            factory_map[source["definition_id"]] = baseline["definition_id"]
    revised = set()
    for _ in range(len(factory["governance"]) + 1):
        changed = False
        for source in factory["governance"]:
            baseline = baselines.get(source["id"])
            if source["kind"] in ("workflow", "template_catalog_state", "template_catalog_member") or source["id"] in revised or not baseline:
                continue
            if remap(source["data"], factory_map) != baseline["data"]:
                factory_map[source["id"]] = stable("factory:" + baseline["id"] + ":" + digest(source["data"]))
                revised.add(source["id"])
                changed = True
        if not changed:
            break
    installed = {r[0] for r in conn.execute("SELECT id FROM governance_records")}
    for source in factory["governance"]:
        if source["kind"] in ("template_catalog_state", "template_catalog_member", "agent_instance"):
            continue
        new = remap(source, factory_map)
        if new["id"] in installed:
            continue
        baseline = baselines.get(source["id"])
        if baseline:
            new["definition_id"] = baseline["definition_id"]
            new["supersedes"] = baseline["id"]
        key = (new["kind"], new["definition_id"])
        next_versions[key] = next_versions.get(key, 0) + 1
        new["version"] = max(new["version"], next_versions[key])
        new["content_hash"] = digest(new["data"])
        insert_record(conn, new)
        add_member(new, factory_members.get(source["id"]))
        installed.add(new["id"])

    # Install execution projections with the same remapped exact revision IDs.
    for source in factory["definitions"]:
        new = remap(source, factory_map)
        if not conn.execute("SELECT 1 FROM definitions WHERE id=?", (new["id"],)).fetchone():
            conn.execute("INSERT INTO definitions (id,family,name,version,supersedes,body,agent_profile) VALUES (?,?,?,?,?,?,?)", (new["id"], new["family"], new["name"], new["version"], new.get("supersedes"), new["body"], compact(new["agent_profile"]) if new.get("agent_profile") else None))

    factory_catalog = next(r for r in factory["governance"] if r["kind"] == "template_catalog_state")
    defaults = remap(factory_catalog["data"]["defaults"], factory_map)
    catalog = next((r for r in load_records(conn) if r["kind"] == "template_catalog_state"), None)
    if catalog:
        data = dict(catalog["data"])
        old_defaults = remap(data.get("defaults", {}), mapping)
        data["defaults"] = {**defaults, **old_defaults, "orchestrator_role_ref": defaults["orchestrator_role_ref"], "control_policy": defaults["control_policy"]}
        data["defaults"]["role_agent_bindings"][defaults["orchestrator_role_ref"]] = defaults["role_agent_bindings"][defaults["orchestrator_role_ref"]]
        data["bundle_version"] = factory_catalog["data"]["bundle_version"]
        data["bundle_hash"] = factory_catalog["data"]["bundle_hash"]
        conn.execute("UPDATE governance_records SET data=?,content_hash=?,revision=revision+1 WHERE id=?", (compact(data), digest(data), catalog["id"]))
    else:
        new = remap(factory_catalog, factory_map)
        insert_record(conn, new)

    if "orchestration_leases" not in tables(conn) and "projects" in tables(conn):
        conn.execute("CREATE TABLE orchestration_wakeups(id TEXT PRIMARY KEY,controller_id TEXT NOT NULL,event_cursor INTEGER NOT NULL,status TEXT NOT NULL,turn_id TEXT,created_at INTEGER NOT NULL,UNIQUE(controller_id,event_cursor))")
        conn.execute("CREATE INDEX orchestration_wakeups_pending ON orchestration_wakeups(status,controller_id)")
        conn.execute("CREATE TABLE orchestration_leases(id TEXT PRIMARY KEY,controller_id TEXT NOT NULL UNIQUE,owner_id TEXT NOT NULL,root_instance_id TEXT,epoch INTEGER NOT NULL CHECK(epoch>0),expires_at INTEGER NOT NULL)")
        conn.execute("CREATE UNIQUE INDEX orchestration_root_writer ON orchestration_leases(root_instance_id) WHERE root_instance_id IS NOT NULL")

    if "projects" in tables(conn):
        for old in current:
            if old["kind"] in ("configured_execution_task", "configured_session"):
                conn.execute("UPDATE governance_records SET kind=?,immutable=1 WHERE id=?", ("archived_" + old["kind"], old["id"]))
            if old["kind"] == "project_configuration":
                new = dict(old)
                data = remap(old["data"], mapping)
                data["orchestrator_role_ref"] = defaults["orchestrator_role_ref"]
                data["control_policy"] = defaults["control_policy"]
                data["role_agent_bindings"][defaults["orchestrator_role_ref"]] = defaults["role_agent_bindings"][defaults["orchestrator_role_ref"]]
                new.update(id=stable("project-config:" + old["id"]), version=old["version"] + max(r["version"] for r in current if r["kind"] == "project_configuration"), supersedes=old["id"], data=data, content_hash=digest(data), created_at=now)
                insert_record(conn, new)
            if old["kind"] != "workflow_instance":
                continue
            instance = remap(old["data"], mapping)
            instance["schema_version"] = 5
            adopted = conn.execute("SELECT data FROM governance_records WHERE id=?", (instance["workflow_ref"],)).fetchone()
            if adopted:
                instance["plan"]["document"] = json.loads(adopted[0])
            for key, definition in list(instance["plan"].get("definitions", {}).items()):
                if definition.get("kind") == "workflow":
                    adopted_definition = conn.execute("SELECT * FROM governance_records WHERE id=?", (key,)).fetchone()
                    if adopted_definition:
                        definition = dict(adopted_definition)
                        definition["data"] = json.loads(definition["data"])
                        definition["scope"] = json.loads(definition["scope"])
                        definition["immutable"] = bool(definition["immutable"])
                        instance["plan"]["definitions"][key] = definition
            instance["control_version"] = 1
            definition_id = original.get(old["data"]["workflow_ref"], {}).get("definition_id", old["data"]["workflow_ref"])
            normalized = normalize(binary, temp, {"definition_id": definition_id, "plan": instance["plan"]})
            instance["plan"] = normalized["plan"]
            scope = old["scope"]
            plan_id = stable("plan:" + old["id"])
            plan_data = {"instance_id": old["id"], "plan": instance["plan"], "inputs": instance["inputs"], "role_agent_bindings": instance["role_agent_bindings"], "parent_revision_id": None, "reason": "explicit v4 conversion", "archived_record_id": "governance_records:" + old["id"]}
            frozen = record("workflow_plan_revision", instance["root_instance_id"], scope, plan_data)
            frozen["id"] = plan_id
            insert_record(conn, frozen)
            instance["active_plan_revision_id"] = plan_id
            by_key = {s["key"]: s for s in instance["plan"]["document"]["steps"]}
            for execution in instance["steps"]:
                step = by_key[execution["step"]]
                execution.update(plan_revision_id=plan_id, node_id=step["node_id"], contract_digest=normalized["contract_digests"][step["key"]])
                if execution["status"] == "waiting_confirmation" and step["confirmation"] == "automatic":
                    execution["status"] = "awaiting_acceptance"
            conn.execute("UPDATE governance_records SET data=?,content_hash=?,revision=revision+1 WHERE id=?", (compact(instance), digest(instance), old["id"]))
        conn.execute("UPDATE turns SET status='cancelled',cancel_requested=1,finished=?,revision=revision+1 WHERE status='queued'", (now,))
        conn.execute("UPDATE sessions SET lifecycle='closed',revision=revision+1 WHERE id IN (SELECT definition_id FROM governance_records WHERE kind='archived_configured_session')")
    for guard in guards:conn.execute(guard["sql"])
    version = 14 if source_version in (13, 14) else 45
    conn.execute("PRAGMA user_version=" + str(version))
    violations = conn.execute("PRAGMA foreign_key_check").fetchall()
    if violations:
        raise RuntimeError("foreign key verification failed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("data", type=Path)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--backup-directory", type=Path)
    parser.add_argument("--max-databases",type=int,help="checkpoint after this many converted databases; rerun to continue")
    args = parser.parse_args()
    if args.max_databases is not None and args.max_databases < 1:parser.error("--max-databases must be positive")
    if args.data.is_symlink():
        raise RuntimeError("symlink dataset is not supported")
    root, binary = args.data.resolve(), args.binary.resolve()
    if not (root / "global.sqlite3").is_file():
        raise RuntimeError("global.sqlite3 is required")
    paths = [root / "global.sqlite3", *sorted((root / "projects").glob("*/engineering.sqlite3")), *sorted((root / "conversations").glob("*/engineering.sqlite3"))]
    paths = [p for p in paths if p.exists()]
    if not paths or any(p.is_symlink() or p.parent.is_symlink() for p in paths):
        raise RuntimeError("no valid dataset or symlink database")
    for path in paths:
        current=root
        for part in path.relative_to(root).parts:
            current=current/part
            if current.is_symlink():raise RuntimeError("symlink dataset member: "+str(current))
    with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(prefix="devdesk-v5-normalize-") as tmp:
        for directory in sorted({p.parent for p in paths}):
            handle = stack.enter_context((directory / ".devdesk.lock").open("a+"))
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        connections = []
        report = []
        for path in paths:
            conn = sqlite3.connect(path)
            conn.row_factory = sqlite3.Row
            stack.callback(conn.close)
            source = conn.execute("PRAGMA user_version").fetchone()[0]
            if source not in (13, 14, 44, 45):
                raise RuntimeError("unsupported source version: " + str(source))
            blocked = blockers(conn)
            connections.append((path, conn, source))
            records=load_records(conn)
            report.append({"database":str(path.relative_to(root)),"source_version":source,"blockers":blocked,"instances":sum(r["kind"]=="workflow_instance" for r in records),"queued_intents":sum(r["kind"]=="workflow_intent" and r["data"].get("status")=="prepared" for r in records),"prepared_calls":sum(r["kind"] in ("platform_call","tool_call") and r["data"].get("status")=="prepared" for r in records)})
        print(json.dumps({"apply": args.apply, "databases": report}, ensure_ascii=False))
        if not args.apply:
            return
        if any(item["blockers"] for item in report):
            raise RuntimeError("stop and reconcile active executors before conversion")
        temp = Path(tmp)
        export = temp / "factory.json"
        subprocess.run([str(binary), "--factory-export", str(export)], check=True, capture_output=True)
        factory = json.loads(export.read_text())
        mapping = conversion_mapping([r for _, conn, _ in connections for r in load_records(conn)])
        journal_path = root / "harness-v5-upgrade.json"
        if journal_path.exists():
            journal = json.loads(journal_path.read_text())
        else:
            backup = (args.backup_directory or Path(tempfile.mkdtemp(prefix="devdesk-v5-backup-"))).resolve()
            if backup == root or root in backup.parents:
                raise RuntimeError("backup must be outside the source dataset")
            if backup.exists() and any(backup.iterdir()):raise RuntimeError("new backup directory must be empty; resume uses the existing journal")
            journal = {"backup_directory": str(backup), "completed": [], "state": "running"}
        def save_journal():
            pending = journal_path.with_suffix(".pending")
            with pending.open("w") as handle:
                handle.write(compact(journal))
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(pending, journal_path)
        save_journal()
        shared, projections = [], []
        converted_count=0
        for path, conn, source in connections:
            if path.name == "global.sqlite3" and source == 14:
                shared = [r for r in load_records(conn) if r["scope"].get("kind") == "global"]
                projections = [dict(r) for r in conn.execute("SELECT * FROM definitions")]
            relative = str(path.relative_to(root))
            if source in (14, 45):
                continue
            backup = Path(journal["backup_directory"]) / relative
            backup.parent.mkdir(parents=True, exist_ok=True)
            if not backup.exists():
                with sqlite3.connect(backup) as destination:
                    conn.backup(destination)
            with conn:
                conn.execute("BEGIN IMMEDIATE")
                upgrade(conn, path, binary, temp, factory, mapping, source, shared, projections)
            journal["completed"].append(relative)
            save_journal()
            converted_count+=1
            if args.max_databases and converted_count >= args.max_databases:
                print(json.dumps({"status":"checkpointed","backup_directory":journal["backup_directory"]}));return
            if path.name == "global.sqlite3":
                shared = [r for r in load_records(conn) if r["scope"].get("kind") == "global"]
                projections = [dict(r) for r in conn.execute("SELECT * FROM definitions")]
        journal["state"] = "complete"
        save_journal()
        print(json.dumps({"status": "complete", "backup_directory": journal["backup_directory"]}))


if __name__ == "__main__":
    main()
