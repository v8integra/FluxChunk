//! Minimal early slice of `apicli` (spec section 14): send a single
//! `.apireq` file, optionally against an `.apienv` environment, and print
//! the result. Collection/folder runs, iteration data, JUnit/JSON
//! reporters, and assertion evaluation land later — this proves the
//! engine's parse -> resolve (vars, then vault) -> send path end to end.

use std::path::{Path, PathBuf};

use fluxchunk_engine::format::{ApiRequestFile, EnvironmentFile, VaultFile};
use fluxchunk_engine::http::{build_outgoing_request, HttpClient};
use fluxchunk_engine::vars::{interpolate, merge_scopes, resolve_vault};
use indexmap::IndexMap;

struct Args {
    request_path: PathBuf,
    env_path: Option<PathBuf>,
}

fn parse_args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut request_path = None;
    let mut env_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--env" => {
                env_path = Some(PathBuf::from(args.next().unwrap_or_else(|| {
                    eprintln!("error: --env requires a path to an .apienv file");
                    std::process::exit(2);
                })));
            }
            other => request_path = Some(PathBuf::from(other)),
        }
    }

    let Some(request_path) = request_path else {
        eprintln!("usage: apicli <path-to.apireq> [--env <path-to.apienv>]");
        std::process::exit(2);
    };

    Args { request_path, env_path }
}

/// Loads `<env_path>` and its sibling `<env_path>.vault`, if present. A
/// missing vault file is not an error — plenty of environments have no
/// secrets — but a present-and-unreadable one is, since that's more likely
/// a permissions problem than an absent file.
fn load_env(env_path: &Path) -> (IndexMap<String, String>, IndexMap<String, String>) {
    let source = std::fs::read_to_string(env_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {e}", env_path.display());
        std::process::exit(2);
    });
    let env = EnvironmentFile::parse(&source).unwrap_or_else(|e| {
        eprintln!("error: couldn't parse {}: {e}", env_path.display());
        std::process::exit(2);
    });

    let vault_path = PathBuf::from(format!("{}.vault", env_path.display()));
    let vault = match std::fs::read_to_string(&vault_path) {
        Ok(source) => VaultFile::parse(&source).secrets,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => IndexMap::new(),
        Err(e) => {
            eprintln!("error: couldn't read {}: {e}", vault_path.display());
            std::process::exit(2);
        }
    };

    (env.vars, vault)
}

#[tokio::main]
async fn main() {
    let args = parse_args();

    let source = std::fs::read_to_string(&args.request_path).unwrap_or_else(|e| {
        eprintln!("error: couldn't read {}: {e}", args.request_path.display());
        std::process::exit(2);
    });
    let file = ApiRequestFile::parse(&source).unwrap_or_else(|e| {
        eprintln!("error: couldn't parse {}: {e}", args.request_path.display());
        std::process::exit(2);
    });

    let (env_vars, vault) = match &args.env_path {
        Some(path) => load_env(path),
        None => (IndexMap::new(), IndexMap::new()),
    };
    // Only one scope exists yet (the environment); global/collection-scoped
    // vars merge in ahead of it here once .apicol parsing lands.
    let vars = merge_scopes(&[&env_vars]);

    // Stage 1 only (vars, no vault) -- safe to print. The vault stage runs
    // separately, right before the request goes out, and its output is
    // never echoed: printing a resolved secret to the terminal would
    // defeat the point of keeping it out of Git as surely as committing it
    // would (spec section 9/16).
    let visible_url = interpolate(&file.url, &vars);
    let visible_headers: IndexMap<String, String> =
        file.headers.iter().map(|(k, v)| (k.clone(), interpolate(v, &vars))).collect();

    print!("{} {}", file.method.to_uppercase(), visible_url);
    if !matches!(file.auth, fluxchunk_engine::format::Auth::None) {
        print!(" (auth: {})", file.auth.mode_str());
    }
    println!();

    let send_url = resolve_vault(&visible_url, &vault);
    let send_headers: IndexMap<String, String> =
        visible_headers.iter().map(|(k, v)| (k.clone(), resolve_vault(v, &vault))).collect();
    let resolved_auth = file.auth.resolve(&vars, &vault);

    let resolved_body = file
        .body
        .as_ref()
        .map(|b| resolve_vault(&interpolate(b.content(), &vars), &vault));

    let outgoing = build_outgoing_request(&file, send_url, send_headers, resolved_auth, resolved_body).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(2);
    });

    let client = HttpClient::new();
    match client.send(outgoing).await {
        Ok(resp) => {
            println!("status: {} ({} ms)", resp.status, resp.elapsed_ms);
            println!("{}", resp.body_as_text());
        }
        Err(e) => {
            eprintln!("error: request failed: {e}");
            std::process::exit(2);
        }
    }
}
