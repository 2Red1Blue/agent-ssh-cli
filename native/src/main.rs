#[cfg(windows)]
use interprocess::local_socket::{
    prelude::*, GenericNamespaced, ListenerOptions, Stream as LocalSocketStream,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ssh2::Session;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};
use url::Url;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CONFIG_DIR: &str = ".agent-ssh-cli";
const DEFAULT_CONFIG_FILE: &str = "config.json";
const DEFAULT_CACHE_TTL_MS: u64 = 180_000;
const DAEMON_START_TIMEOUT_MS: u64 = 3_000;
const DAEMON_REQUEST_TIMEOUT_MS: u64 = 86_400_000;

const HELP_AGENTSSHCLI: &str = r#"
用法:
  agentsshcli list [--config <path>] [--json]
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <command>
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --command <command> [--directory <dir>] [--timeout <ms>]
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <localPath> <remotePath>
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --local <path> --remote <path>
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <remotePath> <localPath>
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --remote <path> --local <path>
  agentsshcli init-config
  agentsshcli help [list|exec|upload|download]
  agentsshcli --help
  agentsshcli --version

说明:
  agent-ssh-cli Rust 原生入口。exec/upload/download 默认使用 daemon 缓存，可通过 --no-cache 直连。
"#;

const HELP_LIST: &str = r#"
用法:
  agentsshcli list [--config <path>] [--json]
  agentsshcli help list
  agentsshcli --version

说明:
  列出当前配置文件中的 SSH 连接。
"#;

const HELP_EXEC: &str = r#"
用法:
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <command>
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --command <command> [--directory <dir>] [--timeout <ms>]
  agentsshcli help exec
  agentsshcli --version

说明:
  在远端执行命令。默认使用 daemon 缓存，可通过 --no-cache 直连。
"#;

const HELP_UPLOAD: &str = r#"
用法:
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <localPath> <remotePath>
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --local <path> --remote <path>
  agentsshcli help upload
  agentsshcli --version

说明:
  上传本地文件到远端。默认使用 daemon 缓存，可通过 --no-cache 直连。
"#;

const HELP_DOWNLOAD: &str = r#"
用法:
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <remotePath> <localPath>
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --remote <path> --local <path>
  agentsshcli help download
  agentsshcli --version

说明:
  下载远端文件到本地。默认使用 daemon 缓存，可通过 --no-cache 直连。
"#;

#[derive(Debug, Clone)]
struct AppError(String);

type AppResult<T> = Result<T, AppError>;

impl AppError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<ssh2::Error> for AppError {
    fn from(error: ssh2::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<url::ParseError> for AppError {
    fn from(error: url::ParseError) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConnection {
    name: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    username: Option<String>,
    password: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
    agent: Option<String>,
    socks_proxy: Option<String>,
    pty: Option<bool>,
    allowed_local_paths: Option<Vec<String>>,
    command_whitelist: Option<Vec<String>>,
    command_blacklist: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct PatternRule {
    regex: Regex,
}

#[derive(Debug, Clone)]
struct Connection {
    name: String,
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
    agent: Option<String>,
    socks_proxy: Option<String>,
    pty: Option<bool>,
    allowed_local_paths: Vec<String>,
    command_whitelist: Vec<PatternRule>,
    command_blacklist: Vec<PatternRule>,
}

#[derive(Debug)]
struct GlobalArgs {
    config_path: PathBuf,
    help: bool,
    version: bool,
    no_cache: bool,
    cache_ttl_ms: Option<u64>,
    args: Vec<String>,
}

#[derive(Debug)]
struct ExecuteArgs {
    global: GlobalArgs,
    connection_name: String,
    command: String,
    directory: Option<String>,
    timeout_ms: u64,
}

#[derive(Debug)]
struct TransferArgs {
    global: GlobalArgs,
    connection_name: String,
    local_path: String,
    remote_path: String,
}

#[derive(Debug)]
struct SocksProxy {
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

fn main() {
    if let Err(error) = run(env::args().skip(1).collect()) {
        eprintln!("{}", error);
        process::exit(1);
    }
}

fn run(argv: Vec<String>) -> AppResult<()> {
    let Some((command, args)) = argv.split_first() else {
        print_help("agentsshcli")?;
        return Ok(());
    };
    match command.as_str() {
        "--help" | "-h" => print_help("agentsshcli"),
        "--version" | "-v" | "version" => print_version(),
        "help" => print_help(args.first().map(String::as_str).unwrap_or("agentsshcli")),
        "init-config" => init_config(),
        "list" => run_list(args.to_vec()),
        "exec" => run_exec(args.to_vec()),
        "upload" => run_upload(args.to_vec()),
        "download" => run_download(args.to_vec()),
        "__daemon" => run_daemon(args.to_vec()),
        _ => Err(AppError::new(format!(
            "未知命令: {}，使用 agentsshcli --help 查看说明",
            command
        ))),
    }
}

fn print_version() -> AppResult<()> {
    println!("{}", VERSION);
    Ok(())
}

fn print_help(name: &str) -> AppResult<()> {
    let help = match name {
        "agentsshcli" => HELP_AGENTSSHCLI,
        "list" | "sshls" => HELP_LIST,
        "exec" | "sshx" => HELP_EXEC,
        "upload" | "sshupload" => HELP_UPLOAD,
        "download" | "sshdownload" => HELP_DOWNLOAD,
        _ => return Err(AppError::new(format!("未知帮助命令: {}", name))),
    };
    println!("{}", help.trim());
    Ok(())
}

fn default_config_path() -> PathBuf {
    if let Ok(value) = env::var("AGENT_SSH_CONFIG") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DEFAULT_CONFIG_DIR)
        .join(DEFAULT_CONFIG_FILE)
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

fn project_root() -> AppResult<PathBuf> {
    let exe = env::current_exe()?;
    let mut current = exe.parent();
    while let Some(dir) = current {
        if dir.join("package.json").exists() && dir.join("example.config.json").exists() {
            return Ok(dir.to_path_buf());
        }
        current = dir.parent();
    }
    Ok(env::current_dir()?)
}

fn init_config() -> AppResult<()> {
    let target = default_config_path();
    if target.exists() {
        return Err(AppError::new(format!(
            "{} 已存在，未覆盖",
            target.display()
        )));
    }
    let source = project_root()?.join("example.config.json");
    fs::create_dir_all(
        target
            .parent()
            .ok_or_else(|| AppError::new("默认配置路径缺少父目录"))?,
    )?;
    fs::copy(&source, &target).map_err(|error| {
        AppError::new(format!(
            "复制默认配置失败: {} -> {}，{}",
            source.display(),
            target.display(),
            error
        ))
    })?;
    println!("已创建 {}", target.display());
    Ok(())
}

fn is_non_empty(value: &Option<String>) -> bool {
    value.as_ref().is_some_and(|item| !item.trim().is_empty())
}

fn ensure_string_array(
    values: Option<Vec<String>>,
    field_name: &str,
    index: usize,
) -> AppResult<Vec<String>> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|value| {
            if value.trim().is_empty() {
                return Err(AppError::new(format!(
                    "ssh-config.json 第 {} 项的 {} 必须只包含非空字符串",
                    index + 1,
                    field_name
                )));
            }
            Ok(value)
        })
        .collect()
}

fn ensure_regex_array(
    values: Option<Vec<String>>,
    field_name: &str,
    index: usize,
) -> AppResult<Vec<PatternRule>> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|pattern| {
            if pattern.trim().is_empty() {
                return Err(AppError::new(format!(
                    "ssh-config.json 第 {} 项的 {} 必须只包含非空字符串",
                    index + 1,
                    field_name
                )));
            }
            let regex = Regex::new(&pattern).map_err(|error| {
                AppError::new(format!(
                    "ssh-config.json 第 {} 项的 {} 含有非法正则: {}，{}",
                    index + 1,
                    field_name,
                    pattern,
                    error
                ))
            })?;
            Ok(PatternRule { regex })
        })
        .collect()
}

fn normalize_entry(entry: RawConnection, index: usize) -> AppResult<Connection> {
    let name = entry
        .name
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(format!(
                "ssh-config.json 第 {} 项缺少合法的 name",
                index + 1
            ))
        })?;
    let host = entry
        .host
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(format!(
                "ssh-config.json 第 {} 项缺少合法的 host",
                index + 1
            ))
        })?;
    let username = entry
        .username
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            AppError::new(format!(
                "ssh-config.json 第 {} 项缺少合法的 username",
                index + 1
            ))
        })?;
    let port = entry.port.unwrap_or(22);
    if port == 0 {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 port 非法",
            index + 1
        )));
    }
    let has_password = is_non_empty(&entry.password);
    let has_private_key = is_non_empty(&entry.private_key);
    let has_agent = is_non_empty(&entry.agent);
    let auth_count = [has_password, has_private_key, has_agent]
        .iter()
        .filter(|item| **item)
        .count();
    if auth_count == 0 {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项必须配置 password、privateKey 或 agent 其中之一",
            index + 1
        )));
    }
    if auth_count > 1 {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项同时配置了多个认证方式，只允许保留一种",
            index + 1
        )));
    }
    if entry
        .passphrase
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 passphrase 必须是非空字符串",
            index + 1
        )));
    }
    if entry
        .socks_proxy
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 socksProxy 必须是非空字符串",
            index + 1
        )));
    }
    Ok(Connection {
        name,
        host,
        port,
        username,
        password: entry.password.filter(|_| has_password),
        private_key: entry.private_key.filter(|_| has_private_key),
        passphrase: entry.passphrase,
        agent: entry.agent.filter(|_| has_agent),
        socks_proxy: entry.socks_proxy,
        pty: entry.pty,
        allowed_local_paths: ensure_string_array(
            entry.allowed_local_paths,
            "allowedLocalPaths",
            index,
        )?,
        command_whitelist: ensure_regex_array(entry.command_whitelist, "commandWhitelist", index)?,
        command_blacklist: ensure_regex_array(entry.command_blacklist, "commandBlacklist", index)?,
    })
}

fn load_config(config_path: &Path) -> AppResult<Vec<Connection>> {
    let raw = fs::read_to_string(config_path)?;
    let parsed: Vec<RawConnection> = serde_json::from_str(&raw)
        .map_err(|error| AppError::new(format!("ssh-config.json 解析失败: {}", error)))?;
    if parsed.is_empty() {
        return Err(AppError::new("ssh-config.json 不能为空"));
    }
    let configs: Vec<Connection> = parsed
        .into_iter()
        .enumerate()
        .map(|(index, item)| normalize_entry(item, index))
        .collect::<AppResult<Vec<_>>>()?;
    let mut seen = HashSet::new();
    for config in &configs {
        if !seen.insert(config.name.clone()) {
            return Err(AppError::new(format!(
                "ssh-config.json 存在重复的连接名: {}",
                config.name
            )));
        }
    }
    Ok(configs)
}

fn find_connection<'a>(
    configs: &'a [Connection],
    connection_name: &str,
) -> AppResult<&'a Connection> {
    configs
        .iter()
        .find(|item| item.name == connection_name)
        .ok_or_else(|| AppError::new(format!("未找到连接配置: {}", connection_name)))
}

fn path_absolute(path: &Path) -> AppResult<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

fn canonical_or_absolute(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn validate_local_path(
    configs: &[Connection],
    local_path: &str,
    base_cwd: &Path,
) -> AppResult<PathBuf> {
    let resolved_cwd = canonical_or_absolute(base_cwd.to_path_buf());
    let candidate = Path::new(local_path);
    let resolved_path = if candidate.is_absolute() {
        canonical_or_absolute(candidate.to_path_buf())
    } else {
        canonical_or_absolute(resolved_cwd.join(candidate))
    };
    let mut allowed_roots = vec![resolved_cwd, project_root()?];
    for config in configs {
        for allowed_path in &config.allowed_local_paths {
            allowed_roots.push(canonical_or_absolute(PathBuf::from(allowed_path)));
        }
    }
    if allowed_roots
        .iter()
        .any(|root| resolved_path == *root || resolved_path.starts_with(root))
    {
        return Ok(resolved_path);
    }
    Err(AppError::new(
        "本地路径不允许访问，必须位于当前工作目录、项目目录或显式允许的路径内",
    ))
}

fn parse_global_args(argv: Vec<String>) -> AppResult<GlobalArgs> {
    let mut args = argv.into_iter().peekable();
    let mut config_path = default_config_path();
    let mut help = false;
    let mut version = false;
    let mut no_cache = false;
    let mut cache_ttl_ms = None;
    let mut remaining = Vec::new();
    while let Some(current) = args.next() {
        match current.as_str() {
            "--help" | "-h" => help = true,
            "--version" | "-v" => version = true,
            "--json" => {}
            "--no-cache" => no_cache = true,
            "--cache-ttl" => {
                let value = args
                    .next()
                    .ok_or_else(|| AppError::new("--cache-ttl 缺少毫秒值"))?;
                let ttl = normalize_positive_u64(&value, "cache-ttl 必须是正整数毫秒值")?;
                cache_ttl_ms = Some(ttl);
            }
            "--config" => {
                let value = args
                    .next()
                    .ok_or_else(|| AppError::new("--config 缺少路径"))?;
                config_path = PathBuf::from(value);
            }
            _ => {
                remaining.push(current);
                remaining.extend(args);
                break;
            }
        }
    }
    Ok(GlobalArgs {
        config_path,
        help,
        version,
        no_cache,
        cache_ttl_ms,
        args: remaining,
    })
}

fn normalize_positive_u64(value: &str, message: &str) -> AppResult<u64> {
    let parsed = value.parse::<u64>().map_err(|_| AppError::new(message))?;
    if parsed == 0 {
        return Err(AppError::new(message));
    }
    Ok(parsed)
}

fn take_option(args: &mut Vec<String>, names: &[&str]) -> AppResult<Option<String>> {
    let indexes: Vec<usize> = args
        .iter()
        .enumerate()
        .filter_map(|(index, item)| names.contains(&item.as_str()).then_some(index))
        .collect();
    if indexes.len() > 1 {
        return Err(AppError::new(format!("参数重复声明: {}", names[0])));
    }
    let Some(index) = indexes.first().copied() else {
        return Ok(None);
    };
    let Some(value) = args.get(index + 1).cloned() else {
        return Err(AppError::new(format!("{} 缺少参数值", args[index])));
    };
    if value.starts_with("--") {
        return Err(AppError::new(format!("{} 缺少参数值", args[index])));
    }
    args.drain(index..=index + 1);
    Ok(Some(value))
}

fn take_positional(args: &mut Vec<String>, field_name: &str) -> AppResult<Option<String>> {
    if args.is_empty() {
        return Ok(None);
    }
    let value = args.remove(0);
    if value.starts_with("--") {
        return Err(AppError::new(format!(
            "{} 位置参数非法: {}",
            field_name, value
        )));
    }
    Ok(Some(value))
}

fn ensure_no_mixed(
    named: &Option<String>,
    positional: &Option<String>,
    field_name: &str,
) -> AppResult<()> {
    if named.is_some() && positional.is_some() {
        return Err(AppError::new(format!(
            "{} 同时使用了命名参数和位置参数，保留一种即可",
            field_name
        )));
    }
    Ok(())
}

fn ensure_no_unknown_options(args: &[String]) -> AppResult<()> {
    if let Some(unknown) = args.iter().find(|item| item.starts_with("--")) {
        return Err(AppError::new(format!("不支持的参数: {}", unknown)));
    }
    Ok(())
}

fn ensure_no_extra_positionals(args: &[String]) -> AppResult<()> {
    if !args.is_empty() {
        return Err(AppError::new(format!(
            "存在多余的位置参数: {}",
            args.join(" ")
        )));
    }
    Ok(())
}

fn parse_execute_args(argv: Vec<String>) -> AppResult<ExecuteArgs> {
    let global = parse_global_args(argv)?;
    if global.help || global.version {
        return Ok(ExecuteArgs {
            global,
            connection_name: String::new(),
            command: String::new(),
            directory: None,
            timeout_ms: 30000,
        });
    }
    let mut args = global.args.clone();
    let connection_option = take_option(&mut args, &["--connection", "-c"])?;
    let command_option = take_option(&mut args, &["--command"])?;
    let directory = take_option(&mut args, &["--directory", "-d"])?;
    let timeout_value = take_option(&mut args, &["--timeout", "-t"])?;
    let connection_positional = take_positional(&mut args, "connectionName")?;
    let command_positional = take_positional(&mut args, "command")?;
    ensure_no_mixed(&connection_option, &connection_positional, "connectionName")?;
    ensure_no_mixed(&command_option, &command_positional, "command")?;
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let connection_name = connection_option.or(connection_positional).ok_or_else(|| {
        AppError::new("缺少必填参数 connectionName 或 command，使用 --help 查看说明")
    })?;
    let command = command_option.or(command_positional).ok_or_else(|| {
        AppError::new("缺少必填参数 connectionName 或 command，使用 --help 查看说明")
    })?;
    let timeout_ms = match timeout_value {
        Some(value) => normalize_positive_u64(&value, "timeout 必须是正整数毫秒值")?,
        None => 30000,
    };
    Ok(ExecuteArgs {
        global,
        connection_name,
        command,
        directory,
        timeout_ms,
    })
}

fn resolve_value(
    args: &mut Vec<String>,
    names: &[&str],
    field_name: &str,
) -> AppResult<Option<String>> {
    match take_option(args, names)? {
        Some(value) => Ok(Some(value)),
        None => take_positional(args, field_name),
    }
}

fn parse_transfer_args(argv: Vec<String>, mode: &str) -> AppResult<TransferArgs> {
    let global = parse_global_args(argv)?;
    if global.help || global.version {
        return Ok(TransferArgs {
            global,
            connection_name: String::new(),
            local_path: String::new(),
            remote_path: String::new(),
        });
    }
    let mut args = global.args.clone();
    let connection_name = resolve_value(&mut args, &["--connection", "-c"], "connectionName")?;
    let (local_path, remote_path) = if mode == "upload" {
        (
            resolve_value(&mut args, &["--local", "-l"], "localPath")?,
            resolve_value(&mut args, &["--remote", "-r"], "remotePath")?,
        )
    } else {
        let remote = resolve_value(&mut args, &["--remote", "-r"], "remotePath")?;
        let local = resolve_value(&mut args, &["--local", "-l"], "localPath")?;
        (local, remote)
    };
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let Some(connection_name) = connection_name else {
        return Err(AppError::new("缺少必填参数，使用 --help 查看说明"));
    };
    let Some(local_path) = local_path else {
        return Err(AppError::new("缺少必填参数，使用 --help 查看说明"));
    };
    let Some(remote_path) = remote_path else {
        return Err(AppError::new("缺少必填参数，使用 --help 查看说明"));
    };
    Ok(TransferArgs {
        global,
        connection_name,
        local_path,
        remote_path,
    })
}

fn run_list(argv: Vec<String>) -> AppResult<()> {
    let global = parse_global_args(argv)?;
    if global.help {
        return print_help("list");
    }
    if global.version {
        return print_version();
    }
    if !global.args.is_empty() {
        return Err(AppError::new(format!(
            "agentsshcli list 不接受位置参数: {}",
            global.args.join(" ")
        )));
    }
    let configs = load_config(&global.config_path)?;
    let output: Vec<serde_json::Value> = configs
        .iter()
        .map(|item| {
            serde_json::json!({
                "name": item.name,
                "host": item.host,
                "port": item.port,
                "username": item.username,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn run_exec(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_execute_args(argv)?;
    if parsed.global.help {
        return print_help("exec");
    }
    if parsed.global.version {
        return print_version();
    }
    let configs = load_config(&parsed.global.config_path)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    validate_command(connection, &parsed.command)?;
    let remote_command = match parsed.directory {
        Some(ref directory) => format!(
            "cd -- {} && {}",
            shell_json_quote(directory)?,
            parsed.command
        ),
        None => parsed.command.clone(),
    };
    let result = if parsed.global.no_cache {
        execute_remote_command(connection, &remote_command, parsed.timeout_ms)?
    } else {
        request_daemon_execute(&parsed)?
    };
    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
}

fn run_upload(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_transfer_args(argv, "upload")?;
    if parsed.global.help {
        return print_help("upload");
    }
    if parsed.global.version {
        return print_version();
    }
    let configs = load_config(&parsed.global.config_path)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    if parsed.global.no_cache {
        let local_path = validate_local_path(&configs, &parsed.local_path, &env::current_dir()?)?;
        upload_file(connection, &local_path, &parsed.remote_path)?;
    } else {
        request_daemon_transfer(&parsed, "upload")?;
    }
    println!("File uploaded successfully");
    Ok(())
}

fn run_download(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_transfer_args(argv, "download")?;
    if parsed.global.help {
        return print_help("download");
    }
    if parsed.global.version {
        return print_version();
    }
    let configs = load_config(&parsed.global.config_path)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    if parsed.global.no_cache {
        let local_path = validate_local_path(&configs, &parsed.local_path, &env::current_dir()?)?;
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent)?;
        }
        download_file(connection, &parsed.remote_path, &local_path)?;
    } else {
        request_daemon_transfer(&parsed, "download")?;
    }
    println!("File downloaded successfully");
    Ok(())
}

fn validate_command(connection: &Connection, command: &str) -> AppResult<()> {
    if !connection.command_whitelist.is_empty()
        && !connection
            .command_whitelist
            .iter()
            .any(|item| item.regex.is_match(command))
    {
        return Err(AppError::new("命令未命中白名单，拒绝执行"));
    }
    if connection
        .command_blacklist
        .iter()
        .any(|item| item.regex.is_match(command))
    {
        return Err(AppError::new("命令命中黑名单，拒绝执行"));
    }
    Ok(())
}

fn shell_json_quote(value: &str) -> AppResult<String> {
    Ok(serde_json::to_string(value)?)
}

fn parse_socks_proxy(proxy: &str) -> AppResult<SocksProxy> {
    let value = if proxy.contains("://") {
        proxy.to_string()
    } else {
        format!("socks5://{}", proxy)
    };
    let parsed = Url::parse(&value)
        .map_err(|error| AppError::new(format!("socksProxy 格式非法: {}，{}", proxy, error)))?;
    if parsed.scheme() != "socks5" {
        return Err(AppError::new("socksProxy 仅支持 socks5:// 协议"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| AppError::new("socksProxy 必须包含代理主机和端口"))?
        .to_string();
    let port = parsed
        .port()
        .ok_or_else(|| AppError::new("socksProxy 必须包含代理主机和端口"))?;
    let username = (!parsed.username().is_empty()).then(|| parsed.username().to_string());
    let password = parsed.password().map(ToString::to_string);
    if username.is_some() != password.is_some() {
        return Err(AppError::new("socksProxy 用户名和密码必须同时提供"));
    }
    Ok(SocksProxy {
        host,
        port,
        username,
        password,
    })
}

fn read_exact(stream: &mut TcpStream, length: usize) -> AppResult<Vec<u8>> {
    let mut buffer = vec![0_u8; length];
    stream.read_exact(&mut buffer)?;
    Ok(buffer)
}

fn authenticate_socks_proxy(stream: &mut TcpStream, proxy: &SocksProxy) -> AppResult<()> {
    let method = if proxy.username.is_some() { 0x02 } else { 0x00 };
    stream.write_all(&[0x05, 0x01, method])?;
    let response = read_exact(stream, 2)?;
    if response[0] != 0x05 {
        return Err(AppError::new("SOCKS5 代理响应版本非法"));
    }
    if response[1] == 0xff {
        return Err(AppError::new("SOCKS5 代理不接受当前认证方式"));
    }
    if response[1] == 0x00 {
        return Ok(());
    }
    if response[1] != 0x02 || proxy.username.is_none() {
        return Err(AppError::new("SOCKS5 代理返回了不支持的认证方式"));
    }
    let username = proxy.username.as_deref().unwrap_or_default().as_bytes();
    let password = proxy.password.as_deref().unwrap_or_default().as_bytes();
    if username.len() > 255 || password.len() > 255 {
        return Err(AppError::new("SOCKS5 用户名或密码过长"));
    }
    let mut request = Vec::with_capacity(3 + username.len() + password.len());
    request.push(0x01);
    request.push(username.len() as u8);
    request.extend_from_slice(username);
    request.push(password.len() as u8);
    request.extend_from_slice(password);
    stream.write_all(&request)?;
    let auth_response = read_exact(stream, 2)?;
    if auth_response[1] != 0x00 {
        return Err(AppError::new("SOCKS5 代理认证失败"));
    }
    Ok(())
}

fn encode_target_address(host: &str) -> AppResult<Vec<u8>> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(match ip {
            IpAddr::V4(addr) => {
                let mut bytes = vec![0x01];
                bytes.extend_from_slice(&addr.octets());
                bytes
            }
            IpAddr::V6(addr) => {
                let mut bytes = vec![0x04];
                bytes.extend_from_slice(&addr.octets());
                bytes
            }
        });
    }
    let host_bytes = host.as_bytes();
    if host_bytes.len() > 255 {
        return Err(AppError::new("SOCKS5 目标主机名过长"));
    }
    let mut bytes = vec![0x03, host_bytes.len() as u8];
    bytes.extend_from_slice(host_bytes);
    Ok(bytes)
}

fn read_socks_connect_response(stream: &mut TcpStream) -> AppResult<()> {
    let header = read_exact(stream, 4)?;
    if header[0] != 0x05 {
        return Err(AppError::new("SOCKS5 代理响应版本非法"));
    }
    if header[1] != 0x00 {
        return Err(AppError::new(format!(
            "SOCKS5 代理连接目标失败，响应码 {}",
            header[1]
        )));
    }
    if header[2] != 0x00 {
        return Err(AppError::new("SOCKS5 代理响应保留字段非法"));
    }
    match header[3] {
        0x01 => {
            read_exact(stream, 4)?;
        }
        0x04 => {
            read_exact(stream, 16)?;
        }
        0x03 => {
            let len = read_exact(stream, 1)?[0] as usize;
            read_exact(stream, len)?;
        }
        _ => return Err(AppError::new("SOCKS5 代理响应地址类型非法")),
    }
    read_exact(stream, 2)?;
    Ok(())
}

fn connect_socks_proxy(connection: &Connection) -> AppResult<TcpStream> {
    let proxy = parse_socks_proxy(
        connection
            .socks_proxy
            .as_deref()
            .ok_or_else(|| AppError::new("缺少 socksProxy 配置"))?,
    )?;
    let mut stream = TcpStream::connect((proxy.host.as_str(), proxy.port))?;
    authenticate_socks_proxy(&mut stream, &proxy)?;
    let mut request = vec![0x05, 0x01, 0x00];
    request.extend_from_slice(&encode_target_address(&connection.host)?);
    request.extend_from_slice(&connection.port.to_be_bytes());
    stream.write_all(&request)?;
    read_socks_connect_response(&mut stream)?;
    Ok(stream)
}

fn connect_ssh(connection: &Connection) -> AppResult<Session> {
    let tcp = if connection.socks_proxy.is_some() {
        connect_socks_proxy(connection)?
    } else {
        TcpStream::connect((connection.host.as_str(), connection.port))?
    };
    tcp.set_read_timeout(Some(Duration::from_secs(60)))?;
    tcp.set_write_timeout(Some(Duration::from_secs(60)))?;
    let mut session = Session::new()?;
    session.set_tcp_stream(tcp);
    session.handshake()?;
    if let Some(password) = &connection.password {
        session.userauth_password(&connection.username, password)?;
    } else if let Some(private_key) = &connection.private_key {
        session.userauth_pubkey_file(
            &connection.username,
            None,
            Path::new(private_key),
            connection.passphrase.as_deref(),
        )?;
    } else if let Some(agent_path) = &connection.agent {
        env::set_var("SSH_AUTH_SOCK", agent_path);
        let mut agent = session.agent()?;
        agent.connect()?;
        agent.list_identities()?;
        let identities = agent.identities()?;
        let mut authenticated = false;
        let mut last_error: Option<String> = None;
        for identity in identities {
            match agent.userauth(&connection.username, &identity) {
                Ok(()) => {
                    authenticated = true;
                    break;
                }
                Err(error) => last_error = Some(error.to_string()),
            }
        }
        if !authenticated {
            return Err(AppError::new(format!(
                "SSH agent 认证失败{}",
                last_error
                    .map(|item| format!(": {}", item))
                    .unwrap_or_default()
            )));
        }
    } else {
        return Err(AppError::new(format!(
            "连接 {} 缺少可用认证信息",
            connection.name
        )));
    }
    if !session.authenticated() {
        return Err(AppError::new("SSH 认证失败"));
    }
    Ok(session)
}

fn execute_remote_command(
    connection: &Connection,
    remote_command: &str,
    timeout_ms: u64,
) -> AppResult<String> {
    let session = connect_ssh(connection)?;
    // libssh2 的阻塞调用超时用于约束远端命令读写等待，避免命令长期挂起。
    session.set_timeout(timeout_ms.try_into().unwrap_or(u32::MAX));
    let mut channel = session.channel_session()?;
    if connection.pty.unwrap_or(true) {
        channel.request_pty("xterm", None, None)?;
    }
    channel.exec(remote_command)?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    channel.read_to_string(&mut stdout)?;
    channel.stderr().read_to_string(&mut stderr)?;
    channel.wait_close()?;
    let exit_status = channel.exit_status()?;
    if exit_status != 0 {
        let mut parts = Vec::new();
        if !stdout.trim().is_empty() {
            parts.push(stdout.trim_end().to_string());
        }
        if !stderr.trim().is_empty() {
            parts.push(format!("[stderr]\n{}", stderr.trim_end()));
        }
        parts.push(format!("[exit code] {}", exit_status));
        return Err(AppError::new(parts.join("\n")));
    }
    Ok(stdout.trim_end().to_string())
}

fn upload_file(connection: &Connection, local_path: &Path, remote_path: &str) -> AppResult<()> {
    let session = connect_ssh(connection)?;
    let sftp = session.sftp()?;
    let mut local_file = fs::File::open(local_path)?;
    let mut remote_file = sftp.create(Path::new(remote_path))?;
    std::io::copy(&mut local_file, &mut remote_file)?;
    Ok(())
}

fn download_file(connection: &Connection, remote_path: &str, local_path: &Path) -> AppResult<()> {
    let session = connect_ssh(connection)?;
    let sftp = session.sftp()?;
    let mut remote_file = sftp.open(Path::new(remote_path))?;
    let mut local_file = fs::File::create(local_path)?;
    std::io::copy(&mut remote_file, &mut local_file)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_config(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn load_config_validates_duplicate_names() {
        let (_dir, path) = write_config(
            r#"[
              {"name":"a","host":"127.0.0.1","username":"root","password":"p"},
              {"name":"a","host":"127.0.0.2","username":"root","password":"p"}
            ]"#,
        );
        let err = load_config(&path).unwrap_err();
        assert!(err.to_string().contains("重复的连接名"));
    }

    #[test]
    fn command_blacklist_blocks_matching_command() {
        let connection = normalize_entry(
            serde_json::from_str(
                r#"{"name":"a","host":"127.0.0.1","username":"root","password":"p","commandBlacklist":["(^|[;&|()\\s])rm(\\s|$)"]}"#,
            )
            .unwrap(),
            0,
        )
        .unwrap();
        assert!(validate_command(&connection, "rm -rf /tmp/a").is_err());
        assert!(validate_command(&connection, "pwd").is_ok());
    }

    #[test]
    fn parse_exec_allows_cache_mode() {
        let parsed = parse_execute_args(vec!["server".into(), "pwd".into()]).unwrap();
        assert!(!parsed.global.no_cache);
        assert_eq!(parsed.connection_name, "server");
        assert_eq!(parsed.command, "pwd");
    }

    #[test]
    fn parse_exec_supports_named_arguments() {
        let parsed = parse_execute_args(vec![
            "--no-cache".into(),
            "--connection".into(),
            "server".into(),
            "--command".into(),
            "pwd".into(),
            "--timeout".into(),
            "1000".into(),
        ])
        .unwrap();
        assert_eq!(parsed.connection_name, "server");
        assert_eq!(parsed.command, "pwd");
        assert_eq!(parsed.timeout_ms, 1000);
    }

    #[test]
    fn socks_proxy_supports_host_port_without_scheme() {
        let proxy = parse_socks_proxy("127.0.0.1:1080").unwrap();
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 1080);
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonRequest {
    operation: String,
    config_path: PathBuf,
    cwd: PathBuf,
    connection_name: String,
    command: Option<String>,
    directory: Option<String>,
    timeout: Option<u64>,
    local_path: Option<String>,
    remote_path: Option<String>,
    cache_ttl_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct DaemonResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stdout: Option<String>,
}

struct PoolEntry {
    session: Session,
    last_used_at: Instant,
    ttl_ms: u64,
}

fn cache_ttl(global: &GlobalArgs) -> u64 {
    global.cache_ttl_ms.unwrap_or(DEFAULT_CACHE_TTL_MS)
}

fn request_daemon_execute(parsed: &ExecuteArgs) -> AppResult<String> {
    let config_path = path_absolute(&parsed.global.config_path)?;
    let request = serde_json::json!({
        "operation": "execute",
        "configPath": config_path,
        "cwd": env::current_dir()?,
        "connectionName": parsed.connection_name,
        "command": parsed.command,
        "directory": parsed.directory,
        "timeout": parsed.timeout_ms,
        "cacheTtlMs": cache_ttl(&parsed.global),
    });
    let response = request_daemon(&config_path, &request)?;
    Ok(response.stdout.unwrap_or_default())
}

fn request_daemon_transfer(parsed: &TransferArgs, operation: &str) -> AppResult<()> {
    let config_path = path_absolute(&parsed.global.config_path)?;
    let request = serde_json::json!({
        "operation": operation,
        "configPath": config_path,
        "cwd": env::current_dir()?,
        "connectionName": parsed.connection_name,
        "localPath": parsed.local_path,
        "remotePath": parsed.remote_path,
        "cacheTtlMs": cache_ttl(&parsed.global),
    });
    request_daemon(&config_path, &request)?;
    Ok(())
}

fn request_daemon(config_path: &Path, request: &serde_json::Value) -> AppResult<DaemonResponse> {
    let socket_path = get_socket_path(config_path)?;
    ensure_daemon(&socket_path, config_path)?;
    let mut stream = connect_socket(&socket_path, DAEMON_REQUEST_TIMEOUT_MS)?;
    let line = format!("{}\n", serde_json::to_string(request)?);
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    let response_line = read_line_from_socket(&mut stream)?;
    if response_line.is_empty() {
        unlink_socket_path(&socket_path)?;
        ensure_daemon(&socket_path, config_path)?;
        let mut retry_stream = connect_socket(&socket_path, DAEMON_REQUEST_TIMEOUT_MS)?;
        retry_stream.write_all(line.as_bytes())?;
        retry_stream.flush()?;
        let retry_line = read_line_from_socket(&mut retry_stream)?;
        if retry_line.is_empty() {
            return Err(AppError::new("SSH 缓存进程提前关闭连接"));
        }
        let response: DaemonResponse = serde_json::from_str(&retry_line)
            .map_err(|error| AppError::new(format!("SSH 缓存进程响应非法: {}", error)))?;
        if !response.ok {
            return Err(AppError::new(
                response
                    .message
                    .unwrap_or_else(|| "SSH 缓存进程执行失败".to_string()),
            ));
        }
        return Ok(response);
    }
    let response: DaemonResponse = serde_json::from_str(&response_line)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应非法: {}", error)))?;
    if !response.ok {
        return Err(AppError::new(
            response
                .message
                .unwrap_or_else(|| "SSH 缓存进程执行失败".to_string()),
        ));
    }
    Ok(response)
}

fn ensure_daemon(socket_path: &Path, config_path: &Path) -> AppResult<()> {
    match connect_socket(socket_path, 500) {
        Ok(mut stream) => {
            let _ = stream.write_all(b"{\"operation\":\"ping\"}\n");
            match read_line_from_socket(&mut stream) {
                Ok(line) if !line.is_empty() => return Ok(()),
                _ => unlink_socket_path(socket_path)?,
            }
        }
        Err(_) => unlink_socket_path(socket_path)?,
    }
    spawn_daemon(socket_path, config_path)?;
    wait_for_daemon(socket_path)
}

fn spawn_daemon(socket_path: &Path, config_path: &Path) -> AppResult<()> {
    let exe = env::current_exe()?;
    let mut command = Command::new(exe);
    command
        .arg("__daemon")
        .arg("--socket")
        .arg(socket_path)
        .arg("--config")
        .arg(config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir(project_root()?);
    command.spawn()?;
    Ok(())
}

fn wait_for_daemon(socket_path: &Path) -> AppResult<()> {
    let start = Instant::now();
    let mut last_error = None;
    while start.elapsed() < Duration::from_millis(DAEMON_START_TIMEOUT_MS) {
        match connect_socket(socket_path, 500).and_then(|mut stream| {
            stream.write_all(b"{\"operation\":\"ping\"}\n")?;
            stream.flush()?;
            let line = read_line_from_socket(&mut stream)?;
            if line.is_empty() {
                Err(AppError::new("SSH 缓存进程提前关闭连接"))
            } else {
                Ok(())
            }
        }) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = Some(error.to_string());
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(AppError::new(format!(
        "启动 SSH 缓存进程失败: {}",
        last_error.unwrap_or_else(|| "未知错误".to_string())
    )))
}

fn get_daemon_dir() -> AppResult<PathBuf> {
    #[cfg(unix)]
    let uid = unsafe { libc::getuid() }.to_string();
    #[cfg(not(unix))]
    let uid = "nouid".to_string();
    let dir = env::temp_dir().join(format!("agent-ssh-cli-{}", uid));
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

fn get_socket_path(config_path: &Path) -> AppResult<PathBuf> {
    let resolved = path_absolute(config_path)?;
    let mut hasher = Sha256::new();
    hasher.update(resolved.to_string_lossy().as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    #[cfg(windows)]
    {
        let user_key = env::var("USERPROFILE")
            .or_else(|_| env::var("USERNAME"))
            .unwrap_or_else(|_| {
                home_dir()
                    .unwrap_or_else(|| PathBuf::from("nouser"))
                    .display()
                    .to_string()
            });
        let mut user_hasher = Sha256::new();
        user_hasher.update(user_key.as_bytes());
        let user_digest = format!("{:x}", user_hasher.finalize());
        return Ok(PathBuf::from(format!(
            "agent-ssh-cli-{}-{}",
            &user_digest[..12],
            &digest[..24]
        )));
    }
    #[cfg(unix)]
    {
        Ok(get_daemon_dir()?.join(format!("{}.sock", &digest[..24])))
    }
}

fn unlink_socket_path(socket_path: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        let _ = socket_path;
        return Ok(());
    }
    #[cfg(unix)]
    match fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(unix)]
fn connect_socket(socket_path: &Path, timeout_ms: u64) -> AppResult<UnixStream> {
    let stream = UnixStream::connect(socket_path)?;
    let timeout = Some(Duration::from_millis(timeout_ms));
    stream.set_read_timeout(timeout)?;
    stream.set_write_timeout(timeout)?;
    Ok(stream)
}

#[cfg(windows)]
fn connect_socket(socket_path: &Path, _timeout_ms: u64) -> AppResult<LocalSocketStream> {
    let pipe_name = windows_pipe_name_from_path(socket_path);
    let name = pipe_name
        .as_str()
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| AppError::new(format!("Windows named pipe 名称非法: {}", error)))?;
    LocalSocketStream::connect(name).map_err(|error| AppError::new(error.to_string()))
}

#[cfg(windows)]
fn windows_pipe_name_from_path(socket_path: &Path) -> String {
    socket_path
        .to_string_lossy()
        .replace('\\', "-")
        .replace(':', "")
        .replace('/', "-")
}

fn read_line_from_socket<S: Read>(stream: &mut S) -> AppResult<String> {
    let mut bytes = Vec::new();
    let mut one = [0_u8; 1];
    loop {
        let count = stream.read(&mut one)?;
        if count == 0 {
            break;
        }
        if one[0] == b'\n' {
            break;
        }
        bytes.push(one[0]);
    }
    String::from_utf8(bytes)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应非法: {}", error)))
}

#[cfg(unix)]
fn run_daemon(argv: Vec<String>) -> AppResult<()> {
    let (socket_path, config_path) = parse_daemon_args(argv)?;
    unlink_socket_path(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    let bound_config_path = path_absolute(&config_path)?;
    let mut connections: HashMap<String, PoolEntry> = HashMap::new();
    let mut last_activity_at = Instant::now();
    loop {
        let wait_ms = next_daemon_wait_ms(&connections, last_activity_at);
        listener.set_nonblocking(true)?;
        match listener.accept() {
            Ok((mut stream, _)) => {
                last_activity_at = Instant::now();
                let response =
                    match handle_daemon_stream(&mut stream, &bound_config_path, &mut connections) {
                        Ok(response) => response,
                        Err(error) => DaemonResponse {
                            ok: false,
                            message: Some(error.to_string()),
                            stdout: None,
                        },
                    };
                let line = format!("{}\n", serde_json::to_string(&response)?);
                stream.write_all(line.as_bytes())?;
                stream.flush()?;
                expire_connections(&mut connections);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(wait_ms.min(100)));
                expire_connections(&mut connections);
                if connections.is_empty()
                    && last_activity_at.elapsed() >= Duration::from_millis(DEFAULT_CACHE_TTL_MS)
                {
                    break;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    unlink_socket_path(&socket_path)?;
    Ok(())
}

#[cfg(windows)]
fn run_daemon(argv: Vec<String>) -> AppResult<()> {
    let (socket_path, config_path) = parse_daemon_args(argv)?;
    let pipe_name = windows_pipe_name_from_path(&socket_path);
    let name = pipe_name
        .as_str()
        .to_ns_name::<GenericNamespaced>()
        .map_err(|error| AppError::new(format!("Windows named pipe 名称非法: {}", error)))?;
    let listener = ListenerOptions::new().name(name).create_sync()?;
    let bound_config_path = path_absolute(&config_path)?;
    let mut connections: HashMap<String, PoolEntry> = HashMap::new();
    let mut last_activity_at = Instant::now();
    loop {
        match listener.accept() {
            Ok(mut stream) => {
                last_activity_at = Instant::now();
                let response =
                    match handle_daemon_stream(&mut stream, &bound_config_path, &mut connections) {
                        Ok(response) => response,
                        Err(error) => DaemonResponse {
                            ok: false,
                            message: Some(error.to_string()),
                            stdout: None,
                        },
                    };
                let line = format!("{}\n", serde_json::to_string(&response)?);
                stream.write_all(line.as_bytes())?;
                stream.flush()?;
                expire_connections(&mut connections);
            }
            Err(error) => return Err(AppError::new(error.to_string())),
        }
        if connections.is_empty()
            && last_activity_at.elapsed() >= Duration::from_millis(DEFAULT_CACHE_TTL_MS)
        {
            break;
        }
    }
    Ok(())
}

fn parse_daemon_args(argv: Vec<String>) -> AppResult<(PathBuf, PathBuf)> {
    let mut socket_path = None;
    let mut config_path = None;
    let mut iter = argv.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--socket" => socket_path = iter.next().map(PathBuf::from),
            "--config" => config_path = iter.next().map(PathBuf::from),
            _ => {}
        }
    }
    let socket_path = socket_path.ok_or_else(|| AppError::new("daemon 缺少 --socket 参数"))?;
    let config_path = config_path.ok_or_else(|| AppError::new("daemon 缺少 --config 参数"))?;
    Ok((socket_path, config_path))
}

fn next_daemon_wait_ms(connections: &HashMap<String, PoolEntry>, last_activity_at: Instant) -> u64 {
    if connections.is_empty() {
        return DEFAULT_CACHE_TTL_MS
            .saturating_sub(last_activity_at.elapsed().as_millis() as u64)
            .max(100);
    }
    connections
        .values()
        .map(|entry| {
            entry
                .ttl_ms
                .saturating_sub(entry.last_used_at.elapsed().as_millis() as u64)
                .max(100)
        })
        .min()
        .unwrap_or(DEFAULT_CACHE_TTL_MS)
}

fn expire_connections(connections: &mut HashMap<String, PoolEntry>) {
    let expired: Vec<String> = connections
        .iter()
        .filter_map(|(key, entry)| {
            (entry.last_used_at.elapsed() >= Duration::from_millis(entry.ttl_ms))
                .then(|| key.clone())
        })
        .collect();
    for key in expired {
        connections.remove(&key);
    }
}

fn handle_daemon_stream<S: Read + Write>(
    stream: &mut S,
    bound_config_path: &Path,
    connections: &mut HashMap<String, PoolEntry>,
) -> AppResult<DaemonResponse> {
    let line = read_line_from_socket(stream)?;
    let raw_value: serde_json::Value = serde_json::from_str(&line)?;
    if raw_value.get("operation").and_then(|item| item.as_str()) == Some("ping") {
        return Ok(DaemonResponse {
            ok: true,
            message: None,
            stdout: None,
        });
    }
    let request: DaemonRequest = serde_json::from_value(raw_value)?;
    let request_config_path = path_absolute(&request.config_path)?;
    if request_config_path != bound_config_path {
        return Err(AppError::new("SSH 缓存进程拒绝访问非绑定配置文件"));
    }
    let ttl_ms = request.cache_ttl_ms.unwrap_or(DEFAULT_CACHE_TTL_MS);
    if ttl_ms == 0 {
        return Err(AppError::new("cache-ttl 必须是正整数毫秒值"));
    }
    let configs = load_config(bound_config_path)?;
    let connection = find_connection(&configs, &request.connection_name)?.clone();
    if request.operation == "execute" {
        let command = request
            .command
            .as_deref()
            .ok_or_else(|| AppError::new("daemon execute 缺少 command"))?;
        validate_command(&connection, command)?;
    }
    let key = build_connection_key(bound_config_path, &connection);
    if !connections.contains_key(&key) {
        connections.insert(
            key.clone(),
            PoolEntry {
                session: connect_ssh(&connection)?,
                last_used_at: Instant::now(),
                ttl_ms,
            },
        );
    }
    let entry = connections
        .get_mut(&key)
        .ok_or_else(|| AppError::new("SSH 缓存连接状态异常"))?;
    entry.ttl_ms = ttl_ms;
    entry.last_used_at = Instant::now();
    let result = match request.operation.as_str() {
        "execute" => {
            let command = request
                .command
                .ok_or_else(|| AppError::new("daemon execute 缺少 command"))?;
            let remote_command = match request.directory {
                Some(directory) => {
                    format!("cd -- {} && {}", shell_json_quote(&directory)?, command)
                }
                None => command,
            };
            let stdout = execute_remote_command_with_session(
                &entry.session,
                &connection,
                &remote_command,
                request.timeout.unwrap_or(30000),
            )?;
            DaemonResponse {
                ok: true,
                message: None,
                stdout: Some(stdout),
            }
        }
        "upload" => {
            let local = request
                .local_path
                .ok_or_else(|| AppError::new("daemon upload 缺少 localPath"))?;
            let remote = request
                .remote_path
                .ok_or_else(|| AppError::new("daemon upload 缺少 remotePath"))?;
            let local_path = validate_local_path(&configs, &local, &request.cwd)?;
            upload_file_with_session(&entry.session, &local_path, &remote)?;
            DaemonResponse {
                ok: true,
                message: None,
                stdout: None,
            }
        }
        "download" => {
            let local = request
                .local_path
                .ok_or_else(|| AppError::new("daemon download 缺少 localPath"))?;
            let remote = request
                .remote_path
                .ok_or_else(|| AppError::new("daemon download 缺少 remotePath"))?;
            let local_path = validate_local_path(&configs, &local, &request.cwd)?;
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent)?;
            }
            download_file_with_session(&entry.session, &remote, &local_path)?;
            DaemonResponse {
                ok: true,
                message: None,
                stdout: None,
            }
        }
        _ => {
            return Err(AppError::new(format!(
                "不支持的 daemon 操作: {}",
                request.operation
            )))
        }
    };
    entry.last_used_at = Instant::now();
    Ok(result)
}

fn build_connection_key(config_path: &Path, connection: &Connection) -> String {
    let auth = if let Some(agent) = &connection.agent {
        format!("agent:{}", agent)
    } else if let Some(private_key) = &connection.private_key {
        format!("privateKey:{}:{:?}", private_key, connection.passphrase)
    } else {
        format!("password:{:?}", connection.password)
    };
    let raw = format!(
        "{}|{}|{}|{}|{}|{:?}|{}",
        path_absolute(config_path)
            .unwrap_or_else(|_| canonical_or_absolute(config_path.to_path_buf()))
            .display(),
        connection.name,
        connection.host,
        connection.port,
        connection.username,
        connection.socks_proxy,
        auth
    );
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn execute_remote_command_with_session(
    session: &Session,
    connection: &Connection,
    remote_command: &str,
    timeout_ms: u64,
) -> AppResult<String> {
    session.set_timeout(timeout_ms.try_into().unwrap_or(u32::MAX));
    let mut channel = session.channel_session()?;
    if connection.pty.unwrap_or(true) {
        channel.request_pty("xterm", None, None)?;
    }
    channel.exec(remote_command)?;
    let mut stdout = String::new();
    let mut stderr = String::new();
    channel.read_to_string(&mut stdout)?;
    channel.stderr().read_to_string(&mut stderr)?;
    channel.wait_close()?;
    let exit_status = channel.exit_status()?;
    if exit_status != 0 {
        let mut parts = Vec::new();
        if !stdout.trim().is_empty() {
            parts.push(stdout.trim_end().to_string());
        }
        if !stderr.trim().is_empty() {
            parts.push(format!("[stderr]\n{}", stderr.trim_end()));
        }
        parts.push(format!("[exit code] {}", exit_status));
        return Err(AppError::new(parts.join("\n")));
    }
    Ok(stdout.trim_end().to_string())
}

fn upload_file_with_session(
    session: &Session,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let sftp = session.sftp()?;
    let mut local_file = fs::File::open(local_path)?;
    let mut remote_file = sftp.create(Path::new(remote_path))?;
    std::io::copy(&mut local_file, &mut remote_file)?;
    Ok(())
}

fn download_file_with_session(
    session: &Session,
    remote_path: &str,
    local_path: &Path,
) -> AppResult<()> {
    let sftp = session.sftp()?;
    let mut remote_file = sftp.open(Path::new(remote_path))?;
    let mut local_file = fs::File::create(local_path)?;
    std::io::copy(&mut remote_file, &mut local_file)?;
    Ok(())
}
