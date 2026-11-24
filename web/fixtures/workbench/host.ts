import { resolve } from "node:path";

export const fixtureWebDir = resolve(import.meta.dirname, "../../dist/fixture");
export const fixtureHost = resolve(
  import.meta.dirname,
  "../../../target/fixtures",
  process.env.DEVDESK_E2E_PROFILE ?? "debug",
  "devdesk-host",
);
