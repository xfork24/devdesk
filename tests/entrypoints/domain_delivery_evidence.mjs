import assert from "node:assert/strict";
import { test } from "node:test";
import { authorizeDelivery } from "../../plugins/software-development/runtime/worker.mjs";

const source = "a".repeat(40);
const candidate = "b".repeat(40);
const branch = "devdesk/features/example";
const success = (revision, operation = "tests.project") => ({
  platform_operation: operation,
  result: { candidate: revision, status: "succeeded", expected_failure: false, writer_revoked: true },
});

function feature() {
  return {
    operation: "git.delivery.merge",
    source_revision: source,
    source_branch: branch,
    candidate_revision: candidate,
    destination: "mainline",
    regression_configured: true,
    facts: {
      review: { workspace_revision: source, result: { decision: "pass" } },
      source_regression: success(source),
      source_publication: {
        platform_operation: "git.source.publish",
        result: { branch, revision: source },
      },
      children: { result: [{ synchronized: true }] },
      candidate_regression: success(candidate),
      approval: { human_confirmation: "persisted-confirmation" },
    },
  };
}

test("mainline authorization requires acceptance, exact revisions and Human approval", () => {
  assert.deepEqual(authorizeDelivery(feature()), { allowed: true });
  for (const change of [
    ({ facts }) => { facts.review.workspace_revision = candidate; },
    ({ facts }) => { facts.source_publication.result.branch = "another-branch"; },
    ({ facts }) => { facts.candidate_regression.result.candidate = source; },
    ({ facts }) => { facts.candidate_regression.result.writer_revoked = false; },
    ({ facts }) => { facts.approval = { result: { approved: true } }; },
    ({ facts }) => { delete facts.children; },
  ]) {
    const input = feature();
    change(input);
    assert.equal(authorizeDelivery(input).allowed, false);
  }
});

test("Task authorization requires independent Green for the same source revision", () => {
  const input = feature();
  input.destination = "parent_work";
  delete input.facts.approval;
  delete input.facts.children;
  assert.equal(authorizeDelivery(input).allowed, false);
  input.facts.green = success(candidate, "tests.run");
  assert.equal(authorizeDelivery(input).allowed, false);
  input.facts.green = success(source, "tests.run");
  assert.equal(authorizeDelivery(input).allowed, true);
  input.facts.green.result.skipped = true;
  assert.equal(authorizeDelivery(input).allowed, false);
});

test("regression may be skipped only when that project's regression is unconfigured", () => {
  const input = feature();
  Object.assign(input.facts.source_regression.result, {
    skipped: true, reason: "no_regression_configured",
  });
  assert.equal(authorizeDelivery(input).allowed, false);
  input.regression_configured = false;
  assert.equal(authorizeDelivery(input).allowed, true);
  input.facts.source_regression.result.reason = "unavailable";
  assert.equal(authorizeDelivery(input).allowed, false);
});
