# Nested harness sandbox profile

`moby-default.json` and `LICENSE.moby` come from [moby/profiles](https://github.com/moby/profiles/tree/4b8214fc4971e06172938a1f1554ccff6647ee80), commit `4b8214fc4971e06172938a1f1554ccff6647ee80` (Apache-2.0).

`nested-sandbox.json` retains Docker's default deny action and syscall rules, except for allowing `clone`, `clone3`, `unshare`, `mount`, `umount2`, `pivot_root` and `setns`. These support the harness's unprivileged nested namespace sandbox. They do not grant kernel capabilities or access to the host namespace. Containers still run as a non-root user with `cap-drop=ALL`, `no-new-privileges`, a read-only root filesystem, explicit project mounts and resource limits. The policy is coordinator-owned and is never supplied by the agent.

The default Docker profile blocked Codex bubblewrap in the tested Docker Desktop Linux VM. `tests/live/probe_nested_sandbox.py` verifies the adjusted profile with a credential canary: the actual Codex sandbox can modify `/work` and cannot read `/state/codex/auth.json`. The probe does not use model credentials or call a model. Results are in `docs/plans/real-engineering-integration/R0-nested-sandbox.json`.

Native Linux hosts with additional AppArmor/SELinux restrictions require their own compatibility verification; this report does not assert those hosts have been tested. No privileged mode, host PID/network namespace, or administrator bypass is used.

## Ubuntu AppArmor compatibility

On Ubuntu the `docker-default` AppArmor profile rejects bubblewrap's initial
`mount(..., MS_SLAVE|MS_REC)` even with the nested seccomp profile. Authentication
and model-only discussion do not exercise this boundary.

`devdesk-harness-v1.apparmor` is a dedicated profile derived from Moby's AppArmor
v0.3.0 template. It retains the network-family, proc/sys, signal and ptrace
restrictions and permits only bubblewrap's root propagation, temporary root,
bind/remount, tmpfs/proc/devpts and pivot operations at the documented setup
paths. Other mount operations stay denied. It changes the mount policy compared
with `docker-default`; it is not an identical permission set and requires an
explicitly reviewed administrator installation. It does not grant outer
CAP_SYS_ADMIN, privileged mode, host namespaces, writable root or daemon access.

```bash
./scripts/install-linux-sandbox-policy.sh --check
# After reviewing/authorizing the named profile, on the Docker host:
sudo ./scripts/install-linux-sandbox-policy.sh --install
```

The installer compiles before loading, installs just `/etc/apparmor.d/devdesk-harness-v1`,
and does not restart Docker, replace `docker-default`, modify sysctls or affect
other container profiles. On AppArmor-enabled daemons, nested Harness creation
requires the matching root-owned profile file and freezes its content digest
into container identity. Missing/drifted policy is an explicit error; there is
no unconfined fallback. Credential-free test containers retain the default
profile. The full live resource tests must pass before claiming Ubuntu
compatibility. Parser compilation alone is not runtime validation. Actual Codex credential-canary, Codex/DSH directory/state/proc boundaries and rejection of an unlisted tmpfs mount passed on Ubuntu 24.04.5 with the user-approved named profile; see `docs/plans/ubuntu-web-acceptance/ubuntu-20261108T095147Z/resume-20261108T102305Z/`.
