# V2 → V3 Migration Matrix

Source repository: `jingyuansrobin/campus-reconstruction-tool`
Target repository: `jingyuansrobin/campus-rebuild-table`

The V2 repository is a reference and source of proven implementation techniques. It is not copied wholesale into V3.

## Decision rule

A V2 capability is migrated only when it directly serves the V3.0 journey or removes a proven implementation risk.

“Already written” is not sufficient justification.

## Matrix

| V2 capability | V3 decision | Notes |
|---|---|---|
| AMap/high-school campus search and map embedding | KEEP / SELECTIVE MIGRATION | Core entry path. Reuse proven integration ideas and code where boundaries remain clean. |
| Custom polygon campus boundary | KEEP / PRIORITY MIGRATION | Strong product value and required V3.0 input. |
| Custom orientation | KEEP / SIMPLIFY | Optional setting; no separate heavy workflow. |
| Project/scheme management | REWRITE | Replace V2 Scheme semantics with `CampusProject`. |
| SQLite persistence | REUSE EXPERIENCE, NOT MODEL | V3 uses readable local project files as primary truth; SQLite remains optional infrastructure later. |
| OSM candidate acquisition pipeline | DROP FROM V3.0 | Arnis owns base geographic generation. |
| Candidate confidence/review workbench | DROP | Conflicts with the V3.0 “generate first” journey. |
| Data-transformer category pipeline | DROP UNLESS NEEDED BY MIGRATED MAP FEATURE | Do not preserve V2 pipeline for its own sake. |
| Geometry validation | SELECTIVE REUSE | Keep only boundary/geometry safety logic that is actually needed by V3 inputs. |
| Custom generation-engine | REPLACE WITH ARNIS | Stop investing in competing base generation. |
| Sponge `.schem` writer | DEFER / SELECTIVE REUSE | Reuse only if Arnis output or later asset workflows require it. |
| Three.js/3D preview | KEEP PRODUCT CAPABILITY | Reuse proven preview implementation if it remains cheaper than rewriting. |
| Manifest/material export | DEFER | Not part of first V3.0 vertical slice. |
| Notification/error patterns | KEEP PRINCIPLES | Do not automatically migrate a dedicated module. |
| Localization system | DEFER / SIMPLIFY | Keep architecture compatible, but Chinese-first V3.0 should not be blocked by full i18n work. |
| Onboarding tutorial | DROP FROM V3.0 | Main journey should be self-explanatory first. |
| Coverage audit | DROP | Not core to V3 thesis. |
| 30-crate modular governance | DROP AS-IS | V3 starts with a small workspace; split only when real ownership/change boundaries justify it. |
| Rust fmt/clippy/tests/CI | KEEP, SIMPLIFY | Preserve quality gates without recreating V2 governance overhead. |
| ADR/product-baseline/agent workflow discipline | KEEP METHOD | New V3 docs should describe current V3 decisions; do not bulk-copy historical ADRs. |

## First migration candidates

Do not migrate them before the first headless project slice is green.

After the V3 core exists, inspect these V2 areas in this order:

1. AMap campus search / map integration.
2. Polygon boundary drawing and validation.
3. 3D preview.
4. Optional orientation behavior.
5. Only then evaluate any supporting persistence/export utilities.

## What must not enter the first migration PR

- V2 candidate review UI.
- V2 generation engine.
- V2 coverage audit.
- V2 onboarding system.
- full V2 dependency-enforcement machinery.
- historical ADRs copied without an active V3 decision.

## Migration style

Preferred:

```text
understand V2 behavior
→ identify smallest proven unit
→ port behind a V3 interface
→ add V3-focused test
→ delete V2-specific concepts from the port
```

Avoid:

```text
copy directory
→ fix compiler errors
→ preserve old model accidentally
```
