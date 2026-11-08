# Managed protocol regressions

`dsh-fast-completion.json` is a minimized trace from the real DSH SDK `0.2.2-rc.1` acceptance session. The provider emitted inbox consumption before the prompt admission response, then completed within one coordinator drain. Assistant content is replaced with a harmless marker; no credential or authentication frames are retained.

`harness::protocol_tests::dsh_fast_completion_replays_pre_admission_consumption_before_output` reads the fixture and verifies consumption, output, and the separate `session/wait` quiescence proof. The same test exercises the bounded native execution deadline without waiting fifteen minutes.
