# docs/

> Note: Technical specifications (`SPECS_JA.md`) are written in Japanese.

Development documentation for Title Protocol, organized by version.

## Structure

Each version consists of a **SPECS -> COVERAGE -> tasks** three-piece set:

```
docs/
├── v0.1.0/                <- Initial implementation (archived)
│   ├── SPECS_JA.md
│   ├── COVERAGE.md
│   └── tasks/
├── v0.1.1/                <- Incremental update (archived)
│   ├── SPECS_JA.md
│   ├── COVERAGE.md
│   └── tasks/
├── v0.1.2/                <- Full rewrite (current)
│   ├── SPECS_JA.md        <- Technical specification (written by humans)
│   ├── COVERAGE.md        <- Spec-to-implementation mapping
│   └── tasks/             <- Work units (AI development tasks + notes)
└── README.md              <- This file
```

## Data Flow

```
SPECS (what to build) -> COVERAGE (what was built) -> tasks (how to build + learnings)
```

- **SPECS**: Human-written technical specification. Defines protocol design, data structures, and security model
- **COVERAGE**: Implementation status for each spec section. Bridges specification and code
- **tasks**: AI development tasks derived from unimplemented COVERAGE items. Includes work-in-progress notes

## Versioning

- Each version represents a self-contained development phase
- v0.1.2 is a **full rewrite** — no carryover from v0.1.0/v0.1.1
- Completed versions are read-only archives
- To create a new version:
  1. Write the new spec in `docs/vN/SPECS_JA.md`
  2. Create `docs/vN/COVERAGE.md` tracking implementation status
  3. Define tasks in `docs/vN/tasks/`
  4. Update `CLAUDE.md` to reference the latest version

## Versions

| Version | Date | Content | Status |
|---------|------|---------|--------|
| [v0.1.0](./v0.1.0/) | 2026-02-21 | Initial implementation: C2PA verification, TEE, Gateway, WASM, SDK, Indexer | Archived |
| [v0.1.1](./v0.1.1/) | 2026-03-13 | Incremental: ResourcePool, host-side decode, video-vpdq, PQC-ready | Archived |
| [v0.1.2](./v0.1.2/) | 2026-05-23 | Full rewrite: Attestation-based trust, native Processors, optional E2EE, Solana Extension | **Current** |
