import { RevisionSelect } from "../../web/src/kit/configuration-ui";

// This file is checked by tsconfig.test.json, never executed as a browser spec.
function selectionTypes() {
  RevisionSelect({
    label: "single",
    records: [],
    value: "revision:1",
    onChange(value) {
      const selected: string = value;
      void selected;
    },
  });
  RevisionSelect({
    label: "multiple",
    records: [],
    multiple: true,
    value: ["revision:1"],
    onChange(value) {
      const selected: string[] = value;
      void selected;
    },
  });
  // @ts-expect-error Multiple selection cannot receive a scalar value.
  RevisionSelect({
    label: "invalid",
    records: [],
    multiple: true,
    value: "revision:1",
    onChange() {},
  });
  // @ts-expect-error Single selection cannot receive an array callback.
  RevisionSelect({
    label: "invalid",
    records: [],
    value: "revision:1",
    onChange: (_value: string[]) => {},
  });
}
void selectionTypes;
