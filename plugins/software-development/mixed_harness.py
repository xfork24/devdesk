"""Derive the cross-Harness Standard from the two reviewed engineering DAGs."""

from copy import deepcopy


DOMAIN = "software-development"
FEATURE_SOURCE = f"{DOMAIN}.workflow-feature-development:6"
TASK_SOURCE = f"{DOMAIN}.workflow-task-development:5"
FEATURE = f"{DOMAIN}.workflow-mixed-harness-feature:1"
TASK = f"{DOMAIN}.workflow-mixed-harness-task:1"
STANDARD = f"{DOMAIN}.standard-mixed-harness-engineering:1"
RELEASE_REVIEWER = f"{DOMAIN}.role-release-reviewer:1"
ZCODE_REVIEWER = f"{DOMAIN}.agent-mixed-release-reviewer-zcode:2"
STEP_TOOLS = ("builtin-tool-workflow-outputs-read:3", "builtin-tool-spec-lint:2")
FORMAL_OUTPUT = (
    "When the input is a formal Workflow task, the entire final assistant message must be one JSON object "
    "matching its output_contract. Put prose or Markdown inside the declared JSON fields. "
    "Do not place a heading, code fence, or any text outside that object."
)
PRD_OUTPUT = (
    'For a completed PRD, the final response has exactly this structure: '
    '{"status":"completed","summary":"Brief conclusion","outputs":{"result":"# PRD\\n..."}}. '
    'Replace the example result with the full PRD as a JSON string. Put any lint conclusion in summary.'
)
NATIVE = {
    harness: "builtin-tool-dsh-native:2" if harness == "dsh" else f"builtin-tool-{harness}-native:1"
    for harness in ("pi", "dsh", "mcode", "opencode", "zcode")
}


def role(name):
    return f"{DOMAIN}.role-{name}:1"


def mixed_role(name):
    return role(f"mixed-{name}")


BINDINGS = {
    role("project-manager"): f"{DOMAIN}.agent-gpt-6-2-sol-project-manager:4",
    mixed_role("analyst"): f"{DOMAIN}.agent-mixed-analyst-pi:1",
    mixed_role("architect"): f"{DOMAIN}.agent-mixed-architect-opencode:1",
    mixed_role("developer"): f"{DOMAIN}.agent-mixed-developer-mcode:1",
    mixed_role("tester"): f"{DOMAIN}.agent-mixed-tester-dsh:1",
    mixed_role("reviewer"): f"{DOMAIN}.agent-mixed-reviewer-dsh:1",
}


def revision(record, reference, name):
    identity, version = reference.rsplit(":", 1)
    return {**deepcopy(record), "identity": identity, "revision": int(version), "name": name}


def definitions(entries):
    by_ref = {f"{source['identity']}:{source['revision']}": source for source in entries}
    task = revision(by_ref[TASK_SOURCE], TASK, "跨 Harness · Task 开发")
    feature = revision(by_ref[FEATURE_SOURCE], FEATURE, "跨 Harness · Feature 开发")
    functions = {role(name): mixed_role(name) for name in ("analyst", "architect", "developer", "tester", "reviewer")}
    for document in (feature["data"], task["data"]):
        for stage in document["stages"]:
            ref = stage.get("default_role_ref")
            if ref in functions:
                stage["default_role_ref"] = functions[ref]
        for step in document["steps"]:
            executor = step["executor"]
            ref = executor.get("role_ref")
            if ref in functions:
                executor["role_ref"] = functions[ref]
                instructions = step.get("task_instructions", "")
                step["task_instructions"] = f"{instructions}\n\n{FORMAL_OUTPUT}".strip()
    for step in feature["data"]["steps"]:
        executor = step["executor"]
        if step["key"] == "implementation":
            assert executor["workflow_ref"] == TASK_SOURCE
            executor["workflow_ref"] = TASK
        elif step["key"] == "review":
            assert executor["role_ref"] == mixed_role("reviewer")
            executor["role_ref"] = RELEASE_REVIEWER
        elif step["key"] == "requirements":
            step["task_instructions"] += f"\n\n{PRD_OUTPUT}"
    standard = revision(
        by_ref[f"{DOMAIN}.standard-engineering-delivery:6"], STANDARD, "跨 Harness 软件工程",
    )
    standard.update(root_workflow_refs=[FEATURE, TASK], default_workflow_ref=FEATURE)
    reviewer = revision(by_ref[role("reviewer")], RELEASE_REVIEWER, "独立交付评审")
    reviewer["data"]["instructions"] += (
        "\n\nVerify the final aggregate Feature across its independently reviewed Tasks. "
        "Use the frozen source, test evidence and exact reviewed revision. "
        "Leave the human merge decision to its separate workflow gate."
    )
    reviewer["data"]["instructions"] += f"\n\n{FORMAL_OUTPUT}"
    sources = [reviewer, task, feature, standard]
    for original, selected in functions.items():
        selected_role = revision(by_ref[original], selected, f"{by_ref[original]['name']} · 跨 Harness")
        selected_role["data"]["instructions"] += f"\n\n{FORMAL_OUTPUT}"
        sources.append(selected_role)
    step_permissions = {
        "tests": ("simplified-permission-tests", "dsh"),
        "implementation": ("simplified-permission-implementation", "mcode"),
    }
    for step in task["data"]["steps"]:
        if step["key"] in step_permissions:
            name, harness = step_permissions[step["key"]]
            original = f"{DOMAIN}.{name}:3"
            assert step["harness_permission_ref"] == original
            reference = f"{DOMAIN}.{name}-{harness}:1"
            profile = revision(by_ref[original], reference, f"{by_ref[original]['name']} · {harness}")
            profile["data"]["policy"]["tool_refs"] = [NATIVE[harness], *STEP_TOOLS]
            step["harness_permission_ref"] = reference
            sources.append(profile)
    for function, harness, title, agent_version, capability_version, harness_name in [
        ("analyst", "pi", "需求分析", 6, 6, "Pi"),
        ("architect", "opencode", "架构设计", 6, 6, "OpenCode"),
        ("developer", "mcode", "实现", 6, 6, "mcode"),
        ("tester", "dsh", "独立测试", 6, 6, "DSH"),
        ("reviewer", "dsh", "阶段和 Task 评审", 8, 7, "DSH"),
        ("reviewer", "pi", "验收回退 · 独立交付评审", 8, 7, "Pi"),
        ("reviewer", "zcode", "独立交付评审", 8, 7, "ZCode"),
    ]:
        independent = function == "reviewer" and harness in ("pi", "zcode")
        suffix = f"release-reviewer-{harness}" if independent else f"{function}-{harness}"
        source_revision = 2 if harness == "zcode" else 1
        capability_ref = f"{DOMAIN}.capability-mixed-{suffix}:{source_revision}"
        capability = revision(
            by_ref[f"{DOMAIN}.capability-{function}-minimax-dsh:{capability_version}"],
            capability_ref, f"{title} · {harness_name} / MiniMax",
        )
        if harness != "dsh":
            capability["harness_ref"] = f"builtin-harness-{harness}-managed:{source_revision}"
            capability["model_ref"] = f"builtin-model-{harness}-minimax-m3-1-flash:1"
        capability["tools"] = [NATIVE[harness], *STEP_TOOLS]
        agent = revision(
            by_ref[f"{DOMAIN}.agent-minimax-{function}:{agent_version}"],
            f"{DOMAIN}.agent-mixed-{suffix}:{source_revision}", f"{title} · {harness_name}",
        )
        agent["capability_ref"] = capability_ref
        agent["role_ref"] = RELEASE_REVIEWER if independent else mixed_role(function)
        sources.extend((capability, agent))
    return sources
