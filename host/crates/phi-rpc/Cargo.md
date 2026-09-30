# phi-rpc crate manifest

No dependencies: the crate was built for the card target with
`build-std` as well as for the host until the agent became assembly
(2026-09-29), and stays that plain. One dev-dependency, `phi-isa-audit`:
`tests/agent.rs` audits the agent binary it builds. See `src/lib.md`.
