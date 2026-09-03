# API Client — Technical Specification & Build Plan

*A local-first, Git-native, self-hostable Postman alternative. This document compiles all architectural decisions made during planning and is intended as the primary reference for implementation.*

---

## 1. Vision

A desktop API client (REST, GraphQL, and eventually gRPC/WebSocket/SSE) that starts as a clean, fast, familiar Postman-style clone and layers on differentiated features that address well-known gaps in existing tools: forced cloud accounts, gutted free-tier collaboration, proprietary storage formats, and bloated/slow clients.

**Core positioning, in one line:** everything runs locally and for free by default; anyone who needs more control (companies, power users) can point the app at their own infrastructure instead of ours. No account system exists anywhere in this product.

---

## 2. Core Principles (apply to every feature below)

1. **Local-first, no sign-in.** The app works fully offline out of the box. No account creation, no login, ever.
2. **Zero server cost to the maintainer, regardless of user count.** No feature should require us to run a backend that scales with adoption.
3. **Git-native storage.** Collections are plain text files, diffable and mergeable, that can live in any Git repo.
4. **Bring-your-own-infrastructure pattern.** Repeated throughout: collaboration relay, AI provider, heuristics feed source, and update manifest URL are all overridable to point at infrastructure the user or their company controls instead of ours.
5. **Explicit consent over convenience.** Nothing — updates, crash reports, AI network calls — happens silently or automatically without a clear opt-in and, in most cases, per-instance approval.
6. **Performance is a first-class requirement, not an afterthought.** Every architectural choice (Tauri over Electron, Svelte over React, virtualized rendering, Rust core) is made with this as a constraint.

---

## 3. Tech Stack

### Core engine (Rust) — shared source of truth for all platforms
- **tokio** — async runtime
- **reqwest** (on hyper) — HTTP client
- **rustls** — TLS, pure Rust, no C toolchain dependency
- **tonic** + **prost** — gRPC (Phase 2); note: requires runtime/dynamic proto handling since users paste arbitrary `.proto` files with no precompiled bindings — treat as its own R&D spike
- **tokio-tungstenite** — WebSocket (Phase 2)
- **rquickjs** (QuickJS bindings) — scripting sandbox
- **git2-rs** (libgit2 bindings) — real Git operations on collection files
- **webrtc-rs** — P2P data channels
- **yrs** (Yjs port) — CRDT layer for live collaboration merging
- **llama.cpp bindings** — local AI inference (GGUF quantized models); candle (pure-Rust) is a worthwhile future migration to drop the C++ dependency
- **interprocess** — local named pipe / Unix socket transport for the daemon

### Desktop UI
- **Tauri** (not Electron) — lightweight, native webview, no bundled Chromium/Node runtime
- **Svelte** — compiles away at build time, smaller runtime footprint than React
- **Tailwind** — utility CSS, no component library dependency to maintain

### Workspace layout (single Cargo workspace)
```
apiapp/
  crates/
    engine/          <- protocols, storage, sync, AI runtime — shared core, does all real work
    daemon/           <- thin JSON-RPC wrapper around engine, for VS Code (Phase 3)
    cli/              <- thin CLI wrapper around engine, for CI/CD
  apps/
    desktop/          <- Tauri app (Rust + Svelte), links engine directly
    vscode-extension/ <- TypeScript, talks to daemon over local socket (Phase 3)
```

### VS Code bridge (Phase 3)
JSON-RPC 2.0 over a local named pipe (Windows). Avoids native Node addon compilation/versioning fragility. Also unlocks a CLI/CI runner for free, since it's a thin wrapper around the same daemon protocol.

---

## 4. Data Model / File Format

Custom lightweight labeled-block format (`.apireq`), inspired by Bruno's `.bru` — not JSON/YAML, because raw unescaped script/body blocks keep diffs clean and files human-readable. One request per file.

```
# users/get-user-by-id.apireq

meta {
  name: Get user by ID
  type: http
  seq: 3
}

get {
  url: {{base_url}}/users/:id
}

params:path {
  id: {{user_id}}
}

headers {
  Authorization: Bearer {{access_token}}
  Accept: application/json
}

body:json {
  {
    "include": ["profile", "roles"]
  }
}

script:pre-request {
  bru.setVar("request_time", Date.now());
}

script:post-response {
  if (res.status === 200) {
    bru.setVar("last_user_id", res.body.id);
  }
}

assert {
  res.status: eq 200
  res.body.id: isDefined
}
```

Key design points:
- **`seq` field for ordering**, not filename numbering — reordering doesn't rename files, keeping Git history clean.
- **`type` field** (`http`, `graphql`, later `grpc`/`ws`/`sse`) drives protocol-specific dispatch from one file format.
- **Raw, unescaped blocks** — a JSON body is literal JSON, a script is literal JS. No re-serialization, minimal diffs.
- **Block-level structure enables the CRDT/merge system** — sync can diff/merge at the block boundary (headers vs body vs script), so non-overlapping edits merge automatically.
- **Binary bodies are referenced by relative path**, never inlined as base64.

### Folder structure
```
my-api-collection/
  collection.apicol            <- collection-level manifest (base URL, default auth, format version)
  environments/
    local.apienv
    staging.apienv
    production.apienv
  auth/
    login.apireq
  users/
    get-user-by-id.apireq
    create-user.apireq
    users.folder                 <- optional folder-level settings
  .apiworkspace                  <- workspace/team config (approver keys, relay URL)
```

### Secrets: split from structure
```
# local.apienv  — committed, shared with team
vars {
  base_url: https://api.local.dev
  api_key: {{vault:api_key}}
}
```
```
# local.apienv.vault  — gitignored, machine-local only, never synced or exported
api_key: sk-live-abc123...
```
`{{vault:...}}` resolves only inside the Rust engine, only at send time — see Section 9 (Scripting Sandbox security boundary).

---

## 5. Feature List by Phase

### Phase 1 — MVP core clone
- Local-first storage, Git-native, no sign-in, works offline
- REST (all methods, query params, headers, body types: raw/JSON/XML/text/form-data/urlencoded/binary)
- GraphQL (query editor, variables, schema introspection)
- Auth helpers: Basic, Bearer, API Key, OAuth2
- Response viewer: status/timing/size, pretty/raw views, response history
- Collections, folders, environments (global/collection/environment-scoped variables), `{{variable}}` interpolation
- Masked/secret vault variables
- Pre-request and post-response scripting (QuickJS sandbox)
- Import: Postman collections, OpenAPI specs
- Code generation from requests (curl, fetch, etc.)
- Multi-tab interface, command palette, dark mode, fast startup
- Workspace shell: theme selector, layout presets + custom, panel show/hide
- Response search, response history, side-by-side response comparison (structural diff)

### Phase 2 — Collaboration & protocol expansion
- P2P collaboration: LAN discovery (mDNS), self-hosted signaling relay (one-click deploy), community public relay option
- CRDT-based live merge for simultaneous editing
- Git-commit-based async merge for offline reconnects, with PR-style conflict review (approver roles via local keypair, signed approvals)
- Additional protocols: gRPC, WebSocket, SSE
- Import security scanning (fixed heuristics) + auto-updating heuristics feed (AI-assisted monitoring of external public sources, translated into local rule format, never auto-applied)

### Phase 3 — AI, VS Code, CI
- Local AI: hardware detection → gated model picker → optional bring-your-own external AI (single provider for v1)
- Local daemon + VS Code extension (thin client over JSON-RPC)
- CLI test runner (`apicli`) for CI/CD, including `--live` mode against a collaboration workspace

---

## 6. Collaboration System

### Three tiers, none hosted by the maintainer
1. **Same network** — mDNS/LAN discovery, zero server involved
2. **Self-hosted relay** — company/team runs a small open-source signaling container (Docker one-click deploy to Railway/Fly/Render); relay only exchanges brief WebRTC handshake messages, never touches actual data
3. **Community public relay (optional)** — free/opt-in, not maintainer-run at scale

### Room code mechanics
Code encodes relay address + session ID + secret (e.g. `wss://acme-corp.example.com/room/7X9K2Q`). Entering it auto-configures the connection — no manual server entry step.

### Sync layers
- **Live (both online):** CRDT (Yjs/yrs) — automatic, field-level merge, no UI needed for non-conflicting edits; deterministic tie-break for true simultaneous same-field edits
- **Async (offline reconnect):** Git-commit-style merge (isomorphic-git-equivalent via git2-rs) — non-overlapping changes merge silently; genuine overlaps get flagged for review
- **Deletions:** "edits win over deletes" rule — a deleted-but-being-edited item is restored with a note, never silently lost

### Conflict review (PR-style)
- Each user has a local keypair (no account) generated on first install
- `.apiworkspace` lists approver public keys, editable by existing approvers
- Approvals are signed — full audit trail without a central authority
- Review screen: side-by-side diff, options = accept A / accept B / hand-edit / reject both
- No approver present → item stays blocked and flagged, no silent timeout fallback

---

## 7. Local AI

- **Hardware-aware model picker**: on first use, scan hardware, present all model tiers, gray out ones that won't run well with a red explanation of why
- **Runtime**: llama.cpp bindings, GGUF quantized models, downloaded on demand (not bundled in installer)
- **Bring-your-own external AI**: optional, single provider for v1 (e.g. Claude), user-supplied API key, never required
- **Use cases**: explaining failed requests/errors (ties into error handling UI), and — later — monitoring/translating external security heuristic sources (see Section 9)
- **Never used to invent security rules from scratch** — only to monitor, extract, and translate patterns from established external sources; all changes require human review before applying

---

## 8. Import & Security Scanning

### Two-dialog flow
1. **Import summary** — what's being imported (counts of environments/collections/requests/scripts), Cancel or Scan & Continue
2. **Security findings** (only shown if issues found) — severity-tagged (Critical/Warning), code snippet, plain-language explanation, per finding. Options: Reject Import / Import & Skip Flagged Scripts / Import Anyway (styled as a deliberate override, not a default action)

### Detection approach
- **Fixed, small heuristic rule set for MVP** — not AI-generated, not infinitely maintained; a handful of high-signal patterns: `eval()`/`Function()` use, encoded-string decode-and-execute patterns, network calls to hosts outside the collection's known environments
- **Reused for P2P sync trust**: same component flags scripts from an unfamiliar peer on first sync, not just at import

### Heuristics feed updates (Phase 2)
- Rules come from established external security sources (community rule sets, security research), never invented by the local AI
- AI's role: check configured sources for changes, extract/translate patterns into the app's internal rule format (useful specifically because sources publish in inconsistent/unstructured formats)
- **Never auto-applied** — every check (scheduled monthly or manual) ends in a changelog-style review dialog requiring explicit approval
- **Configurable source list** — default public sources, or a company-internal source (for classified/NDA'd pattern sets), same bring-your-own-infra pattern as collaboration relay
- Feed integrity: HTTPS + signature/checksum verification
- Complex extraction tasks may benefit from routing to external AI if configured, given higher accuracy needs than everyday debugging

---

## 9. Scripting Sandbox

- **Engine**: rquickjs (QuickJS) — no JIT, predictable performance, small footprint, proven for this exact use case (same class of engine Bruno uses)
- **Exposed API** (`bru.*` convention): variable read/write, request read/modify (pre-request), response read (post-response), `console.*` → routed to Console panel, controlled `sendRequest()` for chaining (via engine's own HTTP client, not raw sockets)
- **Not exposed**: filesystem, arbitrary network sockets, Rust internals beyond the `bru` surface
- **Critical security boundary: vault secrets are never readable by scripts.** `{{vault:...}}` resolves only inside the Rust engine, only at actual send time, after scripts have finished running. A script can reference a secret symbolically and trigger its use, but can never read or exfiltrate the real value.
- **Resource limits**: default 5s execution timeout (configurable per-request), memory ceiling via QuickJS runtime limits, fresh isolated context per execution (no state leakage between runs)
- **`assert` block** (declarative, from file format) handles simple assertions without invoking the sandbox at all; free-form `script:post-response` reserved for genuinely custom logic — faster and reduces sandboxed-execution surface for the common case

---

## 10. UI / UX Design

### Layout philosophy
Postman-familiar structure throughout, so switching users feel immediately at home: sidebar collection tree, tabbed request area, request builder above / response viewer below (or beside, depending on layout preset).

### Theme selector (top-right)
- 7 options as vertical "DIP switches": Light, Dark, Blue, Green, Red, Pink, Silver
- Radio-button behavior visualized as physical toggles — selecting one flips it on and all others off
- Silver treated as its own cool-neutral token set, not a derived variant of Light
- Persisted locally (no account needed)

### Layout system
- **3 fixed presets** — Split (sidebar + side-by-side request/response, closest to Postman default), Stacked (request over response, good for narrow screens), Focus (sidebar collapsed, full-width single panel)
- **1 Custom slot** — defaults to mirroring Split until modified
- **Drag-and-drop** panel rearrangement is a distinct interaction from presets; dragging a panel transforms the "Custom" button into a "Save" button (with a small unsaved-state indicator dot)
- **Cancel control**: a separated `×` button next to Save (~16px gap — enough to avoid accidental clicks, close enough to read as related), reverts to the layout active before the drag started
- **Panels are independently show/hide-able** (Params, Headers, Body, Auth, Tests, Console, etc.) via chip toggles; hiding never deletes data, only visibility state

### Request panel tabs
Params · Headers · Body (sub-types: None/JSON/raw/form-data/urlencoded/GraphQL/binary) · Auth (None/Inherit/Basic/Bearer/API Key/OAuth2) · Scripts (Pre-request/Post-response/Tests sub-tabs) · Settings (per-request timeout, redirect behavior, SSL verification)

Browser-style tab strip above for multiple open requests, with unsaved-change dot indicators.

### Response panel tabs
Pretty (virtualized, collapsible, syntax-highlighted tree) · Raw · Preview (sandboxed iframe for HTML) · Headers · Cookies · Test Results

**Performance requirements specific to this panel:**
- Large bodies (>5MB threshold) parsed in the Rust engine, not the webview — ship a lightweight tree structure to the frontend, not a raw multi-MB string
- Virtualized rendering — only visible tree nodes exist as DOM elements
- Explicit "Load full response" action past the size threshold rather than silently attempting to render everything
- Binary/non-text responses get type-appropriate handling (image preview, PDF preview, "save to file" fallback)
- Streaming progress indicator for large downloads

### Response search
Ctrl+F over the parsed structure (not raw text rescan) — match count, next/prev nav, auto-expands collapsed branches to reveal matches, searches both keys and values.

### Response history & comparison
- Stored in a local SQLite DB, **outside** Git-native collection storage (per-user, ephemeral, may contain sensitive response data — never synced or committed)
- Retention: last N runs per request (default ~20, configurable), manual clear option
- **Structural diff** (not text diff) between any two history entries, or the live response vs. any history entry — recursive, walks nested objects/arrays, color-coded like a Git diff (green=added, red=removed, amber=changed)

---

## 11. Settings & Configuration Architecture

Three tiers, each with different scope/sync behavior:

1. **Global/user settings** — theme, layout state, update-check preference, AI model choice (not credentials), keyboard shortcuts. Local TOML file at `%APPDATA%\YourAppName\config.toml`. Never synced.
2. **Workspace settings** (`.apiworkspace`) — relay URL, approver public keys, team heuristics feed override. Synced via Git, shared with the team.
3. **Vault secrets** (`.apienv.vault`) — machine-local only, gitignored, never synced or exported, ever.

Local history/cache database sits separately from all three tiers.

### Export/Import Settings
- Exports tier-1 (global/user) settings only, as plain TOML/JSON — chosen specifically because plain text survives corporate "no binary attachment" email/upload filters and looks unambiguous to security scanning
- **Never includes**: vault secrets, AI API keys/credentials
- **Does include**: which AI provider is configured by name (e.g. "Claude") for reference purposes — identity only, never the credential
- Use case: recovering personal preferences after a company-issued machine is wiped/reassigned

---

## 12. First-Run Experience

- **Auto-launches on first install.** Persistent "? Take the tour" control in the toolbar to relaunch anytime.
- **Live, functional demo** — not static screenshots. Uses a real, free, keyless, HTTPS space/science API (`wheretheiss.at` recommended over the inaccurate/HTTP-only Open Notify "ISS location" API — verify current endpoint status at build time). NASA APOD (requires a key) can be left as a bonus request in the starter collection rather than the tour centerpiece.
- **~5 steps**: welcome → send a real request → read the response → where collections live → where theme/layout customization lives
- **Skip available at every step**, progress shown as dots, Back/Next navigation
- **"What's New" re-launch**: only after updates explicitly flagged `workflow_change: true` in the release manifest — a deliberate maintainer decision per release, not inferred from version numbers. Reuses the same tour component with different step content.

---

## 13. Update Mechanism (App Self-Update)

- **Tauri's built-in updater plugin** — checks a JSON manifest, downloads a signed binary, verifies signature before applying
- **Hosting**: GitHub Releases (free, no server) for both installer binaries and the `latest.json` manifest
- **Update signing**: minisign-style keypair, verified client-side before any update is applied — separate from Windows code-signing
- **Strictly opt-in**: automatic checks OFF by default (per explicit decision — no forced-update behavior). Manual "Check for Updates" button always available regardless of the automatic toggle.
- **Every update requires explicit approval** — no silent download or install. Flow: Check → "Update available" dialog with changelog → Approve & Download → separate "Install and Restart" action. Rejecting just skips; can check again anytime.
- **Enterprise override**: the check URL itself is configurable, so IT can point it at an internally hosted manifest instead of the public GitHub feed — same mechanism serves both "give me control over updates" and "keep this air-gapped from the public internet," without needing separate on/off logic per installer type.

### Code signing (deferred, post-beta)
Standard Windows code-signing cert needed to avoid SmartScreen warnings — budget roughly $200–500/year depending on CA (EV certs cost more but build SmartScreen reputation faster). Beta releases ship unsigned; beta instructions must clearly explain the expected SmartScreen prompt ("More info → Run anyway") so testers aren't alarmed. Steps to actually obtain and integrate a cert to be worked out closer to release.

### Donations (deferred)
GitHub Sponsors or Ko-fi — free to set up, no infrastructure required, just a link in the app pointing to a maintainer-hosted page. To be set up later.

---

## 14. Testing / CI Runner (`apicli`)

```
apicli run ./my-collection --env staging
apicli run ./my-collection/users --env staging --iteration-data users.csv
```

- Runs a full collection, a folder, or a single request; data-driven runs via CSV/JSON row iteration
- **Secrets in CI**: resolved from environment variables at runtime (`APICLI_VAULT_<name>`), not from a vault file — maps directly onto how every CI system already injects secrets
- **Output**: console (human-readable, default) · `--reporter junit` (XML, for native CI test-result integration) · `--reporter json` (structured, for custom tooling)
- **Exit codes**: `0` all passed · `1` ran fine, assertions failed · `2` runner itself failed (network/auth/invalid collection) — kept distinct so CI can differentiate "your code regressed" from "the runner couldn't connect"
- **Collection source**:
  - **Default**: whatever's checked out from Git at that commit — reproducible builds, no dependency on live collaboration state
  - **Opt-in `--live` mode**: `apicli run ./collection --live --relay <url> --room <code>` — briefly joins as a read-only peer, pulls a consistent CRDT snapshot of current in-progress team state, runs against that, disconnects. No new permission tier required (room code access is already sufficient to join normally); document this access model clearly.
- **Distribution**: same GitHub Release as desktop app; optional npm wrapper package (`npx apicli`) for CI environments that already have Node available

---

## 15. Packaging & Distribution

### Windows installers — both offered upfront
- **NSIS** (`.exe`) — default, front-and-center download for individual users. Lighter to build/maintain; no meaningful runtime performance difference vs. MSI (installer format only affects install-time behavior, not the running app).
- **MSI** (via WiX Toolset) — labeled clearly as "Enterprise MSI." Needed for Intune/SCCM-style fleet deployment, transactional install/rollback, standardized silent install (`msiexec /i app.msi /quiet`).
- Both built from the same underlying app in the same CI pipeline — offering both upfront doubles as a trust/readiness signal to enterprise evaluators, even for those who end up choosing NSIS.
- MSI installs use the **same update system** as NSIS (no special-cased default) — IT controls update cadence entirely via the existing configurable manifest URL override, not via a different toggle default.

### Release pipeline (GitHub Actions, triggered on version tag push)
1. Checkout, run tests
2. Build on `windows-latest`
3. Bundle NSIS + MSI
4. Sign update payload (minisign keypair)
5. Sign installers with code-signing cert (once obtained, post-beta)
6. Generate/update `latest.json`
7. Publish installers + manifest + SHA256 checksums to a GitHub Release
8. Beta releases marked as GitHub "pre-release," not full release

### CLI & future VS Code extension
- CLI binary attaches to the same GitHub Release; optional npm wrapper
- VS Code extension (Phase 3) published to VS Code Marketplace (+ optionally Open VSX) under a free publisher account — no infra cost

---

## 16. Logging, Error Handling & Crash Reporting

### Logging
- Local only, `%APPDATA%\YourApp\logs\`
- Levels: error/warn/info/debug; default is info/warn, not debug
- Rotation: cap by age/size (e.g. 7 days or 50MB), auto-purge older
- **Vault secrets and full request/response bodies are never logged by default** — only method/host/status/timing at normal levels
- Explicit "Verbose logging" toggle for bodies, with a one-time warning before enabling

### In-app error handling
- Failed requests: inline explanation in the response panel (DNS failure, timeout, TLS error, etc. — categorized, not a generic error badge)
- Script errors: surfaced in Console panel with line numbers
- Local AI debugging assistant hooks directly into this same surface — explaining failures is its primary practical use case

### Crash reporting — local-first, GitHub Issues as the report destination, zero backend
1. Rust panic hook catches crashes, writes a **redacted, app/system-level-only** local crash log before exit (app version, OS, stack trace, general action context like "crashed during layout switch") — **explicitly excludes anything about the specific request in flight**: no URLs, headers, bodies, variables, or response data, since request-level issues are almost always environment-specific to the user and not actionable by the maintainer anyway
2. Next launch: calm "app closed unexpectedly" notice with a "View details" option — never alarming
3. Optional "Report this issue" opens the browser to a **pre-filled GitHub new-issue URL** (title/body populated via query params) — no API, no token, no backend; person reviews and edits before submitting on GitHub's own page
4. Nothing is ever sent automatically; a "copy report" fallback exists for people without/unwilling to use GitHub
5. Daemon (Phase 3) uses the same independent panic-hook + local-log approach; VS Code extension offers to restart it on detecting a lost connection

---

## 17. Open Items / Decisions Deferred Intentionally

These were explicitly flagged during planning as "revisit later," not oversights:
- Windows code-signing certificate acquisition (post-beta)
- Donation platform setup (GitHub Sponsors / Ko-fi)
- Multiple simultaneous external AI providers (v1 supports exactly one)
- MSI/WiX build implementation details
- Saved/named custom layout presets beyond the single Custom slot (only if users ask for it post-launch)
- macOS/Linux builds (blocked on maintainer obtaining that hardware)

---

## 18. Suggested Build Order

1. Rust engine core: HTTP client, REST request/response cycle, Git-native file read/write for the `.apireq`/`.apicol`/`.apienv` format
2. Tauri + Svelte desktop shell: basic request/response UI, no styling polish yet
3. Environments, variables, vault split, auth helpers
4. Scripting sandbox (rquickjs) + secrets boundary
5. Collections/folders, multi-tab UI, Postman/OpenAPI import (without security scanning yet)
6. Workspace shell: theme selector, layout presets, panel management
7. Response panel: virtualized tree, search, history, structural diff
8. Import security scanning (fixed heuristics)
9. First-run tour
10. Update mechanism + packaging pipeline (NSIS first, MSI later)
11. Logging + crash reporting
12. **Phase 1 complete — testable MVP**
13. P2P collaboration (LAN → self-hosted relay → conflict review)
14. Additional protocols (gRPC, WebSocket, SSE)
15. Heuristics feed auto-updates
16. Local AI (model picker → debugging assistant → external AI bring-your-own)
17. Daemon + VS Code extension
18. CLI/CI runner
