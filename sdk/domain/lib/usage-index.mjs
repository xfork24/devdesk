// The reverse-reference index of a configuration bundle: for every published spec, skill,
// tool and delivery type, where it is used. This is the one implementation; the kernel's
// catalog checks compare their stored index with it, and a package's build writes it.
export function usageIndex(sources) {
  const index = {};
  for (const source of sources)
    if (["spec", "skill", "tool", "delivery_type"].includes(source.type))
      index[`${source.identity}:${source.revision}`] = [];

  // Text order by code unit, which is how the kernel orders the rows it compares with.
  const later = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
  const entries = (object) => Object.keys(object ?? {}).sort(later);
  const add = (source, target, path, kind = "reference", stage, step, port) => {
    const row = {
      reference: `${source.identity}:${source.revision}`,
      name: source.name,
      type: source.type,
      version: source.revision,
      scope: source.scope,
      path,
      kind,
    };
    for (const [key, value] of [
      ["stage", stage],
      ["step", step],
      ["port", port],
    ])
      if (value !== undefined && value !== null) row[key] = value;
    (index[target] ??= []).push(row);
  };

  for (const source of sources) {
    if (source.type === "capability" || source.type === "delivery_type") {
      const fields =
        source.type === "capability" ? ["spec_refs", "skills", "tools"] : ["spec_refs"];
      for (const field of fields)
        (source[field] ?? []).forEach((target, i) => add(source, target, `/${field}/${i}`));
    } else if (source.type === "workflow") {
      const data = source.data;
      if (data.orchestrator_role_ref)
        add(source, data.orchestrator_role_ref, "/data/orchestrator_role_ref");
      const inputs = data.inputs ?? {};
      const steps = Object.fromEntries((data.steps ?? []).map((step) => [step.key, step]));
      const stages = Object.fromEntries(
        (data.stages ?? []).map((stage) => [stage.key, { key: stage.key, name: stage.name }]),
      );
      // What a binding to a step port delivers: a workflow input, or a step's output.
      const delivery = (ref) =>
        (ref.step === "$input" ? inputs : steps[ref.step]?.outputs ?? {})[ref.port]
          ?.delivery_ref;
      for (const name of entries(inputs))
        add(
          source,
          inputs[name].delivery_ref,
          `/data/inputs/${name}/delivery_ref`,
          "workflow_input",
          undefined,
          undefined,
          name,
        );
      (data.steps ?? []).forEach((step, i) => {
        const stage = stages[step.stage];
        const node = { key: step.key, name: step.name };
        (step.spec_refs ?? []).forEach((spec, j) =>
          add(source, spec, `/data/steps/${i}/spec_refs/${j}`, "step_spec", stage, node),
        );
        for (const name of entries(step.outputs))
          add(
            source,
            step.outputs[name].delivery_ref,
            `/data/steps/${i}/outputs/${name}/delivery_ref`,
            "step_output",
            stage,
            node,
            name,
          );
        for (const name of entries(step.inputs)) {
          const binding = step.inputs[name];
          if (binding.type === "source") {
            const target = delivery(binding.source);
            if (target)
              add(
                source,
                target,
                `/data/steps/${i}/inputs/${name}`,
                "step_input",
                stage,
                node,
                name,
              );
          }
        }
        const executor = step.executor;
        if (executor.type === "tool") {
          add(
            source,
            executor.tool_ref,
            `/data/steps/${i}/executor/tool_ref`,
            "step_tool",
            stage,
            node,
          );
          for (const name of entries(executor.arguments)) {
            const binding = executor.arguments[name];
            if (binding.type === "source") {
              const target = delivery(binding.source);
              if (target)
                add(
                  source,
                  target,
                  `/data/steps/${i}/executor/arguments/${name}`,
                  "step_argument",
                  stage,
                  node,
                  name,
                );
            }
          }
        } else if (executor.type === "workflow" && executor.foreach) {
          const target = delivery(executor.foreach.source);
          if (target)
            add(
              source,
              target,
              `/data/steps/${i}/executor/foreach/source`,
              "step_collection",
              stage,
              node,
            );
        }
      });
      for (const name of entries(data.outputs)) {
        const target = delivery(data.outputs[name]);
        if (target)
          add(
            source,
            target,
            `/data/outputs/${name}`,
            "workflow_output",
            undefined,
            undefined,
            name,
          );
      }
    }
  }
  return Object.fromEntries(
    entries(index).map((key) => [
      key,
      index[key].sort(
        (a, b) =>
          later(a.type, b.type) || later(a.reference, b.reference) || later(a.path, b.path),
      ),
    ]),
  );
}
