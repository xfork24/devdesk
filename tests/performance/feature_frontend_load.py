"""Feature acceptance adds real manager cards to the unchanged native load probe."""
import json
from pathlib import Path
import shutil

from native_frontend_load import NativeFrontendLoad, stats


class FeatureFrontendLoad(NativeFrontendLoad):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        shutil.copy2(__file__, self.output / Path(__file__).name)

    def request(self, method, params, kind, scheduled, client):
        # Half the tool-call-summary readers visit the actual manager cards.
        # Total arrival rates, independent writes and SSE scopes stay the same.
        if kind == "read" and method == "query.tool_calls" and client % 2 == 0 and self.target:
            method = "query.orchestration"
            params = {
                "project_id": self.target["project_id"],
                "root_instance_id": self.target["id"],
            }
        return super().request(method, params, kind, scheduled, client)

    def finish(self):
        summary = super().finish()
        summary["read_mix"] = {
            "query.execution_workspace": 0.2,
            "query.workflow_progress": 0.2,
            "query.workflow_driver": 0.2,
            "query.history": 0.2,
            "query.tool_calls": 0.1,
            "query.orchestration": 0.1,
        }
        summary["read_mix_scope"] = "After root attachment; setup reads before attachment use the original workspace fallback."
        summary["by_method"] = {}
        for method in sorted({row["method"] for row in self.rows}):
            selected = [row for row in self.rows if row["method"] == method]
            summary["by_method"][method] = {
                **stats([row["ms"] for row in selected]),
                "failed": sum(bool(row.get("error")) for row in selected),
                "bytes_max": max((row.get("bytes", 0) for row in selected), default=0),
                "generator_queue": stats([row["generator_queue_ms"] for row in selected]),
            }
        (self.output / "summary.json").write_text(json.dumps(summary, indent=2))
        return summary
