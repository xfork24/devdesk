// Only the latest runs for this exact source event and commit authorize e2e.
module.exports = async ({ github, context, core }) => {
  const trigger = context.payload.workflow_run;
  const repository = context.payload.repository.full_name;
  if (trigger.conclusion !== "success" || trigger.head_repository?.full_name !== repository) {
    core.setOutput("ready", "false");
    return;
  }
  const latest = await Promise.all(["rust.yml", "web.yml"].map(async (workflow_id) => {
    const { data } = await github.rest.actions.listWorkflowRuns({
      ...context.repo,
      workflow_id,
      head_sha: trigger.head_sha,
      event: trigger.event,
      per_page: 100,
    });
    const run = data.workflow_runs
      .filter(run => run.head_repository?.full_name === repository &&
        run.head_sha === trigger.head_sha && run.event === trigger.event)
      .sort((a, b) => b.run_number - a.run_number)[0];
    core.info(`${workflow_id}: ${run?.status}/${run?.conclusion} (${trigger.head_sha}, ${trigger.event})`);
    return run;
  }));
  const current = latest.find(run => run?.workflow_id === trigger.workflow_id);
  const ready = current?.id === trigger.id &&
    current.run_attempt === trigger.run_attempt &&
    latest.every(run => run?.status === "completed" && run.conclusion === "success");
  core.setOutput("ready", String(ready));
};
