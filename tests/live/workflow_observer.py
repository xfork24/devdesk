"""Read an actual workflow tree through bounded status pages, without issuing controls."""


def manager_activity(view):
    """Track bounded manager activity, independent of omitted internal cursors.

    This is observer recovery grace, not the product's business-progress watchdog.
    A project event cursor alone can change for unrelated bookkeeping.
    """
    record = view.get("record") or {}
    data = record.get("data", {})
    calls = tuple((row.get("id"), (row.get("data") or row).get("status"))
                  for row in view.get("calls", []))
    return record.get("revision"), data.get("session_id"), data.get("status"), calls


def read_workflow_tree(client, root_id):
    pending = [root_id]
    views = {}
    while pending:
        identity = pending.pop(0)
        if identity in views:
            continue
        view = client.rpc("query.workflow_instance", {"id": identity, "view": "status"})
        step_page = view
        while step_page.get("next_step_offset") is not None:
            step_page = client.rpc("query.workflow_instance", {
                "id": identity, "view": "status", "step_offset": step_page["next_step_offset"],
            })
            view["runtime"]["steps"].extend(step_page["runtime"]["steps"])
            view["plan"]["steps"].extend(step_page["plan"]["steps"])
            for key in ("step_revisions", "completion_status", "actors"):
                view[key].update(step_page.get(key, {}))
        view["record"]["data"] = view["runtime"]
        views[identity] = view
        page = view
        while True:
            pending.extend(child["id"] for child in page.get("children", []))
            offset = page.get("next_child_offset")
            if offset is None:
                break
            page = client.rpc("query.workflow_instance", {
                "id": identity, "view": "status", "child_offset": offset,
            })
    return views
