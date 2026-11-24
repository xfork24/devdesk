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

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("report", type=Path)
parser.add_argument("--github-repo", required=True)
parser.add_argument("--require-packaged-handlers", action="store_true")
parser.add_argument("--require-delivery-evidence", action="store_true")
args = parser.parse_args()
root = args.report.resolve()
context = json.loads((root / "context.json").read_text())
views = json.loads((root / "latest-tree.json").read_text())
project = root / "data" / "projects" / context["project"]
db = sqlite3.connect(f"file:{project / 'engineering.sqlite3'}?mode=ro", uri=True)
repo = args.github_repo
assert "test" in repo.lower(), "use a dedicated test repository"
assert len(views) == 2
seen = set()
with sqlite3.connect(f"file:{root / 'data/domain-packages.sqlite3'}?mode=ro", uri=True) as registry:
    package_digest, archive = registry.execute(
        "SELECT p.digest,p.zip FROM domain_projects d JOIN domain_packages p ON p.digest=d.digest WHERE d.project=?",
        (context["project"],),
    ).fetchone()
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

expected_refs = {context["defaults"]["workflow_ref"], context["defaults"]["task_workflow_ref"]}
assert {v["runtime"]["workflow_ref"] for v in views.values()} == expected_refs
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
    for receipt in (source_regression, candidate_regression):
        assert receipt["status"] == "succeeded" and receipt["exit_code"] == 0
        assert receipt["counts"]["tests"] > 0 and receipt["container_id"] and receipt["report_sha256"]
    assert merged["synchronized"] and merged["sync"]["synchronized"]
    assert merged["sync"]["revision"] == merged["revision"]
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
           "candidate_regression": candidate_regression["operation_id"], "synchronized": True}
    if ident != context["root"]:
        red, green = values["red", "result"], values["green", "result"]
        assert red["assertion_failed"] and red["expected_failure"] and red["exit_code"] != 0
        assert green["exit_code"] == 0 and green["counts"]["failed"] == 0 and not green["expected_failure"]
        assert green["counts"]["tests"] == red["counts"]["tests"] and red["recipe"] == green["recipe"]
        assert green["container_id"] != red["container_id"]
        row["red"] = {"failed": red["counts"]["failed"], "operation": red["operation_id"], "revision":red["candidate"]}
        row["green"] = {"passed":green["counts"]["passed"], "operation":green["operation_id"], "revision":green["candidate"]}
    else:
        assert pr["base_branch"] == context["mainline"]
    stages.append(row)

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
confirmations = {item["execution_id"]: item for (raw,) in db.execute(
    "SELECT data FROM governance_records WHERE type='workflow_confirmation'") if (item := json.loads(raw))}
root_steps = views[context["root"]]["runtime"]["steps"]
for step in root_steps:
    if step["step"] in {"requirements_review", "design_review", "plan_review", "approval"}:
        confirmed = confirmations[step["id"]]
        assert confirmed["input_digest"] == step["input_digest"]
        assert confirmed["outputs"] == step["outputs"]
        assert confirmed["override_completion"] is False and confirmed["actor"]["kind"] == "human"
authorizations = [json.loads(row[0]) for row in db.execute("SELECT data FROM governance_records WHERE type='workflow_human_decision'")]
if mode == "agent":
    assert len(authorizations) >= 4 and all(item["approved"] and item["actor"]["kind"] == "human" for item in authorizations)
creation_plans = []
for (record,) in db.execute("SELECT data FROM governance_records WHERE type='platform_publish_stage'"):
    action = payload(json.loads(record)["action"])
    if action.get("PlannedDeliveryPullRequest"):
        plan = action["PlannedDeliveryPullRequest"]
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
for effect, call, record in db.execute("SELECT id,definition_id,data FROM governance_records WHERE type='platform_effect_intent'"):
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
    with ZipFile(io.BytesIO(archive)) as installed:
        for view in views.values():
            # The package manifest only includes concrete workflow source revisions.
            ref = view["runtime"]["workflow_ref"]
            source, revision = ref.rsplit(":", 1)
            frozen = json.loads(installed.read(f"configurations/{source}-v{revision}.json"))["data"]
            assert "delivery_policy" not in frozen
            assert frozen["operation_evidence"]["git.delivery"]["checks"]["$completion"] == ["delivery_receipt"]
    for verified in evidence_decisions.values():
        assert set(verified) == {"workspace.target", "git.source.publish", "git.delivery.open", "git.delivery.merge"}
result = {"domain_digest": package_digest, "domain_version": manifest["version"],
          "root":context["root"], "repository":repo, "mode":mode,
          "workflows":stages,"published_creation_plans":creation_plans,"frozen_reporters":reporters,
          "human_decisions":len(authorizations),"step_confirmations":len(confirmations),
          "dispatch_actors":dispatch_actors,
          "domain_evidence_decisions":evidence_decisions}
(root / "delivery-audit.json").write_text(json.dumps(result,indent=2,ensure_ascii=False)+"\n")
print(json.dumps(result,ensure_ascii=False))
