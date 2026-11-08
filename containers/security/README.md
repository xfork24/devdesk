# Nested harness sandbox profile

`moby-default.json` and `LICENSE.moby` come from [moby/profiles](https://github.com/moby/profiles/tree/4b8214fc4971e06172938a1f1554ccff6647ee80), commit `4b8214fc4971e06172938a1f1554ccff6647ee80` (Apache-2.0).

`nested-sandbox.json` retains Docker's default deny action and syscall rules, except for allowing `clone`, `clone3`, `unshare`, `mount`, `umount2`, `pivot_root` and `setns`. These support the harness's unprivileged nested namespace sandbox. They do not grant kernel capabilities or access to the host namespace. Containers still run as a non-root user with `cap-drop=ALL`, `no-new-privileges`, a read-only root filesystem, explicit project mounts and resource limits. The policy is coordinator-owned and is never supplied by the agent.

The default Docker profile blocked Codex bubblewrap in the tested Docker Desktop Linux VM. `tests/live/probe_nested_sandbox.py` verifies the adjusted profile with a credential canary: the actual Codex sandbox can modify `/work` and cannot read `/state/codex/auth.json`. The probe does not use model credentials or call a model. Results are in `docs/reports/real-integration/R0-nested-sandbox.json`.

Native Linux hosts with additional AppArmor/SELinux restrictions require their own compatibility verification; this report does not assert those hosts have been tested. No privileged mode, host PID/network namespace, or administrator bypass is used.
