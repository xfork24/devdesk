# Managed protocol regressions

`dsh-fast-completion.json` is a minimized trace from the real DSH SDK `0.2.2-rc.1` acceptance session. The provider emitted inbox consumption before the prompt admission response, then completed within one coordinator drain. Assistant content is replaced with a harmless marker; no credential or authentication frames are retained.

`harness::protocol_tests::dsh_fast_completion_replays_pre_admission_consumption_before_output` reads the fixture and verifies consumption, output, and the separate `session/wait` quiescence proof. The same test exercises the bounded native execution deadline without waiting fifteen minutes.

`dsh-agent-context.mjs` exercises the official DSH SDK, tool registry and Host-facing JSON-RPC callbacks with loopback-only mock inference. The Docker regression selects an immutable image explicitly and uses `--network=none`; it proves two context calls and final output, separately from real supplier inference.

`pi-turn.json` retains the Pi 1.6.0 RPC shapes for prompt admission, tool dialogs, streamed text, the final message and settlement. The adapter fixture also injects unmanaged and unexpected approval dialogs to verify cancellation.
