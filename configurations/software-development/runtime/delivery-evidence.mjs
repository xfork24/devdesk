export function authorizeDelivery(input) {
  const { operation, source_revision, source_branch, destination, candidate_revision,
    regression_configured, facts } = input;
  const result = (name) => facts[name]?.result;
  const deny = (reason) => ({ allowed: false, reason });
  const tested = (name, revision, projectRegression) => {
    const fact = facts[name];
    const value = fact?.result;
    return fact?.platform_operation === (projectRegression ? "tests.project" : "tests.run")
      && value?.status === "succeeded" && value.candidate === revision
      && value.expected_failure === false && value.writer_revoked === true
      && (value.skipped !== true
        || (projectRegression && !regression_configured && value.reason === "no_regression_configured"));
  };
  if (!result("review") || facts.review.workspace_revision !== source_revision) {
    return deny("独立代码评审必须绑定确切源版本");
  }
  if (operation === "workspace.target") return { allowed: true };
  if (!["git.source.publish", "git.delivery.open", "git.delivery.merge"].includes(operation)) {
    return deny("未知交付操作");
  }
  if (!tested("source_regression", source_revision, true)) {
    return deny("源版本需要真实、已停止的项目回归");
  }
  if (operation === "git.source.publish") return { allowed: true };
  const publication = result("source_publication");
  if (facts.source_publication?.platform_operation !== "git.source.publish"
      || publication?.branch !== source_branch || publication.revision !== source_revision) {
    return deny("候选必须来自已推送的确切源分支");
  }
  if (!["mainline", "parent_work"].includes(destination)) return deny("未知交付上级");
  if (destination === "mainline" && !result("children")) {
    return deny("正式交付需要已接纳并同步的 Task 汇合");
  }
  if (operation === "git.delivery.open") return { allowed: true };
  if (!tested("candidate_regression", candidate_revision, true)) {
    return deny("合入需要确切候选版本的真实、已停止回归");
  }
  if (destination === "mainline") {
    if (!facts.approval?.human_confirmation) return deny("正式合入需要确切候选的真实人工批准");
  } else if (!tested("green", source_revision, false)) {
    return deny("Task 自动合入需要同一源版本的独立 Green");
  }
  return { allowed: true };
}
