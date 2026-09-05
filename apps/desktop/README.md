# Desktop shell

`mcrebuild-desktop` is the V3 native Windows shell. It is intentionally thin: project loading, boundary validation and persistence remain in `app-core` / `campus-core`.

## Current v0.3b role

The current executable is a vertical-slice development entry rather than the final application homepage:

```text
mcrebuild-desktop <project_dir>
```

It loads the project's boundary editing context through `app-core`, renders the high-level boundary editor through `gaode-map`, receives a small IPC event set, and sends submitted WGS-84 vertices back to `app-core` for validation and atomic persistence.

Runtime configuration on Windows:

```powershell
$env:AMAP_JS_KEY="<your-js-api-key>"
$env:AMAP_JS_SECURITY_CODE="<your-security-code>"
cargo run -p mcrebuild-desktop -- .\path\to\campus-project
```

The project must have a geographic anchor. Projects created from `gaode-search` have one; the retained manual CLI initialization path does not invent an anchor and therefore cannot open the map editor.

## Why V3 does not migrate the V2 desktop implementation

V2 combined Slint with an embedded Wry child WebView and accumulated raw-window-handle / Windows child-window focus complexity. V3 currently uses one native winit window with one Wry WebView instead.

This is not a commitment that every future MCRebuild screen must be HTML. It is the smallest native desktop shell that proves the current product flow while the headless core remains replaceable and independently testable.

## Boundary rules

The WebView is presentation and map interaction only. It must not:

- parse or rewrite CampusProject JSON directly;
- define geographic truth in GCJ-02;
- perform final polygon validity checks;
- create project IDs;
- call Arnis directly.

Those responsibilities remain behind Rust application/domain boundaries.
