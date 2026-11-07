//! Documented paths, guided imports and lossless configuration edits for open MCP clients (#539).

use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Paths {
    pub xdg_config: Option<PathBuf>,
    pub cline_dir: Option<PathBuf>,
    pub cline_settings: Option<PathBuf>,
    pub continue_config: Option<PathBuf>,
    pub librechat_config: Option<PathBuf>,
}

impl Paths {
    pub fn current() -> Self {
        let path = |name| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
        Self {
            xdg_config: path("XDG_CONFIG_HOME"),
            cline_dir: path("CLINE_DIR"),
            cline_settings: path("CLINE_MCP_SETTINGS_PATH"),
            // Continue's --config and LibreChat's local file are explicitly chosen in the wizard.
            continue_config: None,
            librechat_config: None,
        }
    }
}

fn nonempty(path: &Option<PathBuf>) -> Option<&Path> {
    path.as_deref().filter(|path| !path.as_os_str().is_empty())
}

fn config(env: &Env) -> PathBuf {
    nonempty(&env.open.xdg_config).map(Path::to_path_buf).unwrap_or_else(|| env.home.join(".config"))
}

pub(super) fn plugin_dir(env: &Env) -> PathBuf {
    config(env).join("opencode/plugins/torroworkplace")
}

pub(super) fn file(target: Target, env: &Env) -> PathBuf {
    match target {
        Target::OpenCode => config(env).join("opencode/opencode.json"),
        Target::Zed => {
            #[cfg(windows)]
            {
                env.app_config.join("Zed/settings.json")
            }
            #[cfg(target_os = "macos")]
            {
                env.home.join(".config/zed/settings.json")
            }
            #[cfg(not(any(windows, target_os = "macos")))]
            {
                config(env).join("zed/settings.json")
            }
        }
        Target::Cline => nonempty(&env.open.cline_settings).map(Path::to_path_buf).unwrap_or_else(|| {
            nonempty(&env.open.cline_dir)
                .map(Path::to_path_buf)
                .unwrap_or_else(|| env.home.join(".cline"))
                .join("data/settings/cline_mcp_settings.json")
        }),
        Target::Continue => nonempty(&env.open.continue_config)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| env.home.join(".continue/config.yaml")),
        Target::Librechat => nonempty(&env.open.librechat_config).map(Path::to_path_buf).unwrap_or_default(),
        _ => PathBuf::new(),
    }
}

fn cline_editor(env: &Env) -> PathBuf {
    env.app_config.join("Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json")
}

pub(super) fn protected_files(env: &Env) -> Vec<PathBuf> {
    let mut paths = vec![
        plugin_dir(env),
        plugin_dir(env).join("bridge.mjs"),
        file(Target::OpenCode, env).with_extension("jsonc"),
        cline_editor(env),
    ];
    paths.extend(
        Target::ALL
            .into_iter()
            .filter(|target| target.is_open())
            .map(|target| file(target, env))
            .filter(|path| path.is_absolute()),
    );
    paths
}

pub(super) fn locate(target: Target, env: &Env) -> Vec<Method> {
    let mut file = file(target, env);
    let command = match target {
        Target::OpenCode => "opencode",
        Target::OpenWebui => "open-webui",
        Target::Zed => "zed",
        Target::Cline => "cline",
        Target::Continue => "cn",
        Target::CherryStudio => "cherry-studio",
        _ => "",
    };
    let executable = env.find_command(command, &[]);
    if target == Target::OpenCode && file.with_extension("jsonc").is_file() {
        // Both are returned as an ambiguous target and rejected before any key is written.
        if !file.is_file() {
            file.set_extension("jsonc");
        }
    }
    let mut methods = Vec::new();
    if matches!(target, Target::OpenWebui | Target::CherryStudio) {
        if executable.is_some() {
            methods.push(Method::Guided { target });
        }
    } else if file.is_file() || executable.is_some() {
        if target == Target::Continue && !file.is_file() {
            methods.push(Method::Guided { target });
        } else {
            methods.push(Method::Open {
                target,
                file,
                executable: if target == Target::OpenCode { executable } else { None },
            });
        }
    }
    if target == Target::Cline && cline_editor(env).is_file() {
        let editor = Method::Open { target, file: cline_editor(env), executable: None };
        if !methods.contains(&editor) {
            methods.push(editor);
        }
    }
    methods
}

fn entry(target: Target, server: &Server<'_>) -> Value {
    if target == Target::OpenCode {
        json!({"type":"local", "command":[server.program.display().to_string(), "mcp"],
               "environment":{TOKEN_VARIABLE:server.key}})
    } else if target == Target::Continue {
        let mut value = json_entry(server, false);
        value["name"] = json!(SERVER_NAME);
        value
    } else {
        json_entry(server, false)
    }
}

pub(crate) fn configuration(target: Target, program: &Path, key: &str) -> String {
    let server = Server { program, key };
    let value = match target {
        Target::OpenCode => json!({"mcp":{"servers":{SERVER_NAME:entry(target, &server)}}}),
        Target::Zed => json!({"context_servers":{SERVER_NAME:entry(target, &server)}}),
        Target::Continue => json!({"mcpServers":[entry(target, &server)]}),
        _ => json!({"mcpServers":{SERVER_NAME:entry(target, &server)}}),
    };
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

pub(crate) fn guidance(target: Target) -> &'static str {
    match target {
        Target::OpenCode => tr(
            "Add this entry under mcp.servers in your global OpenCode configuration. Reload OpenCode, then call system_ping.",
        ),
        Target::Zed => {
            tr("Merge this context_servers entry into Zed settings. Enable the server, then call system_ping.")
        }
        Target::Cline => tr(
            "Merge this mcpServers entry into the selected Cline MCP settings. Reload MCP servers, then call system_ping.",
        ),
        Target::Continue => tr(
            "Merge the mcpServers list item into your existing Continue YAML configuration. Keep your models. Select that file with cn --config <file>, then call system_ping.",
        ),
        Target::CherryStudio => tr(
            "In Cherry Studio: Settings → MCP → MCP Servers → Add → Import from JSON. Import this configuration, enable the server and bind it to your agent. Then call system_ping.",
        ),
        Target::Librechat => tr(
            "Merge this mcpServers entry into a local librechat.yaml, then restart LibreChat. The Torro program and socket must be reachable there; containers or remote servers need a separately secured connection. Then call system_ping.",
        ),
        Target::OpenWebui => tr(
            "Open WebUI cannot run this stdio server directly.\n\n1. Save the copied configuration privately for a separately installed mcpo bridge.\n\n2. Start mcpo yourself with --config <file> and --api-key <separate-proxy-key>. The proxy key is NOT the Torro token.\n\n3. Add the bridge's OpenAPI URL in Open WebUI, then call system_ping through the configured tool.\n\nContainers and remote hosts need a separately secured connection. Torro installs nothing and opens no network access.",
        ),
        _ => tr("Put this entry into your client's configuration, then call system_ping."),
    }
}

pub(super) fn fingerprint(target: Target, server: &Server<'_>) -> String {
    hash(&entry(target, server))
}

fn hash(value: &Value) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.to_string().as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn add(method: &Method, server: &Server<'_>, runner: &dyn Runner) -> Result<(), SetupError> {
    add_owned(method, server, runner, None)
}

pub(super) fn add_owned(
    method: &Method,
    server: &Server<'_>,
    runner: &dyn Runner,
    owned: Option<&str>,
) -> Result<(), SetupError> {
    let Method::Open { target, file, executable } = method else { return Ok(()) };
    if *target == Target::OpenCode {
        let other =
            file.with_extension(if file.extension().is_some_and(|ext| ext == "jsonc") { "json" } else { "jsonc" });
        if other.try_exists().map_err(SetupError::io(&other))? {
            return Err(SetupError::Foreign(other));
        }
    }
    let before = read_text(file)?;
    // Validate the whole document and ownership even when a CLI will do the write.
    let after = edited(file, *target, before.as_deref(), Some(&entry(*target, server)), owned)?;
    if let Some(executable) = executable {
        if let Some(text) = &before {
            backup(file, text)?;
        }
        let args = ["mcp", "add", SERVER_NAME, "--global", "--env"]
            .map(String::from)
            .into_iter()
            .chain([
                format!("{TOKEN_VARIABLE}={}", server.key),
                "--".into(),
                server.program.display().to_string(),
                "mcp".into(),
            ])
            .collect::<Vec<_>>();
        let written = || -> Result<(), SetupError> {
            runner.run(executable, &args).map_err(|err| err.of(Tool::OpenCode))?;
            // The CLI owns formatting, but successful setup must leave a readable, private file.
            read_text(file)?.ok_or_else(|| SetupError::Unreadable(file.clone()))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(file, fs::Permissions::from_mode(0o600)).map_err(SetupError::io(file))?;
            }
            Ok(())
        };
        if let Err(err) = written() {
            restore(file, before.as_deref())?;
            return Err(err);
        }
        Ok(())
    } else {
        replace(file, before.as_deref(), &after)
    }
}

fn backup(file: &Path, text: &str) -> Result<(), SetupError> {
    let target = fs::canonicalize(file).map_err(SetupError::io(file))?;
    let mut name = target.as_os_str().to_os_string();
    name.push(BACKUP_SUFFIX);
    let path = PathBuf::from(name);
    write_private(&path, text.as_bytes()).map_err(|_| SetupError::Unreadable(path))
}

fn restore(file: &Path, before: Option<&str>) -> Result<(), SetupError> {
    match before {
        Some(text) => {
            let target = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
            write_private(&target, text.as_bytes()).map_err(|_| SetupError::Unreadable(file.into()))
        }
        None => match fs::remove_file(file) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => Err(SetupError::io(file)(err)),
            _ => Ok(()),
        },
    }
}

pub(super) fn remove(method: &Method) -> Result<(), SetupError> {
    remove_owned(method, None)
}

pub(super) fn remove_owned(method: &Method, owned: Option<&str>) -> Result<(), SetupError> {
    let Method::Open { target, file, .. } = method else { return Ok(()) };
    let Some(before) = read_text(file)? else { return Ok(()) };
    let after = edited(file, *target, Some(&before), None, owned)?;
    replace(file, Some(&before), &after)
}

fn edited(
    file: &Path,
    target: Target,
    before: Option<&str>,
    entry: Option<&Value>,
    owned: Option<&str>,
) -> Result<String, SetupError> {
    if matches!(target, Target::Continue | Target::Librechat) {
        // Never invent Continue's model configuration or a LibreChat installation.
        return edit_yaml(file, target, before.ok_or_else(|| SetupError::Unreadable(file.into()))?, entry, owned);
    }
    edit_json(file, target, before.unwrap_or("{}"), entry, owned)
}

fn check_owner(
    file: &Path,
    current: Option<&Value>,
    entry: Option<&Value>,
    owned: Option<&str>,
) -> Result<(), SetupError> {
    if let Some(current) = current {
        if entry == Some(current) || owned.is_some_and(|owned| hash(current) == owned) {
            return Ok(());
        }
        return Err(SetupError::Foreign(file.into()));
    }
    Ok(())
}

fn input(value: &Value) -> jsonc_parser::cst::CstInputValue {
    use jsonc_parser::cst::CstInputValue as C;
    match value {
        Value::Null => C::Null,
        Value::Bool(v) => C::Bool(*v),
        Value::Number(v) => C::Number(v.to_string()),
        Value::String(v) => C::String(v.clone()),
        Value::Array(v) => C::Array(v.iter().map(input).collect()),
        Value::Object(v) => C::Object(v.iter().map(|(k, v)| (k.clone(), input(v))).collect()),
    }
}

fn unique_json(node: &jsonc_parser::cst::CstNode) -> bool {
    if let Some(object) = node.as_object() {
        let mut names = std::collections::HashSet::new();
        object.properties().iter().all(|prop| {
            prop.name().and_then(|name| name.decoded_value().ok()).is_some_and(|name| names.insert(name))
                && prop.value().is_some_and(|value| unique_json(&value))
        })
    } else if let Some(array) = node.as_array() {
        array.elements().iter().all(unique_json)
    } else {
        true
    }
}

fn edit_json(
    file: &Path,
    target: Target,
    text: &str,
    entry: Option<&Value>,
    owned: Option<&str>,
) -> Result<String, SetupError> {
    use jsonc_parser::cst::{CstInputValue, CstRootNode};
    let bad = || SetupError::Unreadable(file.into());
    let doc = CstRootNode::parse(text, &Default::default()).map_err(|_| bad())?;
    if !doc.value().is_some_and(|value| unique_json(&value)) {
        return Err(bad());
    }
    let mut object = doc.object_value().ok_or_else(bad)?;
    if target == Target::OpenCode && object.object_value("mcp").is_some_and(|mcp| mcp.get(SERVER_NAME).is_some()) {
        // A V1 entry of the same name is not ours to migrate or shadow with a V2 entry.
        return Err(SetupError::Foreign(file.into()));
    }
    let path: &[&str] = match target {
        Target::OpenCode => &["mcp", "servers"],
        Target::Zed => &["context_servers"],
        _ => &["mcpServers"],
    };
    for name in path {
        if object.get(name).is_none() {
            if entry.is_none() {
                return Ok(text.into());
            }
            object.append(name, CstInputValue::Object(Vec::new()));
        }
        object = object.object_value(name).ok_or_else(bad)?;
    }
    let current = object.get(SERVER_NAME);
    let value = current.as_ref().and_then(|prop| prop.to_serde_value());
    check_owner(file, value.as_ref(), entry, owned)?;
    if value.as_ref() == entry {
        return Ok(text.into());
    }
    match (current, entry) {
        (Some(prop), None) => prop.remove(),
        (Some(prop), Some(entry)) => prop.set_value(input(entry)),
        (None, Some(entry)) => {
            object.append(SERVER_NAME, input(entry));
        }
        _ => {}
    }
    Ok(doc.to_string())
}

fn yaml_value(node: &yaml_edit::YamlNode) -> Option<Value> {
    use yaml_edit::YamlNode as Y;
    match node {
        Y::Scalar(s) => Some(Value::String(s.as_string())),
        Y::Mapping(m) => {
            let mut values = Map::new();
            for (key, value) in m.iter() {
                let key = key.as_scalar()?.as_string();
                if key == "<<" || values.contains_key(&key) {
                    return None;
                }
                values.insert(key, yaml_value(&value)?);
            }
            (values.len() == m.len()).then_some(Value::Object(values))
        }
        Y::Sequence(s) => s.values().map(|value| yaml_value(&value)).collect::<Option<Vec<_>>>().map(Value::Array),
        _ => None,
    }
}

fn yaml_input(value: &Value) -> yaml_edit::YamlValue {
    match value {
        Value::Object(values) => {
            yaml_edit::YamlValue::Mapping(values.iter().map(|(key, value)| (key.clone(), yaml_input(value))).collect())
        }
        Value::Array(values) => yaml_edit::YamlValue::Sequence(values.iter().map(yaml_input).collect()),
        _ => yaml_edit::YamlValue::scalar(value.as_str().unwrap_or_default()),
    }
}

fn edit_yaml(
    file: &Path,
    target: Target,
    text: &str,
    entry: Option<&Value>,
    owned: Option<&str>,
) -> Result<String, SetupError> {
    use std::str::FromStr;
    let bad = || SetupError::Unreadable(file.into());
    let doc = yaml_edit::YamlFile::from_str(text).map_err(|_| bad())?;
    if doc.documents().count() != 1 {
        return Err(bad());
    }
    // Refuse anchors/tags too: editing through an alias or merge could change unrelated servers.
    if text.split_whitespace().any(|word| word.starts_with(['&', '*', '!']) || word == "<<:") {
        return Err(bad());
    }
    let document = doc.document().ok_or_else(bad)?;
    yaml_edit::YamlValue::from_document(&document).map_err(|_| bad())?;
    let root = document.as_mapping().ok_or_else(bad)?;
    yaml_value(&yaml_edit::YamlNode::Mapping(root.clone())).ok_or_else(bad)?;
    if root.get("mcpServers").is_none() {
        if entry.is_none() {
            return Ok(text.into());
        }
        let empty = if target == Target::Continue { yaml_input(&json!([])) } else { yaml_input(&json!({})) };
        root.set("mcpServers", empty);
    }
    if target == Target::Continue {
        let seq = root.get_sequence("mcpServers").ok_or_else(bad)?;
        let mut found = Vec::new();
        for (index, value) in seq.values().enumerate() {
            let value = yaml_value(&value).ok_or_else(bad)?;
            if value.get("name").and_then(Value::as_str) == Some(SERVER_NAME) {
                found.push((index, value));
            }
        }
        if found.len() > 1 {
            return Err(bad());
        }
        check_owner(file, found.first().map(|(_, value)| value), entry, owned)?;
        match (found.first(), entry) {
            (Some((index, value)), Some(entry)) if value != entry => {
                seq.set(*index, yaml_input(entry));
            }
            (Some((index, _)), None) => {
                seq.remove(*index);
            }
            (None, Some(entry)) => seq.push(yaml_input(entry)),
            _ => {}
        }
    } else {
        let servers = root.get_mapping("mcpServers").ok_or_else(bad)?;
        let value = servers.get(SERVER_NAME).as_ref().and_then(yaml_value);
        check_owner(file, value.as_ref(), entry, owned)?;
        match entry {
            Some(entry) if value.as_ref() != Some(entry) => servers.set(SERVER_NAME, yaml_input(entry)),
            None => {
                servers.remove(SERVER_NAME);
            }
            _ => {}
        }
    }
    Ok(doc.to_string())
}

fn text_hash(text: &str) -> String {
    hash(&Value::String(text.into()))
}

pub(super) fn connect_plugin(
    env: &Env,
    owner: Option<&str>,
    client: &str,
    key: &IssuedKey,
    mut records: Records,
) -> Result<(), SetupError> {
    let previous = records.clone();
    let key_file = hook_key_file(env, client);
    let dir = plugin_dir(env);
    let index = include_str!("../../../../assets/opencode/torroworkplace/index.ts")
        .replace("__TORRO_PROGRAM__", &Value::String(env.program.display().to_string()).to_string())
        .replace("__TORRO_KEY_FILE__", &Value::String(key_file.display().to_string()).to_string());
    let files = [
        (dir.join("index.ts"), index),
        (dir.join("bridge.mjs"), include_str!("../../../../assets/opencode/torroworkplace/bridge.mjs").into()),
    ];
    let old = records.hooks.iter().find(|record| record.agent == AgentKind::OpenCode);
    let mut before = Vec::new();
    for (file, _) in &files {
        let text = read_text(file)?;
        if let Some(text) = &text
            && !old.is_some_and(|old| old.artifacts.iter().any(|(path, hash)| path == file && *hash == text_hash(text)))
        {
            return Err(SetupError::Foreign(file.clone()));
        }
        before.push((file.clone(), text));
    }
    let old_key = read_text(&key_file)?;
    if old_key.is_some() && old.is_none_or(|old| old.key_file.as_ref() != Some(&key_file)) {
        return Err(SetupError::Foreign(key_file));
    }
    let new = HookRecord {
        client: client.into(),
        owner: owner.map(str::to_string),
        agent: AgentKind::OpenCode,
        file: dir.join("index.ts"),
        key_file: Some(key_file.clone()),
        artifacts: files.iter().map(|(path, text)| (path.clone(), text_hash(text))).collect(),
    };
    records.hooks.retain(|record| record.agent != AgentKind::OpenCode);
    records.hooks.push(new);
    let write = || -> Result<(), SetupError> {
        create_private_dir(&dir).map_err(SetupError::io(&dir))?;
        write_hook_key(&key_file, key.expose())?;
        for ((file, text), (_, old)) in files.iter().zip(&before) {
            replace(file, old.as_deref(), text)?;
        }
        save_records(env, &records)
    };
    if let Err(err) = write() {
        for (file, text) in &before {
            restore(file, text.as_deref())?;
        }
        restore(&key_file, old_key.as_deref())?;
        return Err(err);
    }
    for old in previous.hooks.iter().filter(|record| record.agent == AgentKind::OpenCode) {
        if let Some(old) = old.key_file.as_ref().filter(|path| **path != key_file) {
            delete_hook_key(old)?;
        }
    }
    Ok(())
}

pub(super) fn remove_plugin(record: &HookRecord) -> Result<(), SetupError> {
    for (path, expected) in &record.artifacts {
        if read_text(path)?.is_some_and(|text| text_hash(&text) != *expected) {
            return Err(SetupError::Foreign(path.clone()));
        }
    }
    for (path, _) in &record.artifacts {
        restore(path, None)?;
    }
    if let Some(dir) = record.file.parent() {
        let _ = fs::remove_dir(dir);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_ipc::Key;

    fn environment(dir: &Path) -> Env {
        Env {
            open: Default::default(),
            home: dir.join("home"),
            app_config: dir.join("app-config"),
            path: vec![dir.join("bin")],
            program: dir.join("torroworkplace"),
            config_dir: dir.join("torro"),
        }
    }

    fn write(file: &Path, text: &str) {
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(file, text).expect("write");
    }

    struct Never;
    impl Runner for Never {
        fn run(&self, _: &Path, _: &[String]) -> Result<(), RunError> {
            panic!("no command expected")
        }
    }

    fn key() -> IssuedKey {
        IssuedKey::new(Key::new("torro_test_secret"))
    }

    #[test]
    fn every_open_target_has_a_stable_id_and_an_explicit_way() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut env = environment(dir.path());
        for target in Target::ALL.into_iter().filter(|target| target.is_open()) {
            assert!(torro_ipc::api::MCP_KINDS.contains(&target.kind()));
            assert_eq!(serde_json::to_value(target).expect("json"), target.kind());
            assert!(locate(target, &env).is_empty());
            assert!(!guidance(target).is_empty());
        }
        env.open.xdg_config = Some(dir.path().join("xdg"));
        assert_eq!(file(Target::OpenCode, &env), dir.path().join("xdg/opencode/opencode.json"));
        assert_eq!(plugin_dir(&env), dir.path().join("xdg/opencode/plugins/torroworkplace"));
        env.open.cline_dir = Some(dir.path().join("cline"));
        assert_eq!(file(Target::Cline, &env), dir.path().join("cline/data/settings/cline_mcp_settings.json"));
        env.open.cline_settings = Some(dir.path().join("selected.json"));
        assert_eq!(file(Target::Cline, &env), dir.path().join("selected.json"));
        env.open.cline_settings = Some(PathBuf::new());
        assert_eq!(file(Target::Cline, &env), dir.path().join("cline/data/settings/cline_mcp_settings.json"));
        env.open.librechat_config = Some(dir.path().join("local/librechat.yaml"));
        write(&file(Target::Librechat, &env), "version: 1\n");
        assert_eq!(locate(Target::Librechat, &env).len(), 1);
        write(&file(Target::Cline, &env), "{}");
        write(&cline_editor(&env), "{}");
        assert_eq!(locate(Target::Cline, &env).len(), 2, "both targets are offered, not both modified");
    }

    #[cfg(unix)]
    #[test]
    fn commands_find_only_the_documented_targets_and_do_not_invent_yaml() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        for (target, command) in [
            (Target::OpenCode, "opencode"),
            (Target::OpenWebui, "open-webui"),
            (Target::Zed, "zed"),
            (Target::Cline, "cline"),
            (Target::Continue, "cn"),
            (Target::CherryStudio, "cherry-studio"),
        ] {
            let executable = dir.path().join("bin").join(command);
            write(&executable, "#!/bin/sh\n");
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).expect("mode");
            let methods = locate(target, &env);
            assert_eq!(methods.len(), 1, "{target:?}");
            if matches!(target, Target::Continue | Target::CherryStudio | Target::OpenWebui) {
                assert!(matches!(methods[0], Method::Guided { .. }));
            }
        }
    }

    #[test]
    fn jsonc_edits_preserve_comments_foreign_values_and_ownership() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        for target in [Target::OpenCode, Target::Zed, Target::Cline] {
            let file = file(target, &env);
            let original = "{\n  // keep this comment\n  \"unrelated\": {\"flag\": true,},\n}\n";
            write(&file, original);
            let method = Method::Open { target, file: file.clone(), executable: None };
            connect(&env, target, &method, "c1", &key(), &Never).expect("connected");
            let text = read_text(&file).expect("read").expect("exists");
            assert!(text.contains("// keep this comment") && text.contains("\"flag\": true,"));
            assert!(text.contains(key().expose()));
            assert!(
                !read_text(&env.config_dir.join(RECORD_FILE)).expect("read").expect("exists").contains(key().expose())
            );
            connect(&env, target, &method, "c1", &key(), &Never).expect("idempotent");
            assert_eq!(read_text(&file).expect("read").expect("exists"), text);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(fs::metadata(&file).expect("meta").permissions().mode() & 0o777, 0o600);
                let backup = PathBuf::from(format!("{}{BACKUP_SUFFIX}", file.display()));
                assert_eq!(fs::metadata(backup).expect("backup").permissions().mode() & 0o777, 0o600);
            }
            disconnect(&env, "c1", &Never).expect("disconnected");
            let clean = read_text(&file).expect("read").expect("exists");
            assert!(clean.contains("// keep this comment") && !clean.contains(key().expose()));
            write(
                &file,
                "{\"mcpServers\":{\"torroworkplace\":{\"command\":\"foreign\"}},\"context_servers\":{\"torroworkplace\":{}},\"mcp\":{\"servers\":{\"torroworkplace\":{}}}}",
            );
            assert!(matches!(connect(&env, target, &method, "c2", &key(), &Never), Err(SetupError::Foreign(_))));
        }
    }

    #[test]
    fn yaml_edits_preserve_models_comments_and_other_servers() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        for (target, original) in [
            (
                Target::Continue,
                "# keep\nname: my-config\nversion: 1\nmodels: []\nmcpServers:\n  - name: other\n    command: other\n",
            ),
            (Target::Librechat, "# keep\nversion: 1\nmcpServers:\n  other:\n    command: other\n"),
        ] {
            let path = dir.path().join(format!("{}.yaml", target.kind()));
            write(&path, original);
            let method = Method::Open { target, file: path.clone(), executable: None };
            connect(&env, target, &method, "c1", &key(), &Never).expect("connected");
            let text = read_text(&path).expect("read").expect("exists");
            assert!(text.contains("# keep") && text.contains("command: other"));
            connect(&env, target, &method, "c1", &key(), &Never).expect("again");
            assert_eq!(read_text(&path).expect("read").expect("exists"), text);
            disconnect(&env, "c1", &Never).expect("removed");
            let text = read_text(&path).expect("read").expect("exists");
            assert!(text.contains("# keep") && text.contains("command: other") && !text.contains(key().expose()));
        }
    }

    #[test]
    fn malformed_duplicate_and_ambiguous_documents_are_never_written() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        for (target, source) in [
            (Target::Zed, "{ broken"),
            (Target::Zed, "{\"context_servers\":{},\"context_servers\":{}}"),
            (Target::Continue, "name: a\nname: b\n"),
            (Target::Continue, "mcpServers: {}\n"),
            (Target::Librechat, "mcpServers: *foreign\n"),
            (Target::Librechat, "---\na: 1\n---\nb: 2\n"),
        ] {
            let file = dir.path().join("selected");
            write(&file, source);
            let method = Method::Open { target, file: file.clone(), executable: None };
            assert!(connect(&env, target, &method, "c1", &key(), &Never).is_err(), "{source}");
            assert_eq!(read_text(&file).expect("read").expect("exists"), source);
        }
        let file = file(Target::OpenCode, &env);
        write(&file, "{}");
        write(&file.with_extension("jsonc"), "{}");
        assert!(
            connect(
                &env,
                Target::OpenCode,
                &Method::Open { target: Target::OpenCode, file, executable: None },
                "c1",
                &key(),
                &Never
            )
            .is_err()
        );
    }

    #[test]
    fn the_opencode_cli_uses_global_env_and_never_a_remove_command_and_rolls_back() {
        use std::cell::RefCell;
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        let file = file(Target::OpenCode, &env);
        write(&file, "{\"other\":true}");
        let calls = RefCell::new(Vec::new());
        struct Cli<'a> {
            calls: &'a RefCell<Vec<Vec<String>>>,
            file: &'a Path,
        }
        impl Runner for Cli<'_> {
            fn run(&self, _: &Path, args: &[String]) -> Result<(), RunError> {
                self.calls.borrow_mut().push(args.to_vec());
                fs::write(self.file, "half written key").expect("fake writes");
                Err(RunError::Failed)
            }
        }
        let runner = Cli { calls: &calls, file: &file };
        let method = Method::Open {
            target: Target::OpenCode,
            file: file.clone(),
            executable: Some(dir.path().join("opencode")),
        };
        assert!(connect(&env, Target::OpenCode, &method, "c1", &key(), &runner).is_err());
        assert_eq!(read_text(&file).expect("read").expect("exists"), "{\"other\":true}");
        assert_eq!(&calls.borrow()[0][..5], &["mcp", "add", SERVER_NAME, "--global", "--env"]);
        assert_eq!(calls.borrow().len(), 1);
        assert!(records(&env).expect("records").is_empty());
    }

    #[test]
    fn plugin_install_replacement_and_removal_own_only_their_files() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        let plugin = plugin_dir(&env);
        write(&plugin.join("foreign.txt"), "mine");
        connect_hooks(&env, AgentKind::OpenCode, Some("c1"), "h1", &key()).expect("hooks");
        let index = read_text(&plugin.join("index.ts")).expect("read").expect("exists");
        assert!(index.contains("Plugin.define") && !index.contains(key().expose()) && !index.contains("__TORRO_"));
        connect_hooks(&env, AgentKind::OpenCode, Some("c1"), "h2", &key()).expect("replace");
        assert!(!hook_key_file(&env, "h1").exists());
        assert_eq!(unhook(&env, "c1").expect("removed"), ["h2"]);
        assert!(!plugin.join("index.ts").exists() && !plugin.join("bridge.mjs").exists());
        assert_eq!(read_text(&plugin.join("foreign.txt")).expect("read").as_deref(), Some("mine"));
        write(&plugin.join("index.ts"), "foreign plugin");
        assert!(matches!(
            connect_hooks(&env, AgentKind::OpenCode, Some("c1"), "h3", &key()),
            Err(SetupError::Foreign(_))
        ));
        assert!(!hook_key_file(&env, "h3").exists());
    }

    #[cfg(unix)]
    #[test]
    fn an_owned_bridge_moved_to_dotfiles_is_reserved_before_replacement() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        connect_hooks(&env, AgentKind::OpenCode, Some("c1"), "h1", &key()).expect("hooks");
        let path = plugin_dir(&env).join("bridge.mjs");
        let outside = dir.path().join("dotfiles-bridge.mjs");
        fs::rename(&path, &outside).expect("move");
        std::os::unix::fs::symlink(&outside, &path).expect("link");
        connect_hooks(&env, AgentKind::OpenCode, Some("c1"), "h2", &key()).expect("replace");
        let registry = protection::Registry::new(env.config_dir.clone());
        let (_, paths) = registry.read().expect("protected");
        assert!(paths.contains(&fs::canonicalize(&outside).expect("resolved target")));
        assert!(fs::symlink_metadata(&path).expect("link").file_type().is_symlink());
    }

    #[test]
    fn an_opencode_destination_stays_owned_when_the_write_method_changes() {
        let dir = tempfile::tempdir().expect("tmp");
        let env = environment(dir.path());
        let method = Method::Open { target: Target::OpenCode, file: file(Target::OpenCode, &env), executable: None };
        connect(&env, Target::OpenCode, &method, "c1", &key(), &Never).expect("connect");
        let mut records = load_records(&env).expect("records");
        if let Method::Open { executable, .. } = &mut records.written[0].method {
            *executable = Some(dir.path().join("opencode"));
        }
        save_records(&env, &records).expect("simulate CLI writer of the same entry");
        let new = IssuedKey::new(Key::new("torro_new_secret"));
        connect(&env, Target::OpenCode, &method, "c2", &new, &Never).expect("same destination remains ours");
        assert_eq!(load_records(&env).expect("records").written.len(), 1);
        assert_eq!(disconnect(&env, "c1", &Never).expect("old client"), None);
        assert_eq!(disconnect(&env, "c2", &Never).expect("new client"), Some(Target::OpenCode));
    }
}
