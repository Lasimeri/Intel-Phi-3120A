# 0004: QuickJS is the JavaScript runtime

Status: accepted, 2026-09-13.

## Context

Bun's own installation documentation states that CPUs without SSE4.2 are
not supported even by the `x64-baseline` build, and that Bun crashes with
"illegal instruction" on them. Bun is JavaScriptCore (a JIT that emits SSE)
plus Zig (LLVM x86-64 baseline, which assumes SSE2). KNC has none of that and
no compiler can be taught the KNC vector ISA in this project's lifetime.

## Decision

QuickJS (or its maintained fork QuickJS-ng) is the JavaScript runtime on the
card: pure C, interpreter only, ES2023, small.

## Consequences

- `bun` commands do not exist on the card. `qjs` does.
- Node-compatible APIs are not provided.

## Alternatives rejected

- Bun: impossible (SSE4.2, JIT).
- Node.js: V8 requires SSE2.
- Duktape, mujs: viable, smaller, older language level than QuickJS.
