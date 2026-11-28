#!/usr/bin/env python3
"""Verify a completed one-Task domain release against recorded receipts and actual Git/GitHub."""
import argparse
import hashlib
import io
import json
import re
import sqlite3
import subprocess
from pathlib import Path
from urllib.parse import quote
from zipfile import ZipFile

from verify_project_regression import check_test

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("report", type=Path)
parser.add_argument("--github-repo", required=True)
parser.add_argument("--data-root", type=Path, help="Existing Host dataset used by the observed workflow")
parser.add_argument("--require-packaged-handlers", action="store_true")
parser.add_argument("--require-delivery-evidence", action="store_true")
args = parser.parse_args()
root = args.report.resolve()
context = json.loads((root / "context.json").read_text())
views = json.loads((root / "latest-tree.json").read_text())
dataset = (args.data_root or root / "data").resolve()
project = dataset / "projects" / context["project"]
if not (project / "engineering.sqlite3").is_file():
    with sqlite3.connect(f"{(dataset / 'domain-packages.sqlite3').as_uri()}?mode=ro", uri=True) as registry:
        directory, = registry.execute("SELECT data_root FROM domain_project_storage WHERE project=?",
                                      (context["project"],)).fetchone()
    project = Path(directory) / "projects" / context["project"]
db = sqlite3.connect(f"file:{project / 'engineering.sqlite3'}?mode=ro", uri=True)
repo = args.github_repo
assert context.get("github_repo") == repo or "test" in repo.lower(), "use the exact designated acceptance repository"
assert len(views) == 2
seen = set()
with sqlite3.connect(f"{(dataset / 'domain-packages.sqlite3').as_uri()}?mode=ro", uri=True) as registry:
    original_digest, = registry.execute(
        "SELECT digest FROM domain_projects WHERE project=?",
        (context["project"],),
    ).fetchone()
    package_digest, package_id, archive = registry.execute(
        "SELECT digest,id,zip FROM domain_packages WHERE digest=?",
        (context["domain_digest"],),
    ).fetchone()
    if original_digest != package_digest:
        original_domain = registry.execute(
            "SELECT id FROM domain_package_identities WHERE digest=?", (original_digest,),
        ).fetchone()
        assert original_domain == (package_id,), "execution package belongs to a different domain"
assert package_digest == context["domain_digest"] == hashlib.sha256(archive).hexdigest()
with ZipFile(io.BytesIO(archive)) as installed:
    manifest = json.loads(installed.read("manifest.json"))

def payload(value):
    if not isinstance(value, dict) or "$devdesk_project_payload_v1" not in value:
        return value
    ref = value["$devdesk_project_payload_v1"]
    sha = ref["sha256"]
    assert re.fullmatch("[a-f0-9]{64}", sha)
    file = project / "payloads" / "objects" / sha[:2] / sha[2:4] / sha
    if ref.get("artifact"):
        rel, = db.execute("SELECT relative_path FROM artifacts WHERE id=?", (ref["artifact"],)).fetchone()
        file = project / rel
    if file.is_file():
        body = file.read_bytes()
    else:
        pack, start, size = db.execute("SELECT pack,offset,bytes FROM payload_pack_objects WHERE sha256=?", (sha,)).fetchone()
        assert size == ref["bytes"]
        with (project / "payloads" / "packs" / (pack + ".pack")).open("rb") as stream:
            stream.seek(start)
            body = stream.read(size)
    assert len(body) == ref["bytes"] and hashlib.sha256(body).hexdigest() == sha
    return json.loads(body)

def gh(path):
    return json.loads(subprocess.check_output(["gh", "api", "repos/" + repo + "/" + path], text=True))

def outputs(v):
    result = {}
    for step in v["runtime"]["steps"]:
        assert step["status"] in ("completed", "skipped")
        for key, identifier in step["outputs"].items():
            record, = db.execute("SELECT data FROM governance_records WHERE id=? AND type='workflow_output'", (identifier,)).fetchone()
            item = json.loads(record)
            assert item["execution_id"] == step["id"] and item["port"] == key
            result[step["step"], key] = payload(item["value"])
    return result

def check_frozen_test(receipt, revision, *, red=False, configured_files=None):
    check_test(receipt, revision, red=red)
    assert receipt["origin"] == "external" and receipt["recipe"]["image"] == receipt["image_digest"]
    files = receipt["execution"]["files"]
    declared = receipt["recipe"]["files"] if configured_files is None else configured_files
    assert sorted(file["path"] for file in files) == sorted(declared)
    for file in files:
        actual = subprocess.check_output(["git", "-C", str(project / "repository"),
                                          "show", revision + ":" + file["path"]])
        assert actual == file["content"].encode(), file["path"]
        assert hashlib.sha256(actual).hexdigest() == file["sha256"], file["path"]

def packaged_workflow(ref):
    source, revision = ref.rsplit(":", 1)
    with ZipFile(io.BytesIO(archive)) as installed:
        return json.loads(installed.read(f"configurations/{source}-v{revision}.json"))["data"]


def operation_evidence(workflow):
    return {
        name: {**policy, "checks": {step: sorted(checks) for step, checks in policy["checks"].items()}}
        for name, policy in workflow["operation_evidence"].items()
    }


def verify_plan_lineage(view, expected_ref):
    runtime = view["runtime"]
    instance = view["record"]["id"]
    revision = runtime["active_plan_revision_id"]
    lineage = []
    while revision is not None:
        assert revision not in lineage, "Cyclic plan lineage"
        lineage.append(revision)
        immutable, raw = db.execute(
            "SELECT immutable,data FROM governance_records WHERE type='workflow_plan_revision' AND id=?",
            (revision,),
        ).fetchone()
        plan = json.loads(raw)
        assert immutable and plan["instance_id"] == instance
        if len(lineage) == 1:
            assert plan["workflow_ref"] == runtime["workflow_ref"]
        parent = plan["parent_revision_id"]
        if parent is None:
            assert plan["workflow_ref"] == expected_ref
        else:
            assert plan["plan"]["base_workflow_ref"] == expected_ref
        revision = parent
    assert lineage, "Missing active plan revision"
    return lineage


root_flow = views[context["root"]]
root_ref = context.get("workflow_ref") or context["defaults"]["workflow_ref"]
root_steps = packaged_workflow(root_ref)["steps"]
task_ref = next(step["executor"]["workflow_ref"] for step in root_steps if step["key"] == "implementation")
parent_step = next(step for step in root_flow["runtime"]["steps"] if step["step"] == "implementation")
child_id, = (key for key in views if key != context["root"])
child = views[child_id]
assert child["runtime"]["parent_execution_id"] == parent_step["id"]
packaged_refs = {context["root"]: root_ref, child_id: task_ref}
plan_lineages = {key: verify_plan_lineage(view, packaged_refs[key]) for key, view in views.items()}

def bound_model(binding, agent_ref):
    if "model" in binding:
        return binding["model"]
    assert binding["$source"] == "runtime_binding"
    assert binding["agent_configuration"]["agent_ref"] == agent_ref
    with ZipFile(io.BytesIO(archive)) as installed:
        def configuration(ref, kind):
            source, revision = ref.rsplit(":", 1)
            row = json.loads(installed.read(f"configurations/{source}-v{revision}.json"))
            assert row["type"] == kind and row["identity"] == source and row["revision"] == int(revision)
            return row

        agent = configuration(agent_ref, "agent")
        capability = configuration(agent["capability_ref"], "capability")
    with sqlite3.connect(f"{(dataset / 'global.sqlite3').as_uri()}?mode=ro", uri=True) as catalog:
        model, = catalog.execute("SELECT data FROM source_definitions WHERE reference=? AND type='model'",
                                 (capability["model_ref"],)).fetchone()
    return json.loads(model)["model"]

def mixed_harness_evidence():
    expected = {
        context["root"]: {"requirements": "pi", "requirements_review": "dsh", "design": "opencode",
                          "design_review": "dsh", "planning": "codex", "plan_review": "dsh", "review": "zcode"},
        next(identity for identity in views if identity != context["root"]):
            {"tests": "dsh", "implementation": "mcode", "review": "dsh"},
    }
    evidence = []
    for instance, roles in expected.items():
        by_key = {step["step"]: step for step in views[instance]["runtime"]["steps"]}
        for key, provider in roles.items():
            step = by_key[key]
            row = db.execute(
                """SELECT sessions.runtime_binding, turns.consumed, turns.status
                FROM sessions JOIN turns ON turns.session_id = sessions.id
                WHERE sessions.id = ? AND turns.id = ?""", (step["session_id"], step["turn_id"]),
            ).fetchone()
            assert row and row[1:] == (1, "completed"), (instance, key)
            binding = payload(json.loads(row[0]))
            assert binding["installation"]["key"]["provider"] == provider, (instance, key)
            model = "gpt-6.2-sol" if provider == "codex" else "MiniMax-M3.1-Flash"
            assert bound_model(binding, step["agent_revision_id"]) == model, (instance, key)
            worker = db.execute(
                """SELECT e.operation_id, e.container_id, e.image_digest
                FROM conversation_events AS event JOIN executions AS e
                ON e.session_id = event.session_id AND event.deduplication_key = e.id || ':stop-proof'
                WHERE event.turn_id = ? AND event.kind = 'status' AND e.kind = 'harness'
                AND e.writer_revoked = 1 AND e.container_id IS NOT NULL""", (step["turn_id"],),
            ).fetchall()
            assert len(worker) == 1 and re.fullmatch(r"sha256:[a-f0-9]{64}", worker[0][2]), (instance, key)
            operation, container, image = worker[0]
            observed = {"instance": instance, "step": key, "harness": provider,
                        "model": model, "turn": step["turn_id"], "operation": operation,
                        "container": container, "image": image}
            if provider == "zcode":
                bridge_reads = []
                for call_id, raw in db.execute(
                    """SELECT id, data FROM governance_records WHERE type='platform_call'
                    AND json_extract(data, '$.workflow_authority.execution_id')=?
                    AND json_extract(data, '$.status')='succeeded'
                    AND json_extract(data, '$.handler')='workflow.outputs.read'""", (step["id"],),
                ):
                    caller = payload(json.loads(raw)["request"])["caller"]
                    assert caller["turn_id"] == step["turn_id"] and caller["operation_id"] == operation
                    assert caller["request_id"]["kind"] == "bridge"
                    bridge_reads.append(call_id)
                assert bridge_reads, "Missing native ZCode reads through the Host MCP bridge"
                observed["bridge_reads"] = bridge_reads
            evidence.append(observed)
    manager = json.loads((root / "latest-manager.json").read_text())["record"]["data"]
    manager_row, = db.execute("SELECT runtime_binding FROM sessions WHERE id=?", (manager["session_id"],)).fetchone()
    binding = payload(json.loads(manager_row))
    assert manager["status"] == "closed" and bound_model(binding, manager["agent_revision_id"]) == "gpt-6.2-sol"
    assert binding["installation"]["key"]["provider"] == "codex"
    return evidence

stages = []
for ident, view in views.items():
    assert view["runtime"]["status"] == "completed"
    values = outputs(view)
    publish = values["source_publish", "result"]
    publication = values["publication", "result"]
    merged = values["merge", "result"]
    source_regression = values["regression", "result"]
    candidate_regression = values["merge_regression", "result"]
    pr = publication["pull_request"]
    assert pr["repository"] == repo and pr["number"] not in seen
    seen.add(pr["number"])
    assert source_regression["candidate"] == publish["revision"] == publication["source_revision"]
    assert candidate_regression["candidate"] == publication["revision"]
    configured = ((context.get("test_configuration") or {}).get("regression") or {}).get("files")
    for receipt, tested in ((source_regression, publish["revision"]), (candidate_regression, publication["revision"])):
        check_frozen_test(receipt, tested, configured_files=configured)
    assert merged["synchronized"] and merged["sync"]["synchronized"]
    assert merged["sync"]["revision"] == merged["revision"] == publication["revision"]
    assert merged["sync"]["previous"] == publication["target_revision"]
    assert merged["pull_request"]["number"] == pr["number"]
    assert merged["pull_request"]["merged"] and merged["pull_request"]["merge_commit"] == merged["revision"]
    remote = gh(f"pulls/{pr['number']}")
    assert remote["merged"] and remote["merge_commit_sha"] == merged["revision"]
    assert remote["head"]["sha"] == pr["head"] and remote["base"]["ref"] == pr["base_branch"]
    commit = gh("git/commits/" + merged["revision"])
    assert commit["tree"]["sha"] == publication["tree"]
    assert commit["parents"][0]["sha"] == publication["target_revision"]
    reference = gh("branches/" + quote(pr["base_branch"], safe=""))
    assert reference["commit"]["sha"] == merged["revision"]
    path, = db.execute("SELECT path FROM repository_worktrees WHERE branch=?", (pr["base_branch"],)).fetchone()
    assert subprocess.check_output(["git", "-C", path, "rev-parse", "HEAD"], text=True).strip() == merged["revision"]
    assert not subprocess.check_output(["git", "-C", path, "status", "--porcelain"], text=True)
    row = {"instance": ident, "flow": view["runtime"]["workflow_ref"], "pr": pr["number"], "url": pr["url"],
           "source": publish["revision"], "candidate": publication["revision"], "target": publication["target_revision"],
           "merged": merged["revision"], "tree": publication["tree"], "base_branch": pr["base_branch"],
           "target_protected": reference["protected"], "source_regression": source_regression["operation_id"],
           "candidate_regression": candidate_regression["operation_id"],
           "regression_counts": {"source": source_regression["counts"], "candidate": candidate_regression["counts"]},
           "regression_containers": [source_regression["container_id"], candidate_regression["container_id"]],
           "synchronized": True}
    if ident != context["root"]:
        red, green = values["red", "result"], values["green", "result"]
        check_frozen_test(red, values["tests_snapshot", "result"]["revision"], red=True)
        check_frozen_test(green, publish["revision"])
        assert red["candidate"] != green["candidate"]
        assert green["counts"]["tests"] == red["counts"]["tests"] and red["recipe"] == green["recipe"]
        assert green["container_id"] != red["container_id"]
        assert red["execution"]["files"] == green["execution"]["files"]
        row["red"] = {"failed": red["counts"]["failed"], "operation": red["operation_id"], "revision":red["candidate"]}
        row["green"] = {"passed":green["counts"]["passed"], "operation":green["operation_id"], "revision":green["candidate"]}
    else:
        assert pr["base_branch"] == context["mainline"]
    stages.append(row)
assert len({container for row in stages for container in row["regression_containers"]}) == 2 * len(stages)

mode = context.get("mode", "agent")
drivers = [json.loads(row[0]) for row in db.execute(
    "SELECT data FROM governance_records WHERE type='workflow_driver' AND definition_id=?", (context["root"],))]
if args.require_delivery_evidence:
    if mode == "manual":
        assert {driver["mode"] for driver in drivers} <= {"manual"}
        actions = [json.loads(line) for line in (root / "manual-actions.jsonl").read_text().splitlines()]
        assert {row["instance"] for row in actions if row["method"] == "workflow.complete"} == set(views)
        assert {row["instance"] for row in actions if row["method"] == "workflow.step.dispatch"} == set(views)
    else:
        assert drivers and {driver["mode"] for driver in drivers} == {mode}
dispatch_actors = {}
for (raw,) in db.execute(
    "SELECT d.data FROM governance_records p JOIN governance_records d "
    "ON d.id=json_extract(p.data,'$.decision_id') WHERE p.type='workflow_dispatch'"):
    decision = json.loads(raw)
    if decision["root_instance_id"] == context["root"]:
        kind = decision["actor"]["kind"]
        dispatch_actors[kind] = dispatch_actors.get(kind, 0) + 1
if args.require_delivery_evidence:
    expected = {"manual": {"human"}, "rust": {"driver"}, "agent": {"controller", "segment"}}[mode]
    assert dispatch_actors and set(dispatch_actors) <= expected
execution_ids = {step["id"] for view in views.values() for step in view["runtime"]["steps"]}
confirmations = {item["execution_id"]: item for (raw,) in db.execute(
    "SELECT data FROM governance_records WHERE type='workflow_confirmation'")
    if (item := json.loads(raw))["execution_id"] in execution_ids}
root_steps = views[context["root"]]["runtime"]["steps"]
for step in root_steps:
    if step["step"] in {"requirements_review", "design_review", "plan_review", "approval"}:
        confirmed = confirmations[step["id"]]
        assert confirmed["input_digest"] == step["input_digest"]
        assert confirmed["outputs"] == step["outputs"]
        assert confirmed["override_completion"] is False and confirmed["actor"]["kind"] == "human"
authorizations = [item for (raw,) in db.execute("SELECT data FROM governance_records WHERE type='workflow_human_decision'")
                  if (item := json.loads(raw))["request"].get("arguments", {}).get("instance_id") in views]
if mode == "agent":
    assert all(item["actor"]["kind"] == "human" for item in authorizations)
    assert sum(item["approved"] is True for item in authorizations) >= 4
creation_plans = []
for (record,) in db.execute("SELECT data FROM governance_records WHERE type='platform_publish_stage'"):
    action = payload(json.loads(record)["action"])
    if action.get("PlannedDeliveryPullRequest"):
        plan = action["PlannedDeliveryPullRequest"]
        if plan["request"]["merge"] not in views:
            continue
        assert plan["creation"]["head"] == plan["request"]["head_branch"]
        assert plan["creation"]["base"] == plan["request"]["base_branch"]
        actual = next(row for row in stages if row["instance"] == plan["request"]["merge"])
        created = gh(f"pulls/{actual['pr']}")
        assert created["title"] == plan["creation"]["title"]
        assert created["body"].replace("\r\n", "\n") == plan["creation"]["body"]
        creation_plans.append(plan["request"]["merge"])
reporters = {}
evidence_decisions = {identity: {} for identity in views}
observations = {effect: payload(json.loads(data)["result"]) for effect, data in db.execute(
    "SELECT definition_id,data FROM governance_records WHERE type='platform_effect_observation' AND json_extract(data,'$.phase')='domain-authorization'")}
for effect, call, record in db.execute(
    """SELECT intent.id,intent.definition_id,intent.data FROM governance_records AS intent
       JOIN governance_records AS call ON call.id=intent.definition_id AND call.type='platform_call'
       WHERE intent.type='platform_effect_intent' AND json_extract(call.data,'$.root_instance_id')=?""",
    (context["root"],),
):
    plan = payload(json.loads(record)["plan"])
    action = payload(plan["action"])
    if check := plan.get("domain_check"):
        raw, = db.execute("SELECT data FROM governance_records WHERE id=? AND type='platform_call'", (call,)).fetchone()
        call_data = json.loads(raw)
        if call_data["status"] == "succeeded":
            assert check["handler"] == "software-development.delivery.authorize"
            operation = check["input"]["operation"]
            assert operation == call_data["handler"]
            assert observations[effect] == {"Ok": {"allowed": True}}
            instance = payload(call_data["request"])["instance_id"]
            for fact in check["input"]["facts"].values():
                assert re.fullmatch("[a-f0-9]{64}", fact["output_digest"])
                assert fact["execution_id"]
            evidence_decisions[instance][operation] = effect
    if action.get("Test",{}).get("reporter"):
        test = action["Test"]
        reporter = test["reporter"]
        reporter_path = manifest["runtime"]["test_reporters"][test["recipe"]["runner"]]
        assert (hashlib.sha256(reporter["content"].encode()).hexdigest()
                == reporter["digest"] == manifest["files"][reporter_path])
        reporters[test["operation_id"]] = reporter["digest"]
if args.require_packaged_handlers:
    assert len(creation_plans) == 2 and len(reporters) >= 6
if args.require_delivery_evidence:
    for identity, view in views.items():
        frozen = packaged_workflow(packaged_refs[identity])
        assert "delivery_policy" not in frozen
        assert frozen["operation_evidence"]["git.delivery"]["checks"]["$completion"] == ["delivery_receipt"]
        ref = view["runtime"]["workflow_ref"]
        if ref != packaged_refs[identity]:
            raw, = db.execute("SELECT data FROM source_definitions WHERE type='workflow' AND reference=?",
                              (ref,)).fetchone()
            active = json.loads(raw)
            assert operation_evidence(active) == operation_evidence(frozen)
            for policy in ("completion", "review_policies", "rework_policies"):
                assert active.get(policy) == frozen.get(policy), policy
    for verified in evidence_decisions.values():
        assert set(verified) == {"workspace.target", "git.source.publish", "git.delivery.open", "git.delivery.merge"}
result = {"domain_digest": package_digest, "domain_version": manifest["version"],
          "root":context["root"], "repository":repo, "mode":mode,
          "workflows":stages,"published_creation_plans":creation_plans,"frozen_reporters":reporters,
          "human_decisions":len(authorizations),"step_confirmations":len(confirmations),
          "dispatch_actors":dispatch_actors,"plan_lineages":plan_lineages,
          "domain_evidence_decisions":evidence_decisions}
if context.get("standard_ref") == "software-development.standard-mixed-harness-engineering:1":
    result["native_harness_turns"] = mixed_harness_evidence()
(root / "delivery-audit.json").write_text(json.dumps(result,indent=2,ensure_ascii=False)+"\n")
print(json.dumps(result,ensure_ascii=False))
