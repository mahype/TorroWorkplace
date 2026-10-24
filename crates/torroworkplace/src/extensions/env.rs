//! What an MCP server gets from the core (task MA.3): the environment of its process, the headers of its
//! requests, and whether its address may be used at all.
//!
//! A server is a program with the person's rights, so it gets as little of the service as possible: its
//! process starts with an empty environment (`env_clear`, task MA.4) and then exactly [`child_env`]: a few
//! basics from the service (never `TORRO_*` or an API key the service happens to have), the `PATH` the core
//! sets, then the server's own variables. A name that would let a server change which code a program loads
//! is refused ([`blocked_env`]). An address is `https://`, or `http://` only on this computer
//! ([`check_url`]): a token in a header must not travel in clear text over the network.
//!
//! The service's environment and the folders are parameters, so tests run with their own.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use torro_core::i18n::{tr, trf};
use torro_ipc::Secret;
use torro_ipc::extensions::{ServerSpec, Value, blocked_env, valid_env_name, valid_header_name};

use super::prereq::{Found, Roots};

/// The variables of the service every server's process gets, when the service has them: where the person's
/// files are, who they are, their language and the folder for temporary files.
pub const BASICS: &[&str] = &["HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "TMPDIR"];

/// On Windows also these: without them many programs (Node.js, Python, Docker) do not start.
pub const WINDOWS_BASICS: &[&str] =
    &["SystemRoot", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP", "PATHEXT", "ComSpec"];

/// Why a server cannot be set up or started as given. `Display` says it for people, in the suite's language
/// (`message` of `error`, and the line of the form in the control centre); it names a variable or header,
/// never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvError {
    /// The variable `name` may not be given to a server ([`blocked_env`]).
    Blocked { name: String },
    /// `name` is no valid name of a variable ([`valid_env_name`]) or header ([`valid_header_name`]).
    InvalidName { name: String },
    /// The value of `name` has a character a variable or header cannot carry (NUL; for a header a line
    /// break or another control character).
    InvalidValue { name: String },
    /// The secret of `name` is not in the keychain.
    MissingSecret { name: String },
    /// The command is empty or a relative path: a server starts a program by name or by absolute path.
    BadCommand,
    /// The address is no URL with a host.
    BadUrl,
    /// The address is neither `https://` nor `http://` on this computer.
    NotSecure,
    /// The address has a user or a password in it, which would be stored in clear text.
    CredentialsInUrl,
    /// A program asked for the headers of an address or the other way round: a mistake of the caller.
    WrongTransport,
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            EnvError::Blocked { name } => trf(
                "“{name}” can't be given to an MCP server: it could change which code the program loads. Remove it.",
                &[("name", name)],
            ),
            EnvError::InvalidName { name } => {
                trf("“{name}” isn't a valid name for a variable or header.", &[("name", name)])
            }
            EnvError::InvalidValue { name } => {
                trf("The value of “{name}” has characters that aren't allowed.", &[("name", name)])
            }
            EnvError::MissingSecret { name } => {
                trf("The secret “{name}” is missing. Enter it again.", &[("name", name)])
            }
            EnvError::BadCommand => tr("Enter the program by name (like npx) or by its full path.").into(),
            EnvError::BadUrl => tr("This isn't a valid address. Enter one like https://example.com/mcp.").into(),
            EnvError::NotSecure => {
                tr("The address must start with https://. Only this computer (localhost) may use http://.").into()
            }
            EnvError::CredentialsInUrl => {
                tr("Remove the user name and password from the address and add them as a secret header.").into()
            }
            EnvError::WrongTransport => tr("This setting doesn't fit this kind of server.").into(),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for EnvError {}

/// The service's own environment for [`child_env`]: `std::env::vars_os`, without the entries that are not
/// UTF-8 (no basic one is, in practice).
pub fn service_env() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(name, value)| Some((name.into_string().ok()?, value.into_string().ok()?)))
        .collect()
}

/// The folders of the `PATH` a server's process gets, in order: the folder its program was found in
/// (`found`, so `npx` finds the `node` next to it), the service's `PATH` ([`Roots::path`]), then the usual
/// folders ([`Roots::usual`]). Each folder once, at its first place.
pub fn path_dirs(found: &Found, roots: &Roots) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in std::iter::once(found.dir.clone()).chain(roots.path.iter().cloned()).chain(roots.usual()) {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// The whole environment of the process of the program `spec` ([`ServerSpec::Stdio`]), in order: the
/// [`BASICS`] (on Windows also [`WINDOWS_BASICS`]) that `base` has, then `PATH` from `path_dirs` (left out
/// when none fits: a folder that is not UTF-8 or holds the separator is skipped), then the variables of
/// `spec`. A variable of the server with the name of a basic replaces it. Nothing else of `base` comes along.
///
/// - `secrets`: the stored secrets by name ([`super::store::read_secrets`]); a [`Value::Secret`] in `spec`
///   (while installing) is taken as it is.
/// - `base`: the service's environment ([`service_env`]); a parameter so tests bring their own.
///
/// The caller starts the process with `env_clear` and exactly these. Fails on the first variable that is
/// blocked or invalid, has a NUL, or whose secret is missing, and with [`EnvError::WrongTransport`] for an
/// address. The result holds secret values: never log it, never answer with it.
pub fn child_env(
    spec: &ServerSpec,
    secrets: &BTreeMap<String, Secret>,
    base: &BTreeMap<String, String>,
    path_dirs: &[PathBuf],
) -> Result<Vec<(String, String)>, EnvError> {
    let ServerSpec::Stdio { env, .. } = spec else { return Err(EnvError::WrongTransport) };
    let mut out: Vec<(String, String)> = Vec::new();
    let basics = BASICS.iter().chain(if cfg!(windows) { WINDOWS_BASICS } else { &[] });
    for wanted in basics {
        if let Some((name, value)) = base.iter().find(|(name, _)| same_name(name, wanted)) {
            out.push((name.clone(), value.clone()));
        }
    }
    if let Some(path) = joined_path(path_dirs) {
        out.push(("PATH".into(), path));
    }
    for (name, value) in env {
        check_env_name(name)?;
        let value = value_of(name, value, secrets)?;
        if value.contains('\0') {
            return Err(EnvError::InvalidValue { name: name.clone() });
        }
        out.retain(|(known, _)| !same_name(known, name));
        out.push((name.clone(), value));
    }
    Ok(out)
}

/// The headers of every request to the address `spec` ([`ServerSpec::Http`]), from its plain values and the
/// stored `secrets` by name (as for [`child_env`]). Fails on an invalid name ([`valid_header_name`]), a value
/// with a line break or another control character but tab, a missing secret, and with
/// [`EnvError::WrongTransport`] for a program. The result holds secret values: never log it.
pub fn headers(spec: &ServerSpec, secrets: &BTreeMap<String, Secret>) -> Result<Vec<(String, String)>, EnvError> {
    let ServerSpec::Http { headers, .. } = spec else { return Err(EnvError::WrongTransport) };
    headers
        .iter()
        .map(|(name, value)| {
            if !valid_header_name(name) {
                return Err(EnvError::InvalidName { name: name.clone() });
            }
            let value = value_of(name, value, secrets)?;
            if value.chars().any(|c| c.is_control() && c != '\t') {
                return Err(EnvError::InvalidValue { name: name.clone() });
            }
            Ok((name.clone(), value))
        })
        .collect()
}

/// Whether the core may talk to `url`: `https://` with a host, or `http://` only for `localhost`,
/// `127.0.0.1` and `::1` (written `[::1]`); no user or password in it. Redirects are refused by the client
/// (task MA.4), so this address is the only one a request goes to.
pub fn check_url(url: &str) -> Result<(), EnvError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| EnvError::BadUrl)?;
    let Some(host) = parsed.host_str().filter(|host| !host.is_empty()) else { return Err(EnvError::BadUrl) };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(EnvError::CredentialsInUrl);
    }
    // The parser writes addresses in one form (`127.1` is `127.0.0.1`, IPv6 in brackets, names lowercased).
    let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    match parsed.scheme() {
        "https" => Ok(()),
        "http" if local => Ok(()),
        _ => Err(EnvError::NotSecure),
    }
}

/// What installing or updating checks before anything is stored or started (`error` otherwise): a command
/// by name or absolute path and variables that may be given ([`blocked_env`], [`valid_env_name`]); an
/// address that passes [`check_url`] and valid header names. Values are not looked at here (a secret may
/// still be missing).
pub fn check_spec(spec: &ServerSpec) -> Result<(), EnvError> {
    match spec {
        ServerSpec::Stdio { command, env, .. } => {
            let path = std::path::Path::new(command);
            if command.trim().is_empty() || (!path.is_absolute() && path.components().count() != 1) {
                return Err(EnvError::BadCommand);
            }
            env.keys().try_for_each(|name| check_env_name(name))
        }
        ServerSpec::Http { url, headers } => {
            check_url(url)?;
            match headers.keys().find(|name| !valid_header_name(name)) {
                Some(name) => Err(EnvError::InvalidName { name: name.clone() }),
                None => Ok(()),
            }
        }
    }
}

fn check_env_name(name: &str) -> Result<(), EnvError> {
    if !valid_env_name(name) {
        return Err(EnvError::InvalidName { name: name.to_string() });
    }
    if blocked_env(name) {
        return Err(EnvError::Blocked { name: name.to_string() });
    }
    Ok(())
}

/// The text of `value`: as it is, the secret given, or the stored one from `secrets`.
fn value_of(name: &str, value: &Value, secrets: &BTreeMap<String, Secret>) -> Result<String, EnvError> {
    match value {
        Value::Plain(text) => Ok(text.clone()),
        Value::Secret(secret) => Ok(secret.expose().to_string()),
        Value::Stored { .. } => secrets
            .get(name)
            .map(|secret| secret.expose().to_string())
            .ok_or_else(|| EnvError::MissingSecret { name: name.to_string() }),
    }
}

/// Variable names: Windows does not tell case apart, the other systems do.
fn same_name(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// `dirs` as the value of `PATH`; folders that are not UTF-8 or hold the separator are left out, `None`
/// when none is left.
fn joined_path(dirs: &[PathBuf]) -> Option<String> {
    let usable: Vec<&PathBuf> =
        dirs.iter().filter(|dir| dir.to_str().is_some() && std::env::join_paths([dir]).is_ok()).collect();
    if usable.is_empty() {
        return None;
    }
    std::env::join_paths(usable).ok()?.into_string().ok()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn program(env: &[(&str, Value)]) -> ServerSpec {
        ServerSpec::Stdio {
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-sequential-thinking@2025.7.1".into()],
            env: env.iter().map(|(name, value)| (name.to_string(), value.clone())).collect(),
        }
    }

    fn service() -> BTreeMap<String, String> {
        [
            ("HOME", "/home/anna"),
            ("USER", "anna"),
            ("LANG", "de_DE.UTF-8"),
            ("PATH", "/usr/bin:/bin"),
            ("TORRO_TOKEN", "torro_k1_service"),
            ("OPENAI_API_KEY", "sk-service"),
            ("SSH_AUTH_SOCK", "/tmp/ssh-agent"),
            ("SystemRoot", "C:\\Windows"),
        ]
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
    }

    fn names(env: &[(String, String)]) -> Vec<&str> {
        env.iter().map(|(name, _)| name.as_str()).collect()
    }

    fn get<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
        env.iter().find(|(known, _)| known == name).map(|(_, value)| value.as_str())
    }

    #[test]
    fn the_service_environment_does_not_reach_the_server() {
        let spec = program(&[("MEMORY_FILE", Value::Plain("/tmp/memory.json".into()))]);
        let env = child_env(&spec, &BTreeMap::new(), &service(), &[PathBuf::from("/opt/node/bin")]).expect("env");
        for kept_back in ["TORRO_TOKEN", "OPENAI_API_KEY", "SSH_AUTH_SOCK"] {
            assert!(!names(&env).contains(&kept_back), "{kept_back} reaches the server: {env:?}");
        }
        assert!(!env.iter().any(|(_, value)| value.contains("service")), "no value of them either");
        assert_eq!(get(&env, "HOME"), Some("/home/anna"));
        assert_eq!(get(&env, "LANG"), Some("de_DE.UTF-8"));
        assert_eq!(get(&env, "LOGNAME"), None, "only what the service has");
        assert_eq!(get(&env, "MEMORY_FILE"), Some("/tmp/memory.json"));
        assert_eq!(get(&env, "PATH"), Some("/opt/node/bin"), "the core's PATH, not the service's");
        #[cfg(not(windows))]
        assert_eq!(get(&env, "SystemRoot"), None);
        #[cfg(windows)]
        assert_eq!(get(&env, "SystemRoot"), Some("C:\\Windows"));
    }

    #[test]
    fn the_path_starts_with_the_folder_of_the_program() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nvm = dir.path().join("home/.nvm/versions/node/v22.11.0/bin");
        let brew = dir.path().join("homebrew/bin");
        std::fs::create_dir_all(&nvm).expect("nvm");
        std::fs::create_dir_all(&brew).expect("brew");
        let service_path = dir.path().join("usr/bin");
        let roots = Roots {
            path: vec![service_path.clone(), nvm.clone()],
            home: Some(dir.path().join("home")),
            fixed: vec![brew.clone()],
        };
        let found = Found { program: nvm.join("npx"), dir: nvm.clone() };
        let dirs = path_dirs(&found, &roots);
        assert_eq!(dirs, vec![nvm.clone(), service_path.clone(), brew.clone()], "each folder once");

        let env = child_env(&program(&[]), &BTreeMap::new(), &service(), &dirs).expect("env");
        let path = get(&env, "PATH").expect("PATH");
        let parts: Vec<PathBuf> = std::env::split_paths(path).collect();
        assert_eq!(parts, dirs);
        assert_eq!(parts.first().map(PathBuf::as_path), Some(nvm.as_path()));
    }

    #[test]
    fn blocked_variables_are_refused_in_any_case() {
        for name in ["ld_preload", "LD_LIBRARY_PATH", "DYLD_FOO", "dyld_insert_libraries", "PATH", "Path", "TORRO_KEY"]
        {
            let spec = program(&[(name, Value::Plain("x".into()))]);
            let blocked = EnvError::Blocked { name: name.into() };
            assert_eq!(check_spec(&spec), Err(blocked.clone()), "{name}");
            assert_eq!(child_env(&spec, &BTreeMap::new(), &service(), &[]), Err(blocked), "{name}");
        }
        let bad = program(&[("A=B", Value::Plain("x".into()))]);
        assert_eq!(check_spec(&bad), Err(EnvError::InvalidName { name: "A=B".into() }));
        assert!(check_spec(&program(&[("GITHUB_TOKEN", Value::Stored { secret: true })])).is_ok());
    }

    #[test]
    fn secrets_come_from_the_keychain_or_the_request() {
        let spec = program(&[
            ("STORED_TOKEN", Value::Stored { secret: true }),
            ("NEW_TOKEN", Value::Secret(Secret::new("new-value"))),
            ("HOME", Value::Plain("/srv/server-home".into())),
        ]);
        let secrets = BTreeMap::from([("STORED_TOKEN".to_string(), Secret::new("from-keychain"))]);
        let env = child_env(&spec, &secrets, &service(), &[]).expect("env");
        assert_eq!(get(&env, "STORED_TOKEN"), Some("from-keychain"));
        assert_eq!(get(&env, "NEW_TOKEN"), Some("new-value"));
        assert_eq!(get(&env, "HOME"), Some("/srv/server-home"), "the server's own replaces the basic");
        assert_eq!(names(&env).iter().filter(|name| **name == "HOME").count(), 1);
        assert_eq!(get(&env, "PATH"), None, "no folder, no PATH");

        let missing = child_env(&spec, &BTreeMap::new(), &service(), &[]);
        assert_eq!(missing, Err(EnvError::MissingSecret { name: "STORED_TOKEN".into() }));
        let nul = program(&[("X", Value::Plain("a\0b".into()))]);
        assert_eq!(child_env(&nul, &secrets, &service(), &[]), Err(EnvError::InvalidValue { name: "X".into() }));
        let http = ServerSpec::Http { url: "https://example.com".into(), headers: BTreeMap::new() };
        assert_eq!(child_env(&http, &secrets, &service(), &[]), Err(EnvError::WrongTransport));
    }

    #[test]
    fn addresses_are_https_or_on_this_computer() {
        for allowed in [
            "https://mcp.example.com/mcp",
            "http://127.0.0.1:8808/mcp",
            "http://localhost:3000",
            "http://LOCALHOST/mcp",
            "http://[::1]:9000/",
        ] {
            assert_eq!(check_url(allowed), Ok(()), "{allowed}");
        }
        for (refused, error) in [
            ("http://example.com/mcp", EnvError::NotSecure),
            ("http://127.0.0.2/mcp", EnvError::NotSecure),
            ("http://localhost.example.com/", EnvError::NotSecure),
            ("ftp://example.com/", EnvError::NotSecure),
            ("ws://localhost/", EnvError::NotSecure),
            ("https://anna:pw@example.com/mcp", EnvError::CredentialsInUrl),
            ("example.com/mcp", EnvError::BadUrl),
            ("file:///etc/passwd", EnvError::BadUrl),
        ] {
            assert_eq!(check_url(refused), Err(error), "{refused}");
        }
        let http = ServerSpec::Http { url: "http://example.com".into(), headers: BTreeMap::new() };
        assert_eq!(check_spec(&http), Err(EnvError::NotSecure));
    }

    #[test]
    fn headers_come_from_values_and_secrets() {
        let spec = ServerSpec::Http {
            url: "https://mcp.example.com/mcp".into(),
            headers: BTreeMap::from([
                ("Authorization".to_string(), Value::Stored { secret: true }),
                ("X-Team".to_string(), Value::Plain("torro".into())),
            ]),
        };
        let secrets = BTreeMap::from([("Authorization".to_string(), Secret::new("Bearer tok"))]);
        assert_eq!(
            headers(&spec, &secrets),
            Ok(vec![("Authorization".into(), "Bearer tok".into()), ("X-Team".into(), "torro".into())])
        );
        assert_eq!(headers(&spec, &BTreeMap::new()), Err(EnvError::MissingSecret { name: "Authorization".into() }));
        let split = ServerSpec::Http {
            url: "https://mcp.example.com/mcp".into(),
            headers: BTreeMap::from([("X-Team".to_string(), Value::Plain("a\r\nHost: evil".into()))]),
        };
        assert_eq!(headers(&split, &secrets), Err(EnvError::InvalidValue { name: "X-Team".into() }));
        let bad_name = ServerSpec::Http {
            url: "https://mcp.example.com/mcp".into(),
            headers: BTreeMap::from([("X Team".to_string(), Value::Plain("a".into()))]),
        };
        assert_eq!(check_spec(&bad_name), Err(EnvError::InvalidName { name: "X Team".into() }));
        assert_eq!(headers(&program(&[]), &secrets), Err(EnvError::WrongTransport));
    }

    #[test]
    fn a_command_is_a_name_or_an_absolute_path() {
        let with =
            |command: &str| ServerSpec::Stdio { command: command.into(), args: Vec::new(), env: BTreeMap::new() };
        assert_eq!(check_spec(&with("uvx")), Ok(()));
        let absolute = if cfg!(windows) { "C:\\tools\\server.exe" } else { "/usr/local/bin/server" };
        assert_eq!(check_spec(&with(absolute)), Ok(()));
        for bad in ["", "  ", "./server", "bin/server"] {
            assert_eq!(check_spec(&with(bad)), Err(EnvError::BadCommand), "{bad:?}");
        }
        assert!(Path::new(absolute).is_absolute());
    }

    #[test]
    fn errors_name_the_variable_but_never_a_value() {
        let error = EnvError::Blocked { name: "LD_PRELOAD".into() };
        assert!(error.to_string().contains("LD_PRELOAD"));
        assert!(EnvError::MissingSecret { name: "TOKEN".into() }.to_string().contains("TOKEN"));
        assert_eq!(
            EnvError::NotSecure.to_string(),
            tr("The address must start with https://. Only this computer (localhost) may use http://.")
        );
    }
}
