//! Minimal early slice of `apicli` (spec section 14): send a single
//! `.apireq` file and print the result. Collection/folder runs, iteration
//! data, JUnit/JSON reporters, and assertion evaluation land later —
//! this just proves the engine crate's parse -> interpolate -> send path
//! works end to end.

use std::path::PathBuf;

use fluxchunk_engine::format::ApiRequestFile;
use fluxchunk_engine::http::{build_outgoing_request, HttpClient};
use fluxchunk_engine::vars::interpolate;
use indexmap::IndexMap;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: apicli <path-to.apireq>");
        std::process::exit(2);
    };

    let path = PathBuf::from(path);
    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: couldn't read {}: {e}", path.display());
            std::process::exit(2);
        }
    };

    let file = match ApiRequestFile::parse(&source) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: couldn't parse {}: {e}", path.display());
            std::process::exit(2);
        }
    };

    // No .apienv loading yet -- empty var set, so unresolved {{vars}}
    // are left visible rather than silently blanked.
    let vars = IndexMap::new();
    let resolved_url = interpolate(&file.url, &vars);
    let resolved_headers: IndexMap<String, String> = file
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), interpolate(v, &vars)))
        .collect();

    let outgoing = match build_outgoing_request(&file, resolved_url, resolved_headers) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };

    println!("{} {}", file.method.to_uppercase(), outgoing.url);

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
