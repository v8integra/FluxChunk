# FluxChunk

A local-first, Git-native, self-hostable Postman alternative. No accounts, no forced cloud sync — collections are plain text files you can put in any Git repo.

Full architecture, feature phases, and design decisions: [api-client-spec.md](api-client-spec.md).

## Layout

```
crates/
  engine/   fluxchunk-engine — HTTP client, .apireq/.apicol/.apienv file format, shared core
  cli/      fluxchunk-cli (apicli bin) — thin CLI wrapper around engine
apps/
  desktop/  Tauri + SvelteKit desktop app, links engine directly
examples/
  *.apireq  sample request files
```

`crates/daemon` and `apps/vscode-extension` (Phase 3, see spec section 18) aren't scaffolded yet — added when we get there.

## Development

Rust workspace (engine + CLI):

```bash
cargo build --workspace
cargo test --workspace
cargo run --bin apicli -- examples/iss-location.apireq
```

Desktop app:

```bash
cd apps/desktop
npm install
npm run tauri dev
```

This directory is pinned to the `stable-x86_64-pc-windows-msvc` Rust toolchain via `rustup override` (Tauri on Windows expects MSVC, not GNU).
