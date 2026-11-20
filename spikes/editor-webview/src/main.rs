//! Spike for issue #917: a tao/wry window with the Squire editor from `assets/`, loaded through a custom
//! protocol (no network). Every event the page and the window learn is one JSON line on stdout, the
//! evidence for `docs/research.md`. `--selftest` checks HTML in and out against `torro_mail::html`,
//! `--measure` times the start and sums the memory of the window and its helper processes.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use wry::http::{Request, Response, header::CONTENT_TYPE};
use wry::{DragDropEvent, WebViewBuilder};

#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Interactive,
    Selftest,
    MeasureOnce,
}

enum UserEvent {
    Ipc(Value),
    Exit(u8),
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// `torro://app/<file>` from `assets/`, `torro://fix/<file>` from `fixtures/`, `torro://img/<cid>` from
/// `fixtures/cid/<cid>.png`; a name with a slash or `..` is a 404.
fn serve(request: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let uri = request.uri();
    if std::env::var_os("EDITOR_DEBUG").is_some() {
        eprintln!("serve {uri}");
    }
    let host = uri.host().unwrap_or_default();
    let name = uri.path().trim_start_matches('/');
    let not_found = || Response::builder().status(404).body(Vec::new()).unwrap_or_default();
    if name.is_empty() || name.contains('/') || name.contains("..") || name.contains('\\') {
        return not_found();
    }
    let path = match host {
        "app" => root().join("assets").join(name),
        "fix" => root().join("fixtures").join(name),
        "img" => root().join("fixtures/cid").join(format!("{name}.png")),
        _ => return not_found(),
    };
    match std::fs::read(&path) {
        Ok(bytes) => Response::builder().header(CONTENT_TYPE, mime(&path)).body(bytes).unwrap_or_default(),
        Err(_) => not_found(),
    }
}

/// One JSON line for the evidence: milliseconds since the start, what happened.
fn emit(start: Instant, event: Value) {
    println!("{}", json!({ "t_ms": start.elapsed().as_millis() as u64, "event": event }));
}

/// Checks of the round trip: the editor's HTML, cleaned as the core would, still has what a reply needs.
fn selftest_checks(msg: &Value) -> Vec<Value> {
    let raw = msg["html"].as_str().unwrap_or_default();
    let clean = torro_mail::html::sanitize(raw);
    let mut out = Vec::new();
    let mut check = |name: &str, ok: bool, detail: String| {
        out.push(json!({ "check": name, "ok": ok, "detail": detail }));
    };
    check("editor html not empty", !raw.is_empty(), format!("{} bytes", raw.len()));
    for needle in ["torro-quote", "torro-signature", "cid:logo@torro.example", "cid:chart@torro.example"] {
        check(
            &format!("squire keeps {needle}"),
            raw.contains(needle),
            if raw.contains(needle) { "in editor html".into() } else { "lost by Squire".into() },
        );
        check(
            &format!("sanitize keeps {needle}"),
            clean.contains(needle),
            if clean.contains(needle) { "in cleaned html".into() } else { "lost by sanitize".into() },
        );
    }
    let cites = clean.matches("type=\"cite\"").count();
    check("sanitize keeps both blockquote type=cite", cites == 2, format!("{cites} found"));
    for text in ["Größe", "Übersicht", "Maßnahmen", "„Öl“", "Ältere Frage", "Hallo Jörg"] {
        check(&format!("text {text}"), clean.contains(text), String::new());
    }
    let drawn = msg["images"]
        .as_array()
        .map(|a| a.iter().filter(|i| i["width"].as_u64().unwrap_or(0) > 0).count())
        .unwrap_or(0);
    check("both cid images draw through torro://img/", drawn == 2, format!("{drawn} drew"));
    check("sanitize leaves no torro:// address", !clean.contains("torro://"), String::new());
    // For the doc: what the cleaning changed.
    check("sanitize is idempotent", torro_mail::html::sanitize(&clean) == clean, String::new());
    out
}

#[cfg(target_os = "linux")]
fn pss_kb(pid: u32) -> u64 {
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) else { return 0 };
    text.lines()
        .find(|l| l.starts_with("Pss:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// PSS in kB of `pid` and all its descendants (the WebKit helper processes).
#[cfg(target_os = "linux")]
fn tree_pss_kb(pid: u32) -> (u64, usize) {
    let mut children: std::collections::HashMap<u32, Vec<u32>> = Default::default();
    if let Ok(dir) = std::fs::read_dir("/proc") {
        for e in dir.flatten() {
            let Some(p) = e.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
            let Ok(stat) = std::fs::read_to_string(format!("/proc/{p}/stat")) else { continue };
            // `pid (comm) state ppid …`: the name may hold spaces, so cut after the last `)`.
            let Some(rest) = stat.rsplit_once(')').map(|r| r.1) else { continue };
            if let Some(ppid) = rest.split_whitespace().nth(1).and_then(|n| n.parse::<u32>().ok()) {
                children.entry(ppid).or_default().push(p);
            }
        }
    }
    let (mut total, mut count, mut todo) = (0, 0, vec![pid]);
    while let Some(p) = todo.pop() {
        total += pss_kb(p);
        count += 1;
        todo.extend(children.get(&p).cloned().unwrap_or_default());
    }
    (total, count)
}

#[cfg(not(target_os = "linux"))]
fn tree_pss_kb(_pid: u32) -> (u64, usize) {
    (0, 0)
}

/// `--measure`: five runs of `--measure-once` in own processes; median and range of start time and memory.
fn measure() -> ExitCode {
    let Ok(exe) = std::env::current_exe() else { return ExitCode::FAILURE };
    let (mut ms, mut kb) = (Vec::new(), Vec::new());
    for _ in 0..5 {
        let Ok(out) = std::process::Command::new(&exe).arg("--measure-once").output() else { return ExitCode::FAILURE };
        let line = String::from_utf8_lossy(&out.stdout);
        let Some(v) = line.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok()) else {
            eprintln!("run gave no result: {}", String::from_utf8_lossy(&out.stderr));
            return ExitCode::FAILURE;
        };
        println!("{v}");
        ms.push(v["ready_ms"].as_u64().unwrap_or(0));
        kb.push(v["pss_kb"].as_u64().unwrap_or(0));
    }
    let stat = |mut v: Vec<u64>| {
        v.sort_unstable();
        json!({ "median": v[v.len() / 2], "min": v[0], "max": v[v.len() - 1] })
    };
    println!("{}", json!({ "summary": { "ready_ms": stat(ms), "pss_kb": stat(kb) } }));
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let start = Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--measure") {
        return measure();
    }
    let mode = if args.iter().any(|a| a == "--selftest") {
        Mode::Selftest
    } else if args.iter().any(|a| a == "--measure-once") {
        Mode::MeasureOnce
    } else {
        Mode::Interactive
    };

    #[cfg(target_os = "linux")]
    if let Err(e) = gtk_init() {
        eprintln!("editor-webview: {e}");
        return ExitCode::FAILURE;
    }

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let Ok(window) = WindowBuilder::new()
        .with_title("editor-webview")
        .with_inner_size(LogicalSize::new(900.0, 700.0))
        .build(&event_loop)
    else {
        eprintln!("editor-webview: cannot open the window");
        return ExitCode::FAILURE;
    };

    let ipc_proxy = proxy.clone();
    let builder = WebViewBuilder::new()
        .with_url("torro://app/editor.html")
        .with_custom_protocol("torro".into(), |_id, request| serve(request).map(Into::into))
        .with_ipc_handler(move |req| {
            if std::env::var_os("EDITOR_DEBUG").is_some() {
                eprintln!("ipc {}", req.body());
            }
            if let Ok(v) = serde_json::from_str::<Value>(req.body()) {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(v));
            }
        })
        .with_drag_drop_handler(move |event| {
            let (kind, paths, position) = match &event {
                DragDropEvent::Enter { paths, position } => ("enter", paths.clone(), Some(*position)),
                DragDropEvent::Over { position } => ("over", Vec::new(), Some(*position)),
                DragDropEvent::Drop { paths, position } => ("drop", paths.clone(), Some(*position)),
                DragDropEvent::Leave => ("leave", Vec::new(), None),
                _ => ("other", Vec::new(), None),
            };
            // `over` fires for every mouse move; the lines of enter, drop and leave are the evidence.
            if kind != "over" {
                emit(start, json!({ "native-drop": kind, "paths": paths, "position": position.map(|p| [p.0, p.1]) }));
            }
            // false: the page gets the drop too, so the DOM side can be compared.
            false
        });

    #[cfg(target_os = "linux")]
    let built = match window.default_vbox() {
        Some(vbox) => builder.build_gtk(vbox),
        None => {
            eprintln!("editor-webview: the window has no GTK box (is this X11/Wayland through GTK?)");
            return ExitCode::FAILURE;
        }
    };
    #[cfg(not(target_os = "linux"))]
    let built = builder.build(&window);

    let webview = match built {
        Ok(w) => w,
        Err(e) => {
            eprintln!("editor-webview: cannot start the webview: {e}");
            return ExitCode::FAILURE;
        }
    };

    let reply = std::fs::read_to_string(root().join("fixtures/reply.html")).unwrap_or_default();
    let quit_proxy = proxy.clone();

    event_loop.run(move |event, _, flow| {
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *flow = ControlFlow::Exit,
            Event::UserEvent(UserEvent::Exit(code)) => {
                // `run` cannot return the code from here; end the process after the output is flushed.
                use std::io::Write;
                let _ = std::io::stdout().flush();
                std::process::exit(i32::from(code));
            }
            Event::UserEvent(UserEvent::Ipc(msg)) => {
                let kind = msg["kind"].as_str().unwrap_or_default().to_string();
                match (kind.as_str(), mode) {
                    ("ready", _) => {
                        let ready_ms = start.elapsed().as_millis() as u64;
                        emit(start, msg.clone());
                        match mode {
                            Mode::Selftest => {
                                let js = format!("window.__roundtrip({});", Value::String(reply.clone()));
                                let _ = webview.evaluate_script(&js);
                            }
                            Mode::MeasureOnce => {
                                // Let the page settle, then count.
                                let p = quit_proxy.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(Duration::from_millis(1500));
                                    let (kb, n) = tree_pss_kb(std::process::id());
                                    println!("{}", json!({ "ready_ms": ready_ms, "pss_kb": kb, "processes": n }));
                                    let _ = p.send_event(UserEvent::Exit(0));
                                });
                            }
                            Mode::Interactive => {}
                        }
                    }
                    ("roundtrip", Mode::Selftest) => {
                        let checks = selftest_checks(&msg);
                        let bad = checks.iter().filter(|c| c["ok"] != true).count();
                        for c in &checks {
                            println!("{c}");
                        }
                        println!(
                            "{}",
                            json!({ "selftest": if bad == 0 { "ok" } else { "deviations" }, "deviations": bad })
                        );
                        let _ = quit_proxy.send_event(UserEvent::Exit(u8::from(bad != 0)));
                    }
                    ("paste", _) => {
                        emit(start, msg.clone());
                        if let Some(html) = msg["fragment"].as_str() {
                            emit(start, json!({ "paste-sanitized": torro_mail::html::sanitize(html) }));
                        }
                    }
                    ("input", _) => {} // too chatty; `__text` and the paste lines are enough
                    _ => emit(start, msg.clone()),
                }
            }
            _ => {}
        }
    });
}

#[cfg(target_os = "linux")]
fn gtk_init() -> Result<(), String> {
    // tao initialises GTK itself when the event loop is built; nothing to do beyond a clear error when no
    // display is there.
    if std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_none() {
        return Err("no WAYLAND_DISPLAY or DISPLAY".into());
    }
    Ok(())
}
