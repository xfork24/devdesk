#!/usr/bin/env python3
"""Create workbench history and approval fixtures through normal Release Host RPCs."""

import argparse
from pathlib import Path
import time
from uuid import uuid4

from workbench import checkpoint_fixture, client, clone, host, required, verify_web, write


def command(opener, origin, method, payload, project=None, target=None, revision=None):
    params = {"command_id": str(uuid4()), "payload": payload}
    if method in ("workflow.create", "workflow.publish", "project.configuration.update"):
        params["expected_catalog_generation"] = required(opener, origin, "query.template_catalog", {})["generation"]
    if project:
        params["project_id"] = project
    if target:
        params["target"] = target
    if revision is not None:
        params["expected_revision"] = revision
    return required(opener, origin, method, params)


def create_workflow(opener, origin):
    draft = command(opener, origin, "workflow.create", {
        "name": "Workbench history control",
        "document": {
            "schema_version": 6,
            "stages": [{"key": "review", "name": "Review"}],
            "steps": [{
                "key": "review", "name": "基准审批", "stage": "review",
                "executor": {"type": "control"}, "confirmation": "human",
            }],
        },
    })
    published = command(opener, origin, "workflow.publish", {}, target=draft["id"],
                        revision=draft["revision"])
    return published["id"]


def create_feature(opener, origin, project, name):
    return command(opener, origin, "feature.create", {
        "project_id": project, "title": name, "initial_requirement": "Workbench history fixture",
        "start_workflow": False,
    }, project)["id"]


def feature_revision(opener, origin, project, feature):
    workspace = required(opener, origin, "query.workspace", {"project_id": project, "feature_id": feature})
    return next(item["revision"] for item in workspace["features"] if item["id"] == feature)


def start(opener, origin, project, feature, workflow, driver):
    return command(opener, origin, "workflow.start", {
        "workflow_ref": workflow, "role_agent_bindings": {}, "orchestration": {"mode": driver},
    }, project, feature, feature_revision(opener, origin, project, feature))


def wait_for_state(opener, origin, project, root, predicate):
    until = time.monotonic() + 30
    while time.monotonic() < until:
        state = required(opener, origin, "query.workflow_instance", {"project_id": project, "id": root})
        if predicate(state["runtime"]):
            return state
        time.sleep(.05)
    raise RuntimeError("Workflow did not reach the requested state within 30 seconds")


def mark_history(opener, origin, project, feature, workflow):
    run = start(opener, origin, project, feature, workflow, "manual")
    runtime = run["data"]
    command(opener, origin, "workflow.cancel", {
        "plan_revision_id": runtime["active_plan_revision_id"],
        "control_version": runtime["control_version"],
        "reason": "Workbench history fixture",
    }, project, run["id"], run["revision"])
    terminal = wait_for_state(opener, origin, project, run["id"], lambda flow: flow["status"] == "cancelled")
    if not terminal["runtime"]["steps"]:
        raise RuntimeError("The terminal workflow has no recorded execution")
    return run["id"]


def approval(opener, origin, project, feature, workflow):
    run = start(opener, origin, project, feature, workflow, "rust")
    state = wait_for_state(opener, origin, project, run["id"],
                           lambda flow: flow["steps"][0]["status"] == "waiting_confirmation")
    flow = state["runtime"]
    step = flow["steps"][0]
    pending = command(opener, origin, "workflow.decision.request", {
        "plan_revision_id": flow["active_plan_revision_id"], "control_version": flow["control_version"],
        "operation": "workflow.step.accept", "reason": "Workbench fixture requires a human decision",
        "arguments": {"instance_id": run["id"], "plan_revision_id": flow["active_plan_revision_id"],
                      "step": step["step"], "execution_id": step["id"], "input_digest": step["input_digest"],
                      "outputs": step["outputs"]},
    }, project, run["id"], state["record"]["revision"])
    orchestration = required(opener, origin, "query.orchestration", {"project_id": project,
                                                                    "root_instance_id": run["id"]})
    exact = next(row for row in orchestration["decisions"] if row["id"] == pending["id"])
    if exact["current"] is not True or exact["data"]["status"] != "pending":
        raise RuntimeError("The pending decision is not current")
    return run["id"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("host", "web", "dataset", "output"):
        parser.add_argument("--" + name, type=lambda p: Path(p).resolve(), required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--scale", type=int, choices=(1, 10, 100), default=100)
    args = parser.parse_args()
    if args.output.is_relative_to(args.dataset) or not (args.dataset / ".devdesk-dataset").is_file():
        parser.error("Provide a dedicated checkpointed fixture and a separate output directory")
    args.output.mkdir(mode=0o700)
    base = args.output / "1x"
    clone(args.dataset, base)
    with host(args.host, base, args.output, "prepare-1x") as (origin, _, _):
        opener = client(origin)
        verify_web(args, origin, opener)
        workflow = create_workflow(opener, origin)
        workspace = required(opener, origin, "query.workspace", {"project_id": args.project})
        command(opener, origin, "project.configuration.update", {
            "workflow_ref": workflow, "role_agent_bindings": {}, "orchestration": {"mode": "manual"},
        }, args.project, args.project, workspace["active_project"]["revision"])
        selected = create_feature(opener, origin, args.project, "Workbench selected history")
        other = create_feature(opener, origin, args.project, "Workbench outside-scope history")
        selected_roots = [mark_history(opener, origin, args.project, selected, workflow)]
        other_roots = [mark_history(opener, origin, args.project, other, workflow)]
    checkpoint_fixture(base)
    fixture = {"workflow": workflow, "project": args.project, "feature": selected, "other_feature": other}
    write(base.parent / "1x.json", {**fixture, "root": selected_roots[-1], "selected_roots": selected_roots,
                                  "other_roots": other_roots})
    for axis in ("same-feature", "other-feature"):
        previous = base
        selected_history = selected_roots.copy()
        other_history = other_roots.copy()
        for scale in (10, 100):
            if scale > args.scale:
                break
            name = f"{axis}-{scale}x"
            dataset = args.output / name
            clone(previous, dataset)
            grow = selected_history if axis == "same-feature" else other_history
            owner = selected if axis == "same-feature" else other
            with host(args.host, dataset, args.output, f"prepare-{name}") as (origin, _, _):
                opener = client(origin)
                while len(grow) < scale:
                    grow.append(mark_history(opener, origin, args.project, owner, workflow))
            checkpoint_fixture(dataset)
            write(args.output / f"{name}.json", {**fixture, "root": selected_history[-1],
                  "selected_roots": selected_history, "other_roots": other_history})
            previous = dataset
    approval_data = args.output / "approval"
    clone(base, approval_data)
    with host(args.host, approval_data, args.output, "prepare-approval") as (origin, _, _):
        opener = client(origin)
        approval_root = approval(opener, origin, args.project, selected, workflow)
    checkpoint_fixture(approval_data)
    write(args.output / "approval.json", {**fixture, "root": selected_roots[-1], "approval_root": approval_root})


if __name__ == "__main__":
    main()
