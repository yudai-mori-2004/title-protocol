# docs/

> Note: Technical specifications (`SPECS_JA.md`) are written in Japanese.

Development documentation for Title Protocol, organized by version.

## Structure

Each version consists of a **SPECS → COVERAGE → tasks** three-piece set:

```
docs/
├── v0.1.0/                ← Initial implementation (2026-02-21)
│   ├── SPECS_JA.md        ← Technical specification (written by humans)
│   ├── COVERAGE.md        ← Spec-to-implementation mapping
│   └── tasks/             ← Work units (AI development tasks + notes)
├── v0.1.1/                ← Next iteration
│   ├── SPECS_JA.md
│   ├── COVERAGE.md
│   └── tasks/
└── README.md              ← This file
```

## Data Flow

```
SPECS (what to build) → COVERAGE (what was built) → tasks (how to build + learnings)
```

- **SPECS**: Human-written technical specification. Defines protocol design, data structures, and security model
- **COVERAGE**: Implementation status for each spec section. Bridges specification and code
- **tasks**: AI development tasks derived from unimplemented COVERAGE items. Includes work-in-progress notes

## Versioning

- Each version represents a self-contained development phase
- COVERAGE is **cumulative**: vN assumes v(N-1) is complete and tracks only specs added in vN
- To create a new version:
  1. Write the new spec in `docs/vN/SPECS_JA.md`
  2. Create `docs/vN/COVERAGE.md` tracking the delta from v(N-1)
  3. Define tasks in `docs/vN/tasks/`
  4. Update `CLAUDE.md` to reference the latest version

## Versions

| Version | Date | Content | Status |
|---------|------|---------|--------|
| [v0.1.0](./v0.1.0/) | 2026-02-21 | Initial implementation: C2PA verification, TEE, Gateway, WASM, SDK, Indexer (tasks 01–13) | **Complete** |
| [v0.1.1](./v0.1.1/) | 2026-03-13 | Documentation restructure (Diataxis), collection authority atomization, deploy script fixes | In progress |
