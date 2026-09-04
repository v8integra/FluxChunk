//! The pre-request / post-response scripting sandbox (spec section 9):
//! rquickjs (QuickJS), no JIT, a `bru` / `console` / `req` / `res` surface,
//! and a hard security boundary.
//!
//! **Vault secrets are structurally impossible for a script to read.**
//! Scripts only ever see `vars` and a `req` that have already gone through
//! [`crate::vars::interpolate`] (stage 1) -- never [`crate::vars::resolve_vault`]
//! (stage 2), which callers must run *after* a pre-request script has
//! finished (see that module's docs). A script can reference `{{vault:...}}`
//! as a literal string and leave it for the engine to resolve at send
//! time, but the real secret value never enters the JS heap.
//!
//! Each execution gets a brand-new `Runtime` + `Context` (no state leakage
//! between runs), a memory ceiling, and a wall-clock timeout enforced via
//! QuickJS's interrupt handler (checked periodically during execution,
//! not just once per call).
//!
//! Not exposed to scripts, by design: the filesystem, arbitrary network
//! sockets, and any Rust internals beyond the surface below. `bru.sendRequest`
//! (request chaining) isn't implemented -- it needs to bridge an async
//! network call into QuickJS's synchronous single-threaded execution
//! model, which is real complexity deserving its own pass rather than a
//! half-working version bolted on here.
//!
//! Execution is deliberately not `async`: it's synchronous CPU-bound work
//! bounded by `ScriptLimits::timeout`. A caller on a single-request path
//! (apicli, one desktop send) can call these functions directly; a caller
//! sending many requests concurrently should run them via
//! `tokio::task::spawn_blocking` instead of parking a whole async worker
//! thread per script.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use rquickjs::function::Rest;
use rquickjs::{Context, Ctx, Function, Null, Object, Runtime, Value};

use crate::error::EngineError;

#[derive(Debug, Clone, Copy)]
pub struct ScriptLimits {
    pub timeout: Duration,
    pub memory_limit_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        ScriptLimits {
            timeout: Duration::from_secs(5), // spec section 9 default
            memory_limit_bytes: 32 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

impl ConsoleLevel {
    fn js_name(self) -> &'static str {
        match self {
            ConsoleLevel::Log => "log",
            ConsoleLevel::Info => "info",
            ConsoleLevel::Warn => "warn",
            ConsoleLevel::Error => "error",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsoleEntry {
    pub level: ConsoleLevel,
    pub message: String,
}

/// The request as scripts see/modify it -- always the vars-interpolated
/// (stage 1) form. See the module docs' security boundary note.
#[derive(Debug, Clone)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: IndexMap<String, String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ScriptResponse {
    pub status: u16,
    pub headers: IndexMap<String, String>,
    pub body: String,
}

#[derive(Debug)]
pub struct PreRequestOutcome {
    pub vars: IndexMap<String, String>,
    pub request: ScriptRequest,
    pub console: Vec<ConsoleEntry>,
}

#[derive(Debug)]
pub struct PostResponseOutcome {
    pub vars: IndexMap<String, String>,
    pub console: Vec<ConsoleEntry>,
}

pub fn run_pre_request(
    script_source: &str,
    vars: &IndexMap<String, String>,
    request: &ScriptRequest,
    limits: &ScriptLimits,
) -> Result<PreRequestOutcome, EngineError> {
    let (_runtime, context) = new_sandbox(limits)?;
    let vars_state = Rc::new(RefCell::new(vars.clone()));
    let console_state = Rc::new(RefCell::new(Vec::new()));

    // The closure returns an owned `ScriptRequest` rather than the `Object<'js>`
    // it built along the way -- `ScriptRequest` isn't tied to the `'js`
    // lifetime, so it's free to outlive `with()`, unlike the JS object itself.
    let request = context.with(|ctx| -> Result<ScriptRequest, EngineError> {
        install_bru(&ctx, &vars_state)?;
        install_console(&ctx, &console_state)?;
        let req_obj = build_req_object(&ctx, request)?;
        ctx.globals().set("req", req_obj.clone())?;
        eval_script(&ctx, script_source)?;
        read_req_object(&ctx, &req_obj)
    })?;

    let vars = vars_state.borrow().clone();
    let console = console_state.borrow().clone();
    Ok(PreRequestOutcome { vars, request, console })
}

pub fn run_post_response(
    script_source: &str,
    vars: &IndexMap<String, String>,
    response: &ScriptResponse,
    limits: &ScriptLimits,
) -> Result<PostResponseOutcome, EngineError> {
    let (_runtime, context) = new_sandbox(limits)?;
    let vars_state = Rc::new(RefCell::new(vars.clone()));
    let console_state = Rc::new(RefCell::new(Vec::new()));

    context.with(|ctx| -> Result<(), EngineError> {
        install_bru(&ctx, &vars_state)?;
        install_console(&ctx, &console_state)?;
        let res_obj = build_res_object(&ctx, response)?;
        ctx.globals().set("res", res_obj)?;
        eval_script(&ctx, script_source)
    })?;

    let vars = vars_state.borrow().clone();
    let console = console_state.borrow().clone();
    Ok(PostResponseOutcome { vars, console })
}

fn new_sandbox(limits: &ScriptLimits) -> Result<(Runtime, Context), EngineError> {
    let runtime = Runtime::new().map_err(EngineError::from)?;
    if limits.memory_limit_bytes > 0 {
        runtime.set_memory_limit(limits.memory_limit_bytes);
    }

    let deadline = Instant::now() + limits.timeout;
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let context = Context::full(&runtime).map_err(EngineError::from)?;
    Ok((runtime, context))
}

fn eval_script(ctx: &Ctx<'_>, source: &str) -> Result<(), EngineError> {
    match ctx.eval::<(), _>(source) {
        Ok(()) => Ok(()),
        Err(rquickjs::Error::Exception) => {
            let exception = ctx.catch();
            Err(EngineError::Script(format_js_error(ctx, &exception)))
        }
        Err(e) => Err(EngineError::from(e)),
    }
}

/// `"<message>\n<stack>"` when the thrown value is an Error-like object
/// with a non-empty `stack` -- QuickJS populates it with `file:line:col`
/// frames for both thrown exceptions and syntax errors, which is what
/// spec section 16's "Script errors: surfaced in Console panel with
/// line numbers" needs; `stringify_js_value` alone only has `.message`.
fn format_js_error<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> String {
    let message = stringify_js_value(ctx, value);
    if let Some(obj) = value.as_object() {
        if let Ok(stack) = obj.get::<_, String>("stack") {
            if !stack.trim().is_empty() {
                return format!("{message}\n{}", stack.trim_end());
            }
        }
    }
    message
}

fn install_bru<'js>(ctx: &Ctx<'js>, vars: &Rc<RefCell<IndexMap<String, String>>>) -> Result<(), EngineError> {
    let bru = Object::new(ctx.clone())?;

    let get_vars = vars.clone();
    let get_var = Function::new(ctx.clone(), move |name: String| -> Option<String> { get_vars.borrow().get(&name).cloned() })?;
    bru.set("getVar", get_var)?;

    let set_vars = vars.clone();
    // `value` deliberately isn't typed `String` -- vars regularly get set
    // from `res.body.<field>`, which is whatever JSON produced (number,
    // bool, object...), not always already a string. `call_ctx` is
    // injected per-call the same way `install_console`'s is -- see that
    // function's comment for why capturing `ctx.clone()` here instead
    // would crash the process.
    let set_var = Function::new(ctx.clone(), move |call_ctx: Ctx<'js>, name: String, value: Value<'js>| {
        set_vars.borrow_mut().insert(name, stringify_js_value(&call_ctx, &value));
    })?;
    bru.set("setVar", set_var)?;

    ctx.globals().set("bru", bru)?;
    Ok(())
}

fn install_console<'js>(ctx: &Ctx<'js>, log: &Rc<RefCell<Vec<ConsoleEntry>>>) -> Result<(), EngineError> {
    let console = Object::new(ctx.clone())?;
    for level in [ConsoleLevel::Log, ConsoleLevel::Info, ConsoleLevel::Warn, ConsoleLevel::Error] {
        let log_state = log.clone();
        // `call_ctx` is a parameter rquickjs injects fresh on each call,
        // not a captured clone -- capturing `ctx.clone()` into a closure
        // that's itself stored as a JS function in this same Context
        // creates a reference cycle QuickJS's GC can't unwind, which
        // crashes the whole process on teardown (`Assertion
        // 'list_empty(&rt->gc_obj_list)' failed`) instead of erroring.
        let f = Function::new(ctx.clone(), move |call_ctx: Ctx<'js>, args: Rest<Value<'js>>| {
            let message = args.iter().map(|v| stringify_js_value(&call_ctx, v)).collect::<Vec<_>>().join(" ");
            log_state.borrow_mut().push(ConsoleEntry { level, message });
        })?;
        console.set(level.js_name(), f)?;
    }
    ctx.globals().set("console", console)?;
    Ok(())
}

fn build_req_object<'js>(ctx: &Ctx<'js>, request: &ScriptRequest) -> Result<Object<'js>, EngineError> {
    let req = Object::new(ctx.clone())?;
    req.set("method", request.method.clone())?;
    req.set("url", request.url.clone())?;

    let headers = Object::new(ctx.clone())?;
    for (k, v) in &request.headers {
        headers.set(k.as_str(), v.clone())?;
    }
    req.set("headers", headers)?;

    match &request.body {
        Some(b) => req.set("body", b.clone())?,
        None => req.set("body", Null)?,
    }

    Ok(req)
}

fn read_req_object<'js>(ctx: &Ctx<'js>, req: &Object<'js>) -> Result<ScriptRequest, EngineError> {
    let method: String = req.get("method")?;
    let url: String = req.get("url")?;

    let headers_obj: Object<'js> = req.get("headers")?;
    let mut headers = IndexMap::new();
    for prop in headers_obj.props::<String, Value<'js>>() {
        let (k, v) = prop?;
        headers.insert(k, stringify_js_value(ctx, &v));
    }

    let body_val: Value<'js> = req.get("body")?;
    let body = if body_val.is_undefined() || body_val.is_null() {
        None
    } else {
        Some(stringify_js_value(ctx, &body_val))
    };

    Ok(ScriptRequest { method, url, headers, body })
}

fn build_res_object<'js>(ctx: &Ctx<'js>, response: &ScriptResponse) -> Result<Object<'js>, EngineError> {
    let res = Object::new(ctx.clone())?;
    res.set("status", response.status as i32)?;

    let headers = Object::new(ctx.clone())?;
    for (k, v) in &response.headers {
        headers.set(k.as_str(), v.clone())?;
    }
    res.set("headers", headers)?;

    // Matches the spec's own example usage (`res.body.id`): a JSON body
    // is exposed as a real JS value, not a string the script has to
    // re-parse itself. Anything that doesn't parse falls back to the raw
    // text untouched.
    match ctx.json_parse(response.body.as_str()) {
        Ok(parsed) => res.set("body", parsed)?,
        Err(_) => res.set("body", response.body.clone())?,
    }

    Ok(res)
}

/// Best-effort stringification for `console.*` args and JS exception
/// values. Primitives are formatted directly; anything else goes through
/// `JSON.stringify`, falling back to a placeholder if even that fails
/// (e.g. a value containing a BigInt or a cyclic reference).
fn stringify_js_value<'js>(ctx: &Ctx<'js>, value: &Value<'js>) -> String {
    if value.is_null() {
        return "null".to_string();
    }
    if value.is_undefined() {
        return "undefined".to_string();
    }
    if let Some(s) = value.as_string() {
        return s.to_string().unwrap_or_default();
    }
    if let Some(b) = value.as_bool() {
        return b.to_string();
    }
    if let Some(n) = value.as_int() {
        return n.to_string();
    }
    if let Some(n) = value.as_float() {
        return n.to_string();
    }
    // `Error` instances (thrown exceptions in particular) keep `message`
    // as a non-enumerable own property, so `JSON.stringify(error)` gives
    // `{}` -- useless for an error message. A direct `.get()` bypasses
    // enumerability and picks this up for any Error, without needing to
    // detect the exact Error subclass.
    if let Some(obj) = value.as_object() {
        if let Ok(message) = obj.get::<_, String>("message") {
            if !message.is_empty() {
                return message;
            }
        }
    }
    match ctx.json_stringify(value.clone()) {
        Ok(Some(s)) => s.to_string().unwrap_or_default(),
        _ => "[unprintable value]".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn req(url: &str) -> ScriptRequest {
        ScriptRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            headers: IndexMap::new(),
            body: None,
        }
    }

    #[test]
    fn bru_get_and_set_var_round_trip() {
        let outcome = run_pre_request(
            r#"bru.setVar("request_time", "123"); bru.setVar("echo", bru.getVar("existing"));"#,
            &vars(&[("existing", "hello")]),
            &req("https://example.com"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.vars.get("request_time").unwrap(), "123");
        assert_eq!(outcome.vars.get("echo").unwrap(), "hello");
        // The var that was already there and untouched survives too.
        assert_eq!(outcome.vars.get("existing").unwrap(), "hello");
    }

    #[test]
    fn pre_request_script_can_modify_url_and_headers() {
        let outcome = run_pre_request(
            r#"
            req.url = req.url + "?traced=1";
            req.headers["X-Trace-Id"] = "abc123";
            "#,
            &IndexMap::new(),
            &req("https://example.com/users"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.request.url, "https://example.com/users?traced=1");
        assert_eq!(outcome.request.headers.get("X-Trace-Id").unwrap(), "abc123");
    }

    #[test]
    fn pre_request_script_can_clear_body() {
        let mut r = req("https://example.com");
        r.body = Some("original".to_string());
        let outcome = run_pre_request("req.body = null;", &IndexMap::new(), &r, &ScriptLimits::default()).unwrap();
        assert_eq!(outcome.request.body, None);
    }

    #[test]
    fn vault_placeholder_reaches_script_unresolved_and_unresolvable() {
        // This is the security-boundary test: a var whose value is a
        // {{vault:...}} reference is visible to the script only as that
        // literal string. There is no vault map anywhere in this
        // function's signature or the sandbox's global surface for a
        // script to read the real secret from -- the type system, not
        // just convention, is what makes exfiltration impossible here.
        let outcome = run_pre_request(
            r#"bru.setVar("seen", bru.getVar("api_key"));"#,
            &vars(&[("api_key", "{{vault:api_key}}")]),
            &req("https://example.com"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.vars.get("seen").unwrap(), "{{vault:api_key}}");
    }

    #[test]
    fn post_response_reads_json_body_and_sets_var_from_spec_example() {
        // Mirrors api-client-spec.md section 4's own example script.
        let outcome = run_post_response(
            r#"
            if (res.status === 200) {
              bru.setVar("last_user_id", res.body.id);
            }
            "#,
            &IndexMap::new(),
            &ScriptResponse {
                status: 200,
                headers: IndexMap::new(),
                body: r#"{"id": 42, "name": "Ada"}"#.to_string(),
            },
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.vars.get("last_user_id").unwrap(), "42");
    }

    #[test]
    fn post_response_non_json_body_stays_a_string() {
        let outcome = run_post_response(
            r#"bru.setVar("type", typeof res.body);"#,
            &IndexMap::new(),
            &ScriptResponse {
                status: 200,
                headers: IndexMap::new(),
                body: "not json".to_string(),
            },
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.vars.get("type").unwrap(), "string");
    }

    #[test]
    fn console_log_captures_mixed_argument_types() {
        let outcome = run_pre_request(
            r#"console.log("status:", 42, true, {a: 1});"#,
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.console.len(), 1);
        assert_eq!(outcome.console[0].level, ConsoleLevel::Log);
        assert_eq!(outcome.console[0].message, r#"status: 42 true {"a":1}"#);
    }

    #[test]
    fn console_warn_and_error_are_tagged_separately() {
        let outcome = run_pre_request(
            r#"console.warn("careful"); console.error("boom");"#,
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(outcome.console[0].level, ConsoleLevel::Warn);
        assert_eq!(outcome.console[1].level, ConsoleLevel::Error);
    }

    #[test]
    fn syntax_error_returns_script_error_not_panic() {
        let result = run_pre_request("this is not valid js {{{", &IndexMap::new(), &req("https://example.com"), &ScriptLimits::default());
        assert!(matches!(result, Err(EngineError::Script(_))));
    }

    #[test]
    fn thrown_exception_returns_script_error_with_message() {
        let result = run_pre_request(
            r#"throw new Error("custom failure");"#,
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        );
        let Err(EngineError::Script(message)) = result else {
            panic!("expected a Script error, got {result:?}");
        };
        assert!(message.contains("custom failure"), "message was: {message}");
    }

    #[test]
    fn thrown_exception_message_includes_line_number() {
        // Spec section 16: "Script errors: surfaced in Console panel
        // with line numbers" -- the stack QuickJS attaches to thrown
        // Errors carries `file:line:col`, appended after the message.
        let result = run_pre_request(
            "function boom() {\n  throw new Error(\"custom failure\");\n}\nboom();",
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        );
        let Err(EngineError::Script(message)) = result else {
            panic!("expected a Script error, got {result:?}");
        };
        assert!(message.contains(":2:"), "expected a line-2 reference in: {message}");
    }

    #[test]
    fn infinite_loop_is_interrupted_by_timeout() {
        let limits = ScriptLimits {
            timeout: Duration::from_millis(50),
            ..ScriptLimits::default()
        };
        let result = run_pre_request("while (true) {}", &IndexMap::new(), &req("https://example.com"), &limits);
        assert!(result.is_err());
    }

    #[test]
    fn excessive_allocation_hits_memory_limit() {
        let limits = ScriptLimits {
            memory_limit_bytes: 64 * 1024,
            ..ScriptLimits::default()
        };
        let result = run_pre_request(
            r#"
            let s = [];
            while (true) { s.push("x".repeat(1000)); }
            "#,
            &IndexMap::new(),
            &req("https://example.com"),
            &limits,
        );
        assert!(result.is_err());
    }

    #[test]
    fn no_state_leaks_between_separate_runs() {
        // Each run gets a brand-new Runtime + Context -- a global set in
        // one run must not be visible in the next.
        let first = run_pre_request(
            "globalThis.leaked = 'yes';",
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        );
        assert!(first.is_ok());

        let second = run_pre_request(
            r#"bru.setVar("saw_leak", typeof globalThis.leaked);"#,
            &IndexMap::new(),
            &req("https://example.com"),
            &ScriptLimits::default(),
        )
        .unwrap();
        assert_eq!(second.vars.get("saw_leak").unwrap(), "undefined");
    }
}
