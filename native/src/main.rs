use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
#[cfg(windows)]
use interprocess::local_socket::{
    prelude::*, GenericNamespaced, ListenerOptions, Stream as LocalSocketStream,
};
use rand_core::{OsRng, RngCore};
use regex::Regex;
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg};
use russh::{client, Channel, ChannelMsg, Disconnect, Preferred};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, SeekFrom, Write};
use std::net::IpAddr;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use url::Url;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CONFIG_DIR: &str = ".agent-ssh-cli";
const DEFAULT_CONFIG_FILE: &str = "config.json";
const SECRET_KEY_FILE: &str = "secret.key";
const SECRETS_FILE: &str = "secrets.json";
const MIGRATION_LOCK_FILE: &str = ".password-migration.lock";
const SECRETS_VERSION: u8 = 1;
const PASSWORD_REF_PREFIX: &str = "agentsshcli:";
const DEFAULT_CACHE_TTL_MS: u64 = 180_000;
const EXEC_HEARTBEAT_MARKER: &str = "__EXEC_HB__";
/// 跳板机 PTY 心跳占位符。jump-exec 长命令期间每 20s 输出一行，避免 JumpServer
/// 把会话当成 idle 主动关闭；extract_marker_output 会按行过滤掉这些占位。
const HEARTBEAT_MARKER: &str = "__JUMP_HB__";
const JUMP_MENU_PROMPT_MAX_MS: u64 = 10_000;
const JUMP_DIRECT_ATTEMPT_MAX_MS: u64 = 3_000;
const DAEMON_START_TIMEOUT_MS: u64 = 3_000;
const DAEMON_REQUEST_TIMEOUT_MS: u64 = 86_400_000;
const DAEMON_RESPONSE_LENGTH_BYTES: usize = 8;
const TRANSFER_CHUNK_BYTES: usize = 1024 * 1024;
const TRANSFER_MAX_RETRIES: usize = 3;

const HELP_AGENTSSHCLI: &str = r#"
用法:
  agentsshcli list [--config <path>] [--json]
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--pty|--no-pty] <connectionName> <command>
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--pty|--no-pty] --connection <name> (--command <command>|--command-file <path>) [--directory <dir>] [--timeout <ms>] [--total-timeout <ms>]
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <localPath> <remotePath>
  agentsshcli upload [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --local <path> --remote <path>
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] <connectionName> <remotePath> <localPath>
  agentsshcli download [--config <path>] [--no-cache] [--cache-ttl <ms>] --connection <name> --remote <path> --local <path>
  agentsshcli jump-search [--config <path>] [--timeout <ms>] [--total-timeout <ms>] <gatewayConnection> <query>
  agentsshcli jump-exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--timeout <ms>] <gatewayConnection> --target <hostOrIp> <command>
  agentsshcli add-jump-server [--config <path>] --name <name> --host <host> [--port <port>] --username <user> --private-key <path> [--force] [--dry-run]
  agentsshcli init-config
  agentsshcli stop-daemon [--config <path>]
  agentsshcli help [list|exec|upload|download|jump-search|jump-exec|add-jump-server|stop-daemon]
  agentsshcli --help
  agentsshcli --version

说明:
  agent-ssh-cli Rust 原生入口。当前 SSH 操作使用 russh 直连，缓存参数保留用于兼容旧脚本。
"#;

const HELP_JUMP_SEARCH: &str = r#"
用法:
  agentsshcli jump-search [--config <path>] [--timeout <ms>] [--total-timeout <ms>] <gatewayConnection> <query>
  agentsshcli jump-search [--config <path>] [--timeout <ms>] [--total-timeout <ms>] --connection <name> --query <text>
  agentsshcli help jump-search
  agentsshcli --version

说明:
  在 JumpServer 菜单层搜索当前账号有权限的主机候选，不直接进入目标机 shell。
  适合用户只给了业务简称、机器简称、实例尾号或 IP 片段时，先查出真实 hostname / IP。
  网关连接必须在 config.json 中配置 jumpServer.enabled=true。
  --timeout 控制“无输出/无响应”超时，默认 15000ms；--total-timeout 可选，用于设置整次搜索的硬上限。
"#;

const HELP_JUMP_MENU: &str = r#"
用法:
  agentsshcli jump-menu [--config <path>] [--timeout <ms>] [--total-timeout <ms>] <gatewayConnection>
  agentsshcli jump-menu [--config <path>] [--timeout <ms>] [--total-timeout <ms>] --connection <name>
  agentsshcli help jump-menu
  agentsshcli --version

说明:
  首次进入 JumpServer 菜单并原样展示 Opt> 上方的菜单内容，不直接搜索、不进入目标机 shell。
  适合先确认当前跳板机支持哪些菜单命令，再决定是否使用 p、/关键词 或其它自定义操作。
  网关连接必须在 config.json 中配置 jumpServer.enabled=true。
  --timeout 控制“无输出/无响应”超时，默认 15000ms；--total-timeout 可选，用于设置整次操作的硬上限。
"#;

const HELP_JUMP_EXEC: &str = r#"
用法:
  agentsshcli jump-exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--timeout <ms>] [--total-timeout <ms>] <gatewayConnection> --target <hostOrIp> <command>
  agentsshcli help jump-exec
  agentsshcli --version

说明:
  通过 JumpServer 跳板机以菜单 PTY 模式连接目标主机并执行命令。
  网关连接必须在 config.json 中配置 jumpServer.enabled=true。
  默认复用 daemon 缓存连接，空闲超过 cache-ttl 后自动回收；可用 --no-cache 改为每次直连。
  --timeout 控制“无输出/无响应”超时，默认 60000ms；命令阶段有持续输出时会自动续期。
  --total-timeout 可选，用于设置整次 jump-exec 的硬上限；默认不设总上限，避免大日志检索被固定总时长误杀。
  upload / download 不支持 JumpServer 模式。
"#;

const HELP_ADD_JUMP_SERVER: &str = r#"
用法:
  agentsshcli add-jump-server [--config <path>] --name <name> --host <host> [--port <port>] --username <user> --private-key <path> [--force] [--dry-run]
  agentsshcli help add-jump-server

说明:
  将一个 JumpServer 跳板机连接追加到 ~/.agent-ssh-cli/config.json，
  自动填入 jumpServer 字段（promptRegex=Opt>，shellPromptRegex=[#$]，charDelayMs=60，
  enterStrategy=direct-then-search）和常用 commandBlacklist。
  若 config.json 不存在会自动创建并设置 0600 权限。
  同名连接已存在时报错，加 --force 覆盖。
  支持先加 --dry-run 做参数预检，确认端口、私钥和连接名都没问题后再正式写入。
  适合 AI 收集完跳板机参数后先预检、再一次性写入，无需手动编辑 JSON。

参数:
  --name           连接名（唯一），建议 prod.jumpserver / test.jumpserver
  --host           JumpServer SSH 地址
  --port           SSH 端口，默认 8390（更贴近常见 JumpServer 场景）
  --username       SSH 用户名
  --private-key    私钥路径（必须存在且可被 SSH 栈加载）
  --force          覆盖同名连接（默认报错）
  --dry-run        只预检参数，不写入 config.json
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
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--pty|--no-pty] <connectionName> <command>
  agentsshcli exec [--config <path>] [--no-cache] [--cache-ttl <ms>] [--pty|--no-pty] --connection <name> (--command <command>|--command-file <path>) [--directory <dir>] [--timeout <ms>] [--total-timeout <ms>]
  agentsshcli help exec
  agentsshcli --version

说明:
  在远端执行命令。默认不分配伪终端，可通过 --pty 临时开启。
  --timeout 控制“无输出/无响应”超时，默认 30000ms；命令仍持续输出时会自动续期。
  --total-timeout 可选，用于设置整次 exec 的硬上限；默认不设总上限。
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

const HELP_STOP_DAEMON: &str = r#"
用法:
  agentsshcli stop-daemon [--config <path>]
  agentsshcli help stop-daemon

说明:
  停止当前配置文件对应的 SSH 缓存进程。这是连接池维护命令，不用于精确取消单个上传任务。
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
    password_ref: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
    socks_proxy: Option<String>,
    jump_host: Option<String>,
    pty: Option<bool>,
    allowed_local_paths: Option<Vec<String>>,
    command_whitelist: Option<Vec<String>>,
    command_blacklist: Option<Vec<String>>,
    jump_server: Option<RawJumpServer>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RawJumpServer {
    enabled: Option<bool>,
    prompt_regex: Option<String>,
    shell_prompt_regex: Option<String>,
    search_prefix: Option<String>,
    char_delay_ms: Option<u64>,
    enter_strategy: Option<String>,
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
    password_ref: Option<String>,
    private_key: Option<String>,
    passphrase: Option<String>,
    socks_proxy: Option<String>,
    jump_host: Option<String>,
    pty: Option<bool>,
    allowed_local_paths: Vec<String>,
    command_whitelist: Vec<PatternRule>,
    command_blacklist: Vec<PatternRule>,
    jump_server: Option<JumpServerConfig>,
}

#[derive(Debug, Clone)]
struct JumpServerConfig {
    enabled: bool,
    prompt_regex: String,
    shell_prompt_regex: String,
    search_prefix: String,
    char_delay_ms: u64,
    enter_strategy: String,
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
    command_file: Option<String>,
    directory: Option<String>,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
    pty: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UploadResumeMeta {
    file_size: u64,
    modified_ms: u64,
    chunk_bytes: usize,
}

#[derive(Debug)]
struct TransferArgs {
    global: GlobalArgs,
    connection_name: String,
    local_path: String,
    remote_path: String,
}

#[derive(Debug)]
struct JumpExecArgs {
    global: GlobalArgs,
    connection_name: String,
    target: String,
    command: String,
    command_file: Option<String>,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
}

#[derive(Debug)]
struct JumpSearchArgs {
    global: GlobalArgs,
    connection_name: String,
    query: String,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
}

#[derive(Debug)]
struct JumpMenuArgs {
    global: GlobalArgs,
    connection_name: String,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct CommandTimeouts {
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
}

#[derive(Debug)]
struct SocksProxy {
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

trait SshStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

impl<T> SshStream for T where T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}

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
        "jump-menu" => run_jump_menu(args.to_vec()),
        "jump-search" => run_jump_search(args.to_vec()),
        "jump-exec" => run_jump_exec(args.to_vec()),
        "add-jump-server" => run_add_jump_server(args.to_vec()),
        "stop-daemon" => run_stop_daemon(args.to_vec()),
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
        "jump-menu" | "jumpmenu" => HELP_JUMP_MENU,
        "jump-search" | "jumpsearch" => HELP_JUMP_SEARCH,
        "jump-exec" | "jumpexec" => HELP_JUMP_EXEC,
        "add-jump-server" => HELP_ADD_JUMP_SERVER,
        "stop-daemon" => HELP_STOP_DAEMON,
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
    let has_password_ref = is_non_empty(&entry.password_ref);
    let has_private_key = is_non_empty(&entry.private_key);
    let auth_count = [has_password || has_password_ref, has_private_key]
        .iter()
        .filter(|item| **item)
        .count();
    if auth_count == 0 {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项必须配置 password、passwordRef 或 privateKey 其中之一",
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
        .password_ref
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 passwordRef 必须是非空字符串",
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
    if entry
        .jump_host
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 jumpHost 必须是非空字符串",
            index + 1
        )));
    }
    if matches!(
        entry.jump_host.as_deref().map(str::trim),
        Some(value) if value == name
    ) {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 jumpHost 不能指向自身",
            index + 1
        )));
    }
    Ok(Connection {
        name,
        host,
        port,
        username,
        password: entry.password.filter(|_| has_password),
        password_ref: entry.password_ref.filter(|_| has_password_ref),
        private_key: entry.private_key.filter(|_| has_private_key),
        passphrase: entry.passphrase,
        socks_proxy: entry.socks_proxy,
        jump_host: entry.jump_host,
        pty: entry.pty,
        allowed_local_paths: ensure_string_array(
            entry.allowed_local_paths,
            "allowedLocalPaths",
            index,
        )?,
        command_whitelist: ensure_regex_array(entry.command_whitelist, "commandWhitelist", index)?,
        command_blacklist: ensure_regex_array(entry.command_blacklist, "commandBlacklist", index)?,
        jump_server: normalize_jump_server(entry.jump_server, index)?,
    })
}

fn normalize_jump_server(
    raw: Option<RawJumpServer>,
    index: usize,
) -> AppResult<Option<JumpServerConfig>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let enter_strategy = raw
        .enter_strategy
        .unwrap_or_else(|| "direct-then-search".to_string());
    if enter_strategy != "direct" && enter_strategy != "direct-then-search" {
        return Err(AppError::new(format!(
            "ssh-config.json 第 {} 项的 jumpServer.enterStrategy 非法: {}",
            index + 1,
            enter_strategy
        )));
    }
    let prompt_regex = raw
        .prompt_regex
        .unwrap_or_else(|| r"Opt>\s*$".to_string());
    let shell_prompt_regex = raw
        .shell_prompt_regex
        .unwrap_or_else(|| r"(?m)[#$>]\s*$".to_string());
    Regex::new(&prompt_regex).map_err(|error| {
        AppError::new(format!(
            "ssh-config.json 第 {} 项的 jumpServer.promptRegex 非法: {}",
            index + 1,
            error
        ))
    })?;
    Regex::new(&shell_prompt_regex).map_err(|error| {
        AppError::new(format!(
            "ssh-config.json 第 {} 项的 jumpServer.shellPromptRegex 非法: {}",
            index + 1,
            error
        ))
    })?;
    Ok(Some(JumpServerConfig {
        enabled: raw.enabled.unwrap_or(false),
        prompt_regex,
        shell_prompt_regex,
        search_prefix: raw.search_prefix.unwrap_or_else(|| "/".to_string()),
        char_delay_ms: raw.char_delay_ms.unwrap_or(60),
        enter_strategy,
    }))
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

fn load_config_for_connection(
    config_path: &Path,
    connection_name: &str,
) -> AppResult<Vec<Connection>> {
    let mut configs = load_config(config_path)?;
    let _ = find_connection(&configs, connection_name)?;
    resolve_password_ref_for_connection(config_path, &mut configs, connection_name)?;
    resolve_jump_password_refs(config_path, &mut configs, connection_name)?;
    validate_jump_hosts(&configs)?;
    Ok(configs)
}

fn validate_jump_hosts(configs: &[Connection]) -> AppResult<()> {
    for connection in configs {
        let Some(jump_name) = connection.jump_host.as_deref() else {
            continue;
        };
        let jump = find_connection(configs, jump_name)?;
        if jump.jump_host.is_some() {
            return Err(AppError::new(format!(
                "连接 {} 的 jumpHost {} 不能再配置 jumpHost，当前仅支持单级跳板机",
                connection.name, jump_name
            )));
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct SecretsFile {
    version: u8,
    items: HashMap<String, SecretItem>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SecretItem {
    nonce: String,
    ciphertext: String,
}

fn config_dir(config_path: &Path) -> AppResult<PathBuf> {
    let absolute = path_absolute(config_path)?;
    absolute
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| AppError::new("配置路径缺少父目录"))
}

fn secret_key_path(config_path: &Path) -> AppResult<PathBuf> {
    Ok(config_dir(config_path)?.join(SECRET_KEY_FILE))
}

fn secrets_path(config_path: &Path) -> AppResult<PathBuf> {
    Ok(config_dir(config_path)?.join(SECRETS_FILE))
}

struct MigrationLock {
    file: File,
}

impl MigrationLock {
    fn acquire(config_path: &Path) -> AppResult<Self> {
        let path = config_dir(config_path)?.join(MIGRATION_LOCK_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&path)?;
        lock_file_exclusive(&file)?;
        Ok(Self { file })
    }
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        let _ = unlock_file(&self.file);
    }
}

#[cfg(unix)]
fn lock_file_exclusive(file: &File) -> AppResult<()> {
    let fd = std::os::fd::AsRawFd::as_raw_fd(file);
    let rc = unsafe { libc::flock(fd, libc::LOCK_EX) };
    if rc == 0 {
        Ok(())
    } else {
        Err(AppError::new(format!(
            "获取本地密码迁移锁失败: {}",
            std::io::Error::last_os_error()
        )))
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) -> AppResult<()> {
    let fd = std::os::fd::AsRawFd::as_raw_fd(file);
    let rc = unsafe { libc::flock(fd, libc::LOCK_UN) };
    if rc == 0 {
        Ok(())
    } else {
        Err(AppError::new(format!(
            "释放本地密码迁移锁失败: {}",
            std::io::Error::last_os_error()
        )))
    }
}

#[cfg(not(unix))]
fn lock_file_exclusive(_file: &File) -> AppResult<()> {
    Ok(())
}

#[cfg(not(unix))]
fn unlock_file(_file: &File) -> AppResult<()> {
    Ok(())
}

fn load_or_create_secret_key(config_path: &Path) -> AppResult<[u8; 32]> {
    let path = secret_key_path(config_path)?;
    if path.exists() {
        let encoded = fs::read_to_string(&path)?;
        let bytes = BASE64_STANDARD
            .decode(encoded.trim())
            .map_err(|error| AppError::new(format!("读取本地密码密钥失败: {}", error)))?;
        let key: [u8; 32] = bytes
            .try_into()
            .map_err(|_| AppError::new("本地密码密钥长度非法"))?;
        return Ok(key);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut key = [0_u8; 32];
    OsRng.fill_bytes(&mut key);
    write_private_file(&path, BASE64_STANDARD.encode(key).as_bytes())?;
    Ok(key)
}

fn load_local_secret_key(config_path: &Path) -> AppResult<[u8; 32]> {
    let path = secret_key_path(config_path)?;
    let encoded = fs::read_to_string(&path).map_err(|error| {
        AppError::new(format!(
            "读取本地密码密钥失败: {}，{}",
            path.display(),
            error
        ))
    })?;
    let bytes = BASE64_STANDARD
        .decode(encoded.trim())
        .map_err(|error| AppError::new(format!("读取本地密码密钥失败: {}", error)))?;
    bytes
        .try_into()
        .map_err(|_| AppError::new("本地密码密钥长度非法"))
}

fn load_secrets(config_path: &Path) -> AppResult<SecretsFile> {
    let path = secrets_path(config_path)?;
    if !path.exists() {
        return Ok(SecretsFile {
            version: SECRETS_VERSION,
            items: HashMap::new(),
        });
    }
    let raw = fs::read_to_string(&path)?;
    let secrets: SecretsFile = serde_json::from_str(&raw)
        .map_err(|error| AppError::new(format!("secrets.json 解析失败: {}", error)))?;
    if secrets.version != SECRETS_VERSION {
        return Err(AppError::new("secrets.json 版本不支持"));
    }
    Ok(secrets)
}

fn save_secrets(config_path: &Path, secrets: &SecretsFile) -> AppResult<()> {
    let path = secrets_path(config_path)?;
    let raw = serde_json::to_vec_pretty(secrets)?;
    write_private_file(&path, &raw)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    #[cfg(unix)]
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(tmp, path)?;
    Ok(())
}

fn encrypt_password(config_path: &Path, password_ref: &str, password: &str) -> AppResult<()> {
    let key = load_or_create_secret_key(config_path)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(&key));
    let mut nonce_bytes = [0_u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), password.as_bytes())
        .map_err(|_| AppError::new("加密密码失败"))?;
    let mut secrets = load_secrets(config_path)?;
    secrets.items.insert(
        password_ref.to_string(),
        SecretItem {
            nonce: BASE64_STANDARD.encode(nonce_bytes),
            ciphertext: BASE64_STANDARD.encode(ciphertext),
        },
    );
    save_secrets(config_path, &secrets)
}

fn decrypt_password(config_path: &Path, password_ref: &str) -> AppResult<String> {
    let key = load_local_secret_key(config_path)?;
    let secrets = load_secrets(config_path)?;
    let item = secrets.items.get(password_ref).ok_or_else(|| {
        AppError::new(format!(
            "未找到 passwordRef 对应的本地密码: {}",
            password_ref
        ))
    })?;
    let nonce = BASE64_STANDARD
        .decode(&item.nonce)
        .map_err(|error| AppError::new(format!("本地密码 nonce 非法: {}", error)))?;
    if nonce.len() != 12 {
        return Err(AppError::new("本地密码 nonce 长度非法"));
    }
    let ciphertext = BASE64_STANDARD
        .decode(&item.ciphertext)
        .map_err(|error| AppError::new(format!("本地密码密文非法: {}", error)))?;
    let plaintext = ChaCha20Poly1305::new(Key::from_slice(&key))
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| AppError::new(format!("解密本地密码失败: {}", password_ref)))?;
    String::from_utf8(plaintext)
        .map_err(|error| AppError::new(format!("本地密码编码非法: {}", error)))
}

fn resolve_password_ref_for_connection(
    config_path: &Path,
    configs: &mut [Connection],
    connection_name: &str,
) -> AppResult<()> {
    let config = configs
        .iter_mut()
        .find(|item| item.name == connection_name)
        .ok_or_else(|| AppError::new(format!("未找到连接配置: {}", connection_name)))?;
    if config.password.is_none() {
        if let Some(password_ref) = config.password_ref.as_deref() {
            config.password = Some(decrypt_password(config_path, password_ref)?);
        }
    }
    Ok(())
}

fn resolve_jump_password_refs(
    config_path: &Path,
    configs: &mut [Connection],
    connection_name: &str,
) -> AppResult<()> {
    let jump_name = find_connection(configs, connection_name)?.jump_host.clone();
    if let Some(jump_name) = jump_name {
        resolve_password_ref_for_connection(config_path, configs, &jump_name)?;
    }
    Ok(())
}

fn password_ref_for(connection_name: &str) -> String {
    format!("{}{}", PASSWORD_REF_PREFIX, connection_name)
}

fn migrate_plain_password_for_connection(
    config_path: &Path,
    connection_name: &str,
) -> AppResult<bool> {
    let _lock = MigrationLock::acquire(config_path)?;
    let raw = fs::read_to_string(config_path)?;
    let mut values: Vec<serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|error| AppError::new(format!("ssh-config.json 解析失败: {}", error)))?;
    let mut migrated = false;
    for (index, value) in values.iter_mut().enumerate() {
        let object = value.as_object_mut().ok_or_else(|| {
            AppError::new(format!("ssh-config.json 第 {} 项必须是对象", index + 1))
        })?;
        let name = object
            .get("name")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        if name != connection_name {
            continue;
        }
        let Some(password) = object.get("password").and_then(|item| item.as_str()) else {
            return Ok(false);
        };
        if password.trim().is_empty() {
            return Ok(false);
        }
        let password = password.to_string();
        let password_ref = object
            .get("passwordRef")
            .and_then(|item| item.as_str())
            .filter(|item| !item.trim().is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| password_ref_for(connection_name));
        encrypt_password(config_path, &password_ref, &password)?;
        object.insert(
            "password".to_string(),
            serde_json::Value::String(String::new()),
        );
        object.insert(
            "passwordRef".to_string(),
            serde_json::Value::String(password_ref),
        );
        migrated = true;
        break;
    }
    if migrated {
        write_config_values(config_path, &values)?;
    }
    Ok(migrated)
}

fn write_config_values(config_path: &Path, values: &[serde_json::Value]) -> AppResult<()> {
    let raw = serde_json::to_vec_pretty(values)?;
    let tmp = config_path.with_extension("tmp");
    fs::write(&tmp, raw)?;
    fs::rename(tmp, config_path)?;
    Ok(())
}

fn prepare_connection_config(config_path: &Path, connection_name: &str) -> AppResult<()> {
    let _ = migrate_plain_password_for_connection(config_path, connection_name)?;
    Ok(())
}

fn hash_file(path: &Path) -> AppResult<String> {
    let bytes = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfigSnapshot {
    modified: Option<SystemTime>,
    len: u64,
    hash: String,
}

impl ConfigSnapshot {
    fn read(path: &Path) -> AppResult<Self> {
        let metadata = fs::metadata(path)?;
        Ok(Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
            hash: hash_file(path)?,
        })
    }

    fn metadata_matches(&self, path: &Path) -> AppResult<bool> {
        let metadata = fs::metadata(path)?;
        Ok(self.modified == metadata.modified().ok() && self.len == metadata.len())
    }
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
            command_file: None,
            directory: None,
            idle_timeout_ms: 30000,
            total_timeout_ms: None,
            pty: None,
        });
    }
    let mut args = global.args.clone();
    let connection_option = take_option(&mut args, &["--connection", "-c"])?;
    let command_option = take_option(&mut args, &["--command"])?;
    let command_file = take_option(&mut args, &["--command-file"])?;
    let directory = take_option(&mut args, &["--directory", "-d"])?;
    let timeout_value = take_option(&mut args, &["--timeout", "-t"])?;
    let total_timeout_value = take_option(&mut args, &["--total-timeout"])?;
    let pty = take_bool_flag_pair(&mut args, "--pty", "--no-pty")?;
    let connection_positional = take_positional(&mut args, "connectionName")?;
    let command_positional = take_positional(&mut args, "command")?;
    ensure_no_mixed(&connection_option, &connection_positional, "connectionName")?;
    ensure_no_mixed(&command_option, &command_positional, "command")?;
    ensure_no_mixed(&command_file, &command_positional, "command")?;
    if command_option.is_some() && command_file.is_some() {
        return Err(AppError::new(
            "command 同时使用了 --command 和 --command-file，保留一种即可",
        ));
    }
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let connection_name = connection_option.or(connection_positional).ok_or_else(|| {
        AppError::new("缺少必填参数 connectionName 或 command，使用 --help 查看说明")
    })?;
    let command = command_option.or(command_positional).unwrap_or_default();
    if command.is_empty() && command_file.is_none() {
        return Err(AppError::new(
            "缺少必填参数 connectionName 或 command，使用 --help 查看说明",
        ));
    }
    let idle_timeout_ms = match timeout_value {
        Some(value) => normalize_positive_u64(&value, "timeout 必须是正整数毫秒值")?,
        None => 30000,
    };
    let total_timeout_ms = match total_timeout_value {
        Some(value) => Some(normalize_positive_u64(&value, "total-timeout 必须是正整数毫秒值")?),
        None => None,
    };
    Ok(ExecuteArgs {
        global,
        connection_name,
        command,
        command_file,
        directory,
        idle_timeout_ms,
        total_timeout_ms,
        pty,
    })
}

fn take_bool_flag_pair(
    args: &mut Vec<String>,
    true_name: &str,
    false_name: &str,
) -> AppResult<Option<bool>> {
    let true_count = args
        .iter()
        .filter(|item| item.as_str() == true_name)
        .count();
    let false_count = args
        .iter()
        .filter(|item| item.as_str() == false_name)
        .count();
    if true_count > 1 {
        return Err(AppError::new(format!("参数重复声明: {}", true_name)));
    }
    if false_count > 1 {
        return Err(AppError::new(format!("参数重复声明: {}", false_name)));
    }
    if true_count == 1 && false_count == 1 {
        return Err(AppError::new(format!(
            "{} 和 {} 只能选择一个",
            true_name, false_name
        )));
    }
    if let Some(index) = args.iter().position(|item| item == true_name) {
        args.remove(index);
        return Ok(Some(true));
    }
    if let Some(index) = args.iter().position(|item| item == false_name) {
        args.remove(index);
        return Ok(Some(false));
    }
    Ok(None)
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

fn run_stop_daemon(argv: Vec<String>) -> AppResult<()> {
    let global = parse_global_args(argv)?;
    if global.help {
        return print_help("stop-daemon");
    }
    if global.version {
        return print_version();
    }
    if !global.args.is_empty() {
        return Err(AppError::new(format!(
            "agentsshcli stop-daemon 不接受位置参数: {}",
            global.args.join(" ")
        )));
    }
    request_stop_daemon(&global.config_path)?;
    println!("SSH 缓存进程已停止");
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
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    let command = resolve_execute_command(&configs, &parsed)?;
    validate_command(connection, &command)?;
    let remote_command = match parsed.directory {
        Some(ref directory) => format!("cd -- {} && {}", shell_json_quote(directory)?, command),
        None => command.clone(),
    };
    let result = if parsed.global.no_cache {
        execute_remote_command(
            &configs,
            connection,
            &remote_command,
            CommandTimeouts {
                idle_timeout_ms: parsed.idle_timeout_ms,
                total_timeout_ms: parsed.total_timeout_ms,
            },
            resolve_pty(connection, parsed.pty),
        )?
    } else {
        request_daemon_execute(&parsed, &command)?
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
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    if parsed.global.no_cache {
        let local_path = validate_local_path(&configs, &parsed.local_path, &env::current_dir()?)?;
        upload_file(&configs, connection, &local_path, &parsed.remote_path)?;
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
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    if parsed.global.no_cache {
        let local_path = validate_local_path(&configs, &parsed.local_path, &env::current_dir()?)?;
        if let Some(parent) = local_path.parent() {
            fs::create_dir_all(parent)?;
        }
        download_file(
            &configs,
            connection,
            &parsed.remote_path,
            &local_path,
            30000,
        )?;
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
        .map_err(|error| AppError::new(format!("socksProxy 格式非法: {}", error)))?;
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

async fn read_exact_async(stream: &mut tokio::net::TcpStream, length: usize) -> AppResult<Vec<u8>> {
    let mut buffer = vec![0_u8; length];
    stream.read_exact(&mut buffer).await?;
    Ok(buffer)
}

async fn authenticate_socks_proxy(
    stream: &mut tokio::net::TcpStream,
    proxy: &SocksProxy,
) -> AppResult<()> {
    let method = if proxy.username.is_some() { 0x02 } else { 0x00 };
    stream.write_all(&[0x05, 0x01, method]).await?;
    let response = read_exact_async(stream, 2).await?;
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
    stream.write_all(&request).await?;
    let auth_response = read_exact_async(stream, 2).await?;
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

async fn read_socks_connect_response(stream: &mut tokio::net::TcpStream) -> AppResult<()> {
    let header = read_exact_async(stream, 4).await?;
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
            read_exact_async(stream, 4).await?;
        }
        0x04 => {
            read_exact_async(stream, 16).await?;
        }
        0x03 => {
            let len = read_exact_async(stream, 1).await?[0] as usize;
            read_exact_async(stream, len).await?;
        }
        _ => return Err(AppError::new("SOCKS5 代理响应地址类型非法")),
    }
    read_exact_async(stream, 2).await?;
    Ok(())
}

async fn connect_socks_proxy(connection: &Connection) -> AppResult<tokio::net::TcpStream> {
    let proxy = parse_socks_proxy(
        connection
            .socks_proxy
            .as_deref()
            .ok_or_else(|| AppError::new("缺少 socksProxy 配置"))?,
    )?;
    let mut stream = tokio::net::TcpStream::connect((proxy.host.as_str(), proxy.port)).await?;
    authenticate_socks_proxy(&mut stream, &proxy).await?;
    let mut request = vec![0x05, 0x01, 0x00];
    request.extend_from_slice(&encode_target_address(&connection.host)?);
    request.extend_from_slice(&connection.port.to_be_bytes());
    stream.write_all(&request).await?;
    read_socks_connect_response(&mut stream).await?;
    Ok(stream)
}

struct RusshClient;

impl client::Handler for RusshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

async fn connect_russh(
    configs: &[Connection],
    connection: &Connection,
) -> AppResult<client::Handle<RusshClient>> {
    let stream = open_connection_stream(configs, connection).await?;
    connect_russh_over_stream(connection, stream).await
}

async fn connect_russh_direct(connection: &Connection) -> AppResult<client::Handle<RusshClient>> {
    let stream: Box<dyn SshStream> = if connection.socks_proxy.is_some() {
        Box::new(connect_socks_proxy(connection).await?)
    } else {
        Box::new(tokio::net::TcpStream::connect((connection.host.as_str(), connection.port)).await?)
    };
    connect_russh_over_stream(connection, stream).await
}

async fn connect_russh_over_stream(
    connection: &Connection,
    stream: Box<dyn SshStream>,
) -> AppResult<client::Handle<RusshClient>> {
    let config = client::Config {
        inactivity_timeout: Some(Duration::from_secs(30)),
        preferred: Preferred {
            kex: Cow::Owned(vec![
                russh::kex::CURVE25519,
                russh::kex::CURVE25519_PRE_RFC_8731,
                russh::kex::DH_GEX_SHA256,
                russh::kex::DH_G14_SHA256,
                // 现代算法优先，旧 DH 算法仅作为兼容历史 OpenSSH 服务端的最后兜底。
                russh::kex::DH_G14_SHA1,
                russh::kex::DH_GEX_SHA1,
                russh::kex::DH_G1_SHA1,
                russh::kex::EXTENSION_SUPPORT_AS_CLIENT,
            ]),
            mac: Cow::Owned(vec![
                russh::mac::HMAC_SHA512_ETM,
                russh::mac::HMAC_SHA256_ETM,
                russh::mac::HMAC_SHA512,
                russh::mac::HMAC_SHA256,
                // 旧 MAC 仅作为兼容历史 OpenSSH 服务端的最后兜底。
                russh::mac::HMAC_SHA1_ETM,
                russh::mac::HMAC_SHA1,
            ]),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut session = client::connect_stream(Arc::new(config), stream, RusshClient)
        .await
        .map_err(|error| {
            AppError::new(format!("连接 {} 建立 SSH 失败: {}", connection.name, error))
        })?;
    authenticate_russh(connection, &mut session).await?;
    Ok(session)
}

async fn open_connection_stream(
    configs: &[Connection],
    connection: &Connection,
) -> AppResult<Box<dyn SshStream>> {
    if let Some(jump_name) = connection.jump_host.as_deref() {
        let jump = find_connection(configs, jump_name)?;
        let jump_session = connect_russh_direct(jump).await?;
        let channel = jump_session
            .channel_open_direct_tcpip(
                connection.host.clone(),
                u32::from(connection.port),
                "127.0.0.1",
                0,
            )
            .await
            .map_err(|error| {
                AppError::new(format!(
                    "连接 {} 通过跳板机 {} 打开直连通道失败: {}",
                    connection.name, jump.name, error
                ))
            })?;
        return Ok(Box::new(channel.into_stream()));
    }
    if connection.socks_proxy.is_some() {
        return Ok(Box::new(connect_socks_proxy(connection).await?));
    }
    Ok(Box::new(
        tokio::net::TcpStream::connect((connection.host.as_str(), connection.port)).await?,
    ))
}

async fn authenticate_russh(
    connection: &Connection,
    session: &mut client::Handle<RusshClient>,
) -> AppResult<()> {
    if let Some(password) = connection.password.as_deref() {
        let auth = session
            .authenticate_password(connection.username.clone(), password.to_string())
            .await
            .map_err(|error| {
                AppError::new(format!("连接 {} 密码认证失败: {}", connection.name, error))
            })?;
        if !auth.success() {
            return Err(AppError::new(format!(
                "连接 {} 密码认证被拒绝",
                connection.name
            )));
        }
        return Ok(());
    }
    let private_key = connection
        .private_key
        .as_deref()
        .ok_or_else(|| AppError::new(format!("连接 {} 缺少认证配置", connection.name)))?;
    let key_pair =
        load_secret_key(private_key, connection.passphrase.as_deref()).map_err(|error| {
            AppError::new(format!(
                "连接 {} 加载私钥失败: {}，{}",
                connection.name, private_key, error
            ))
        })?;
    let hash_alg = session
        .best_supported_rsa_hash()
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 协商 RSA hash 失败: {}",
                connection.name, error
            ))
        })?
        .flatten();
    let auth = session
        .authenticate_publickey(
            connection.username.clone(),
            PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
        )
        .await
        .map_err(|error| {
            AppError::new(format!("连接 {} 公钥认证失败: {}", connection.name, error))
        })?;
    if !auth.success() {
        return Err(AppError::new(format!(
            "连接 {} 公钥认证被拒绝",
            connection.name
        )));
    }
    Ok(())
}

async fn execute_remote_command_with_session_async(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    remote_command: &str,
    pty: bool,
    timeouts: CommandTimeouts,
) -> AppResult<String> {
    let mut channel = session.channel_open_session().await.map_err(|error| {
        AppError::new(format!("连接 {} 打开会话失败: {}", connection.name, error))
    })?;
    if pty {
        channel
            .request_pty(true, "xterm", 80, 24, 0, 0, &[])
            .await
            .map_err(|error| {
                AppError::new(format!(
                    "连接 {} 分配伪终端失败: {}",
                    connection.name, error
                ))
            })?;
    }
    let wrapped_remote_command = wrap_command_with_heartbeat(remote_command, EXEC_HEARTBEAT_MARKER);
    channel.exec(true, wrapped_remote_command.as_str()).await.map_err(|error| {
        AppError::new(format!("连接 {} 执行命令失败: {}", connection.name, error))
    })?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_status = None;
    let total_deadline = total_deadline_from_ms(timeouts.total_timeout_ms);
    loop {
        let wait_budget = phase_timeout_ms(timeouts.idle_timeout_ms, total_deadline);
        let msg = match tokio::time::timeout(Duration::from_millis(wait_budget), channel.wait()).await {
            Ok(Some(msg)) => msg,
            Ok(None) => break,
            Err(_) => {
                return Err(AppError::new(format!(
                    "连接 {} 执行命令等待输出空闲超时",
                    connection.name
                )))
            }
        };
        match msg {
            ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
            ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
            ChannelMsg::ExitStatus { exit_status: code } => exit_status = Some(code),
            _ => {}
        }
    }
    let stdout = strip_exec_heartbeat_lines(&String::from_utf8_lossy(&stdout));
    let stderr = String::from_utf8_lossy(&stderr).trim_end().to_string();
    let code = exit_status.unwrap_or(0);
    if code != 0 {
        let mut parts = Vec::new();
        if !stdout.is_empty() {
            parts.push(stdout);
        }
        if !stderr.is_empty() {
            parts.push(format!("[stderr]\n{}", stderr));
        }
        parts.push(format!("[exit code] {}", code));
        return Err(AppError::new(parts.join("\n")));
    }
    Ok(stdout)
}

async fn execute_remote_command_async(
    configs: &[Connection],
    connection: &Connection,
    remote_command: &str,
    pty: bool,
    timeouts: CommandTimeouts,
) -> AppResult<String> {
    let session = connect_russh(configs, connection).await?;
    let result =
        execute_remote_command_with_session_async(&session, connection, remote_command, pty, timeouts).await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    result
}

fn execute_remote_command(
    configs: &[Connection],
    connection: &Connection,
    remote_command: &str,
    timeouts: CommandTimeouts,
    pty: bool,
) -> AppResult<String> {
    run_with_timeout(
        command_runtime_timeout_ms(timeouts.idle_timeout_ms, timeouts.total_timeout_ms),
        execute_remote_command_async(configs, connection, remote_command, pty, timeouts),
    )
}

async fn open_sftp_session(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
) -> AppResult<SftpSession> {
    let channel = session.channel_open_session().await.map_err(|error| {
        AppError::new(format!(
            "连接 {} 打开 SFTP 会话失败: {}",
            connection.name, error
        ))
    })?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 请求 SFTP 子系统失败: {}",
                connection.name, error
            ))
        })?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 初始化 SFTP 失败: {}",
                connection.name, error
            ))
        })
}

fn temporary_remote_path(remote_path: &str) -> String {
    format!("{}.part", remote_path)
}

fn temporary_remote_meta_path(remote_path: &str) -> String {
    format!("{}.part.meta", remote_path)
}

fn build_upload_resume_meta(metadata: &std::fs::Metadata) -> UploadResumeMeta {
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0);
    UploadResumeMeta {
        file_size: metadata.len(),
        modified_ms,
        chunk_bytes: TRANSFER_CHUNK_BYTES,
    }
}

async fn upload_file_with_session_async(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let local_metadata = fs::metadata(local_path)?;
    let resume_meta = build_upload_resume_meta(&local_metadata);
    let file_size = resume_meta.file_size;
    let temp_remote_path = temporary_remote_path(remote_path);
    let temp_remote_meta_path = temporary_remote_meta_path(remote_path);
    let mut last_error: Option<AppError> = None;

    // SFTP 传输不再设置总超时：大文件允许长时间运行，失败时按整次上传重试。
    for attempt in 1..=TRANSFER_MAX_RETRIES {
        let sftp = open_sftp_session(session, connection).await?;
        let upload_result = upload_file_once(
            &sftp,
            connection,
            local_path,
            remote_path,
            &temp_remote_path,
            &temp_remote_meta_path,
            &resume_meta,
            file_size,
            attempt,
        )
        .await;
        let _ = sftp.close().await;

        match upload_result {
            Ok(()) => return Ok(()),
            Err(error) if attempt < TRANSFER_MAX_RETRIES => {
                eprintln!(
                    "上传失败，准备重试 {}/{}: {}",
                    attempt + 1,
                    TRANSFER_MAX_RETRIES,
                    error
                );
                last_error = Some(error);
            }
            Err(error) => last_error = Some(error),
        }
    }

    Err(AppError::new(format!(
        "上传失败，已重试 {} 次: {}",
        TRANSFER_MAX_RETRIES,
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "未知错误".to_string())
    )))
}

async fn upload_file_once(
    sftp: &SftpSession,
    connection: &Connection,
    local_path: &Path,
    remote_path: &str,
    temp_remote_path: &str,
    temp_remote_meta_path: &str,
    resume_meta: &UploadResumeMeta,
    file_size: u64,
    attempt: usize,
) -> AppResult<()> {
    ensure_upload_resume_meta(sftp, temp_remote_path, temp_remote_meta_path, resume_meta).await?;
    let resume_offset = resolve_upload_resume_offset(sftp, temp_remote_path, file_size).await?;
    let mut local_file = tokio::fs::File::open(local_path).await?;
    if resume_offset > 0 {
        local_file.seek(SeekFrom::Start(resume_offset)).await?;
        eprintln!(
            "发现远端临时文件，断点续传: {}/{} bytes",
            resume_offset, file_size
        );
    }

    let open_flags = if resume_offset > 0 {
        OpenFlags::CREATE | OpenFlags::APPEND | OpenFlags::WRITE
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut remote_file = sftp
        .open_with_flags(temp_remote_path.to_string(), open_flags)
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 打开远端临时文件失败: {}",
                connection.name, error
            ))
        })?;

    let mut buffer = vec![0_u8; TRANSFER_CHUNK_BYTES];
    let mut uploaded = resume_offset;
    print_upload_progress(uploaded, file_size, attempt)?;
    loop {
        let read_bytes = local_file.read(&mut buffer).await?;
        if read_bytes == 0 {
            break;
        }
        remote_file.write_all(&buffer[..read_bytes]).await?;
        remote_file.flush().await?;
        uploaded += read_bytes as u64;
        print_upload_progress(uploaded, file_size, attempt)?;
    }

    remote_file.shutdown().await?;
    verify_remote_temp_size(sftp, temp_remote_path, file_size).await?;
    // 尽量先删除目标文件，兼容不支持覆盖 rename 的 SFTP 服务端。
    let _ = sftp.remove_file(remote_path.to_string()).await;
    sftp.rename(temp_remote_path.to_string(), remote_path.to_string())
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 替换远端文件失败: {}",
                connection.name, error
            ))
        })?;
    let _ = sftp.remove_file(temp_remote_meta_path.to_string()).await;
    eprintln!("上传完成: {} bytes", file_size);
    Ok(())
}

async fn ensure_upload_resume_meta(
    sftp: &SftpSession,
    temp_remote_path: &str,
    temp_remote_meta_path: &str,
    resume_meta: &UploadResumeMeta,
) -> AppResult<()> {
    let expected = serde_json::to_vec(resume_meta)?;
    let current = match sftp.read(temp_remote_meta_path.to_string()).await {
        Ok(bytes) => Some(bytes),
        Err(_) => None,
    };
    if current.as_deref() == Some(expected.as_slice()) {
        return Ok(());
    }

    // 本地文件特征变化时，旧 .part 不能安全续传，必须删除后重建元数据。
    let _ = sftp.remove_file(temp_remote_path.to_string()).await;
    let _ = sftp.remove_file(temp_remote_meta_path.to_string()).await;
    let mut meta_file = sftp
        .open_with_flags(
            temp_remote_meta_path.to_string(),
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|error| AppError::new(format!("创建远端续传元数据失败: {}", error)))?;
    meta_file
        .write_all(&expected)
        .await
        .map_err(|error| AppError::new(format!("写入远端续传元数据失败: {}", error)))?;
    meta_file
        .shutdown()
        .await
        .map_err(|error| AppError::new(format!("关闭远端续传元数据失败: {}", error)))?;
    Ok(())
}

async fn resolve_upload_resume_offset(
    sftp: &SftpSession,
    temp_remote_path: &str,
    file_size: u64,
) -> AppResult<u64> {
    let metadata = match sftp.metadata(temp_remote_path.to_string()).await {
        Ok(metadata) => metadata,
        Err(_) => return Ok(0),
    };
    let remote_size = metadata.size.unwrap_or(0);
    if remote_size == file_size {
        return Ok(remote_size);
    }
    if remote_size < file_size {
        return Ok(remote_size);
    }
    // 远端临时文件比本地还大，说明它不属于当前上传内容，删除后重传。
    let _ = sftp.remove_file(temp_remote_path.to_string()).await;
    Ok(0)
}

async fn verify_remote_temp_size(
    sftp: &SftpSession,
    temp_remote_path: &str,
    expected_size: u64,
) -> AppResult<()> {
    let metadata = sftp
        .metadata(temp_remote_path.to_string())
        .await
        .map_err(|error| AppError::new(format!("读取远端临时文件大小失败: {}", error)))?;
    let actual_size = metadata.size.unwrap_or(0);
    if actual_size != expected_size {
        return Err(AppError::new(format!(
            "远端临时文件大小不一致: 期望 {} bytes，实际 {} bytes",
            expected_size, actual_size
        )));
    }
    Ok(())
}

fn print_upload_progress(uploaded: u64, total: u64, attempt: usize) -> AppResult<()> {
    if total == 0 {
        eprintln!("上传进度: 100% (0/0 bytes, 第 {} 次)", attempt);
        return Ok(());
    }
    let percent = uploaded.saturating_mul(100) / total;
    eprintln!(
        "上传进度: {}% ({}/{} bytes, 第 {} 次)",
        percent, uploaded, total, attempt
    );
    Ok(())
}

async fn download_file_with_session_async(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    remote_path: &str,
    local_path: &Path,
) -> AppResult<()> {
    let sftp = open_sftp_session(session, connection).await?;
    let mut remote_file = sftp.open(remote_path.to_string()).await.map_err(|error| {
        AppError::new(format!(
            "连接 {} 打开远端文件失败: {}",
            connection.name, error
        ))
    })?;
    let mut local_file = tokio::fs::File::create(local_path).await?;
    tokio::io::copy(&mut remote_file, &mut local_file).await?;
    local_file.shutdown().await?;
    let _ = sftp.close().await;
    Ok(())
}

async fn upload_file_async(
    configs: &[Connection],
    connection: &Connection,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let session = connect_russh(configs, connection).await?;
    let result =
        upload_file_with_session_async(&session, connection, local_path, remote_path).await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    result
}

async fn download_file_async(
    configs: &[Connection],
    connection: &Connection,
    remote_path: &str,
    local_path: &Path,
) -> AppResult<()> {
    let session = connect_russh(configs, connection).await?;
    let result =
        download_file_with_session_async(&session, connection, remote_path, local_path).await;
    let _ = session
        .disconnect(Disconnect::ByApplication, "", "English")
        .await;
    result
}

fn upload_file(
    configs: &[Connection],
    connection: &Connection,
    local_path: &Path,
    remote_path: &str,
) -> AppResult<()> {
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| AppError::new(format!("创建 tokio runtime 失败: {}", error)))?;
    runtime.block_on(upload_file_async(
        configs,
        connection,
        local_path,
        remote_path,
    ))
}

fn download_file(
    configs: &[Connection],
    connection: &Connection,
    remote_path: &str,
    local_path: &Path,
    timeout_ms: u64,
) -> AppResult<()> {
    run_with_timeout(
        timeout_ms,
        download_file_async(configs, connection, remote_path, local_path),
    )
}

fn run_with_timeout<T, F>(timeout_ms: u64, future: F) -> AppResult<T>
where
    F: std::future::Future<Output = AppResult<T>>,
{
    let runtime = tokio::runtime::Runtime::new()
        .map_err(|error| AppError::new(format!("创建 tokio runtime 失败: {}", error)))?;
    block_with_timeout(&runtime, timeout_ms, future)
}

fn block_with_timeout<T, F>(
    runtime: &tokio::runtime::Runtime,
    timeout_ms: u64,
    future: F,
) -> AppResult<T>
where
    F: std::future::Future<Output = AppResult<T>>,
{
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(timeout_ms), future)
            .await
            .map_err(|_| AppError::new(format!("操作超时: {} ms", timeout_ms)))?
    })
}

fn resolve_pty(connection: &Connection, override_pty: Option<bool>) -> bool {
    override_pty.or(connection.pty).unwrap_or(false)
}

fn resolve_command_from_file_or_inline(
    configs: &[Connection],
    inline_command: &str,
    command_file: Option<&String>,
) -> AppResult<String> {
    let Some(command_file) = command_file else {
        return Ok(inline_command.to_string());
    };
    let path = validate_local_path(configs, command_file, &env::current_dir()?)?;
    // 命令文件按 UTF-8 读取，避免二进制内容或错误编码被误当作远端 shell 命令执行。
    fs::read_to_string(&path).map_err(|error| {
        AppError::new(format!(
            "读取 command-file 失败: {}，{}",
            path.display(),
            error
        ))
    })
}

fn resolve_execute_command(configs: &[Connection], parsed: &ExecuteArgs) -> AppResult<String> {
    resolve_command_from_file_or_inline(configs, &parsed.command, parsed.command_file.as_ref())
}

const ANSI_ESCAPE_PATTERN: &str = r"\x1B\[[0-?]*[ -/]*[@-~]|\x1B\].*?(?:\x07|\x1b\\)";

fn ansi_regex() -> Regex {
    Regex::new(ANSI_ESCAPE_PATTERN).expect("ANSI 正则编译失败")
}

fn strip_ansi(input: &str) -> String {
    ansi_regex().replace_all(input, "").into_owned()
}

fn wrap_command_with_heartbeat(command: &str, heartbeat_marker: &str) -> String {
    format!(
        "{{ ( {cmd} ) & __agent_pid=$!; ( while kill -0 $__agent_pid 2>/dev/null; do printf '{hb}\\n'; sleep 1; done ) & __agent_hb=$!; wait $__agent_pid; __agent_status=$?; kill $__agent_hb >/dev/null 2>&1; wait $__agent_hb >/dev/null 2>&1; exit $__agent_status; }}",
        cmd = command,
        hb = heartbeat_marker,
    )
}

fn parse_jump_exec_args(argv: Vec<String>) -> AppResult<JumpExecArgs> {
    let global = parse_global_args(argv)?;
    if global.help || global.version {
        return Ok(JumpExecArgs {
            global,
            connection_name: String::new(),
            target: String::new(),
            command: String::new(),
            command_file: None,
            idle_timeout_ms: 60000,
            total_timeout_ms: None,
        });
    }
    let mut args = global.args.clone();
    let connection_option = take_option(&mut args, &["--connection", "-c"])?;
    let target_option = take_option(&mut args, &["--target"])?;
    let command_option = take_option(&mut args, &["--command"])?;
    let command_file = take_option(&mut args, &["--command-file"])?;
    let timeout_value = take_option(&mut args, &["--timeout", "-t"])?;
    let total_timeout_value = take_option(&mut args, &["--total-timeout"])?;
    let connection_positional = take_positional(&mut args, "gatewayConnection")?;
    let command_positional = take_positional(&mut args, "command")?;
    ensure_no_mixed(
        &connection_option,
        &connection_positional,
        "gatewayConnection",
    )?;
    ensure_no_mixed(&command_option, &command_positional, "command")?;
    ensure_no_mixed(&command_file, &command_positional, "command")?;
    if command_option.is_some() && command_file.is_some() {
        return Err(AppError::new(
            "command 同时使用了 --command 和 --command-file，保留一种即可",
        ));
    }
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let connection_name = connection_option
        .or(connection_positional)
        .ok_or_else(|| AppError::new("缺少必填参数 gatewayConnection，使用 --help 查看说明"))?;
    let target = target_option
        .ok_or_else(|| AppError::new("缺少必填参数 --target，使用 --help 查看说明"))?;
    let command = command_option
        .or(command_positional)
        .unwrap_or_default();
    if command.is_empty() && command_file.is_none() {
        return Err(AppError::new("缺少必填参数 command，使用 --help 查看说明"));
    }
    let idle_timeout_ms = match timeout_value {
        Some(value) => normalize_positive_u64(&value, "timeout 必须是正整数毫秒值")?,
        None => 60000,
    };
    let total_timeout_ms = match total_timeout_value {
        Some(value) => Some(normalize_positive_u64(&value, "total-timeout 必须是正整数毫秒值")?),
        None => None,
    };
    if target.trim().is_empty() {
        return Err(AppError::new("--target 不能为空"));
    }
    if command_file.is_none() && command.trim().is_empty() {
        return Err(AppError::new("command 不能为空"));
    }
    Ok(JumpExecArgs {
        global,
        connection_name,
        target,
        command,
        command_file,
        idle_timeout_ms,
        total_timeout_ms,
    })
}

fn parse_jump_search_args(argv: Vec<String>) -> AppResult<JumpSearchArgs> {
    let global = parse_global_args(argv)?;
    if global.help || global.version {
        return Ok(JumpSearchArgs {
            global,
            connection_name: String::new(),
            query: String::new(),
            idle_timeout_ms: 15000,
            total_timeout_ms: None,
        });
    }
    let mut args = global.args.clone();
    let connection_option = take_option(&mut args, &["--connection", "-c"])?;
    let query_option = take_option(&mut args, &["--query"])?;
    let timeout_value = take_option(&mut args, &["--timeout", "-t"])?;
    let total_timeout_value = take_option(&mut args, &["--total-timeout"])?;
    let connection_positional = take_positional(&mut args, "gatewayConnection")?;
    let query_positional = take_positional(&mut args, "query")?;
    ensure_no_mixed(
        &connection_option,
        &connection_positional,
        "gatewayConnection",
    )?;
    ensure_no_mixed(&query_option, &query_positional, "query")?;
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let connection_name = connection_option
        .or(connection_positional)
        .ok_or_else(|| AppError::new("缺少必填参数 gatewayConnection，使用 --help 查看说明"))?;
    let query = query_option
        .or(query_positional)
        .ok_or_else(|| AppError::new("缺少必填参数 query，使用 --help 查看说明"))?;
    let idle_timeout_ms = match timeout_value {
        Some(value) => normalize_positive_u64(&value, "timeout 必须是正整数毫秒值")?,
        None => 15000,
    };
    let total_timeout_ms = match total_timeout_value {
        Some(value) => Some(normalize_positive_u64(&value, "total-timeout 必须是正整数毫秒值")?),
        None => None,
    };
    if query.trim().is_empty() {
        return Err(AppError::new("query 不能为空"));
    }
    Ok(JumpSearchArgs {
        global,
        connection_name,
        query,
        idle_timeout_ms,
        total_timeout_ms,
    })
}

fn parse_jump_menu_args(argv: Vec<String>) -> AppResult<JumpMenuArgs> {
    let global = parse_global_args(argv)?;
    if global.help || global.version {
        return Ok(JumpMenuArgs {
            global,
            connection_name: String::new(),
            idle_timeout_ms: 15000,
            total_timeout_ms: None,
        });
    }
    let mut args = global.args.clone();
    let connection_option = take_option(&mut args, &["--connection", "-c"])?;
    let timeout_value = take_option(&mut args, &["--timeout", "-t"])?;
    let total_timeout_value = take_option(&mut args, &["--total-timeout"])?;
    let connection_positional = take_positional(&mut args, "gatewayConnection")?;
    ensure_no_mixed(
        &connection_option,
        &connection_positional,
        "gatewayConnection",
    )?;
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;
    let connection_name = connection_option
        .or(connection_positional)
        .ok_or_else(|| AppError::new("缺少必填参数 gatewayConnection，使用 --help 查看说明"))?;
    let idle_timeout_ms = match timeout_value {
        Some(value) => normalize_positive_u64(&value, "timeout 必须是正整数毫秒值")?,
        None => 15000,
    };
    let total_timeout_ms = match total_timeout_value {
        Some(value) => Some(normalize_positive_u64(&value, "total-timeout 必须是正整数毫秒值")?),
        None => None,
    };
    Ok(JumpMenuArgs {
        global,
        connection_name,
        idle_timeout_ms,
        total_timeout_ms,
    })
}

fn run_jump_menu(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_jump_menu_args(argv)?;
    if parsed.global.help {
        return print_help("jump-menu");
    }
    if parsed.global.version {
        return print_version();
    }
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    let jump_config = connection
        .jump_server
        .as_ref()
        .ok_or_else(|| {
            AppError::new(format!(
                "连接 {} 未配置 jumpServer，无法使用 jump-menu",
                connection.name
            ))
        })?;
    if !jump_config.enabled {
        return Err(AppError::new(format!(
            "连接 {} 的 jumpServer.enabled 为 false，拒绝使用 jump-menu",
            connection.name
        )));
    }
    let result = run_with_timeout(
        jump_runtime_timeout_ms(parsed.total_timeout_ms),
        show_jump_menu_async(
            &configs,
            connection,
            jump_config,
            parsed.idle_timeout_ms,
            parsed.total_timeout_ms,
        ),
    )?;
    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
}

fn run_jump_search(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_jump_search_args(argv)?;
    if parsed.global.help {
        return print_help("jump-search");
    }
    if parsed.global.version {
        return print_version();
    }
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    let jump_config = connection
        .jump_server
        .as_ref()
        .ok_or_else(|| {
            AppError::new(format!(
                "连接 {} 未配置 jumpServer，无法使用 jump-search",
                connection.name
            ))
        })?;
    if !jump_config.enabled {
        return Err(AppError::new(format!(
            "连接 {} 的 jumpServer.enabled 为 false，拒绝使用 jump-search",
            connection.name
        )));
    }
    let result = run_with_timeout(
        jump_runtime_timeout_ms(parsed.total_timeout_ms),
        search_jump_targets_async(
            &configs,
            connection,
            jump_config,
            &parsed.query,
            parsed.idle_timeout_ms,
            parsed.total_timeout_ms,
        ),
    )?;
    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
}

fn run_jump_exec(argv: Vec<String>) -> AppResult<()> {
    let parsed = parse_jump_exec_args(argv)?;
    if parsed.global.help {
        return print_help("jump-exec");
    }
    if parsed.global.version {
        return print_version();
    }
    prepare_connection_config(&parsed.global.config_path, &parsed.connection_name)?;
    let configs = load_config_for_connection(&parsed.global.config_path, &parsed.connection_name)?;
    let connection = find_connection(&configs, &parsed.connection_name)?;
    let jump_config = connection
        .jump_server
        .as_ref()
        .ok_or_else(|| {
            AppError::new(format!(
                "连接 {} 未配置 jumpServer，无法使用 jump-exec",
                connection.name
            ))
        })?;
    if !jump_config.enabled {
        return Err(AppError::new(format!(
            "连接 {} 的 jumpServer.enabled 为 false，拒绝使用 jump-exec",
            connection.name
        )));
    }
    let command = resolve_command_from_file_or_inline(&configs, &parsed.command, parsed.command_file.as_ref())?;
    validate_command(connection, &command)?;
    let result = if parsed.global.no_cache {
        run_with_timeout(
            jump_runtime_timeout_ms(parsed.total_timeout_ms),
            execute_via_jumpserver_async(
                &configs,
                connection,
                jump_config,
                &parsed.target,
                &command,
                CommandTimeouts {
                    idle_timeout_ms: parsed.idle_timeout_ms,
                    total_timeout_ms: parsed.total_timeout_ms,
                },
            ),
        )?
    } else {
        request_daemon_jump_execute(&parsed, &command)?
    };
    if !result.is_empty() {
        println!("{}", result);
    }
    Ok(())
}

async fn channel_send_slow(
    channel: &mut Channel<client::Msg>,
    text: &str,
    char_delay_ms: u64,
) -> AppResult<()> {
    for byte in text.as_bytes() {
        let buf = [*byte];
        channel
            .data(&buf[..])
            .await
            .map_err(|error| AppError::new(format!("写入跳板机会话失败: {}", error)))?;
        if char_delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(char_delay_ms)).await;
        }
    }
    let cr = [b'\r'];
    channel
        .data(&cr[..])
        .await
        .map_err(|error| AppError::new(format!("写入跳板机回车失败: {}", error)))?;
    Ok(())
}

async fn channel_send_line(channel: &mut Channel<client::Msg>, text: &str) -> AppResult<()> {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\r');
    channel
        .data(&bytes[..])
        .await
        .map_err(|error| AppError::new(format!("写入跳板机命令失败: {}", error)))?;
    Ok(())
}

/// 累积 channel 输出直到匹配指定正则；返回累积的全部 raw 输出。
async fn expect_regex(
    channel: &mut Channel<client::Msg>,
    buffer: &mut String,
    pattern: &Regex,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
    label: &str,
) -> AppResult<()> {
    loop {
        let cleaned = strip_ansi(buffer);
        if pattern.is_match(&cleaned) {
            return Ok(());
        }
        if let Some(deadline) = total_deadline {
            let now = tokio::time::Instant::now();
            if now >= deadline {
                return Err(AppError::new(format!("等待 {} 达到总超时上限", label)));
            }
        }
        let total_remaining = total_deadline.and_then(|deadline| {
            let now = tokio::time::Instant::now();
            (deadline > now).then_some(deadline - now)
        });
        let wait_budget = match total_remaining {
            Some(remaining) => remaining.min(Duration::from_millis(idle_timeout_ms)),
            None => Duration::from_millis(idle_timeout_ms),
        };
        let msg = match tokio::time::timeout(wait_budget, channel.wait()).await {
            Ok(Some(msg)) => msg,
            Ok(None) => return Err(AppError::new(format!("等待 {} 时跳板机连接关闭", label))),
            Err(_) => return Err(AppError::new(format!("等待 {} 空闲超时", label))),
        };
        match msg {
            ChannelMsg::Data { data } => {
                buffer.push_str(&String::from_utf8_lossy(&data));
            }
            ChannelMsg::ExtendedData { data, .. } => {
                buffer.push_str(&String::from_utf8_lossy(&data));
            }
            ChannelMsg::Eof | ChannelMsg::Close => {
                return Err(AppError::new(format!("等待 {} 时跳板机会话结束", label)))
            }
            _ => {}
        }
    }
}

fn total_deadline_from_ms(total_timeout_ms: Option<u64>) -> Option<tokio::time::Instant> {
    total_timeout_ms
        .map(|timeout_ms| tokio::time::Instant::now() + Duration::from_millis(timeout_ms))
}

fn remaining_before_deadline(deadline: tokio::time::Instant) -> Option<u64> {
    let now = tokio::time::Instant::now();
    Some(if deadline > now {
        (deadline - now).as_millis().max(1) as u64
    } else {
        1
    })
}

fn phase_timeout_ms(idle_timeout_ms: u64, total_deadline: Option<tokio::time::Instant>) -> u64 {
    match total_deadline.and_then(remaining_before_deadline) {
        Some(remaining_ms) => remaining_ms.min(idle_timeout_ms).max(1),
        None => idle_timeout_ms,
    }
}

fn command_runtime_timeout_ms(idle_timeout_ms: u64, total_timeout_ms: Option<u64>) -> u64 {
    total_timeout_ms.unwrap_or(DAEMON_REQUEST_TIMEOUT_MS).max(idle_timeout_ms)
}

fn jump_runtime_timeout_ms(total_timeout_ms: Option<u64>) -> u64 {
    total_timeout_ms.unwrap_or(DAEMON_REQUEST_TIMEOUT_MS)
}

/// 识别 bash 交互 shell 的 job control 通知行：
///   `[1] 12345`、`[1]+ Done sleep`、`[2]- Running ( ... )`、`[1]+ Terminated ...` 等。
/// 这些行不是用户命令的真实输出，jump-exec wrapper 内的后台进程会触发它们，需要过滤。
fn is_job_control_notice(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.first() != Some(&b'[') {
        return false;
    }
    let mut idx = 1usize;
    let mut had_digit = false;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        had_digit = true;
        idx += 1;
    }
    if !had_digit || idx >= bytes.len() || bytes[idx] != b']' {
        return false;
    }
    idx += 1;
    if idx < bytes.len() && (bytes[idx] == b'+' || bytes[idx] == b'-') {
        idx += 1;
    }
    // 必须紧跟空白，至少有一段空白后才能算 job 状态行
    if idx >= bytes.len() || !bytes[idx].is_ascii_whitespace() {
        return false;
    }
    true
}

fn extract_marker_output(
    buffer: &str,
    start_marker: &str,
    end_marker: &str,
) -> AppResult<(String, i32)> {
    let cleaned = strip_ansi(buffer);
    let start_idx = cleaned
        .rfind(start_marker)
        .ok_or_else(|| AppError::new("未在跳板机输出中找到起始 marker"))?;
    let after_start = &cleaned[start_idx + start_marker.len()..];
    let end_pattern = Regex::new(&format!(r"{}:(-?\d+)", regex::escape(end_marker)))
        .map_err(|error| AppError::new(format!("构造 end marker 正则失败: {}", error)))?;
    let captures = end_pattern
        .captures(after_start)
        .ok_or_else(|| AppError::new("未在跳板机输出中找到结束 marker"))?;
    let end_match = captures.get(0).unwrap();
    let exit_code: i32 = captures
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    let body = &after_start[..end_match.start()];
    let mut lines = Vec::new();
    for line in body.split('\n') {
        let stripped = line.trim_end_matches('\r').to_string();
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("__JUMP_START_") || trimmed.contains("__JUMP_END_") {
            continue;
        }
        if trimmed.contains(HEARTBEAT_MARKER) {
            continue;
        }
        if trimmed == EXEC_HEARTBEAT_MARKER {
            continue;
        }
        // bash 交互 shell 的 job control 通知 ("[1] 12345", "[1]+ Done ...", "[1]- Running ..."），
        // 这些行由 wrapper 的后台心跳/用户命令产生，不是真实业务输出，统一过滤。
        if is_job_control_notice(trimmed) {
            continue;
        }
        if trimmed.starts_with("printf '") || trimmed.starts_with("printf \"") {
            continue;
        }
        lines.push(stripped);
    }
    Ok((lines.join("\n").trim().to_string(), exit_code))
}

async fn execute_via_jumpserver_async(
    configs: &[Connection],
    connection: &Connection,
    jump: &JumpServerConfig,
    target: &str,
    command: &str,
    timeouts: CommandTimeouts,
) -> AppResult<String> {
    let total_deadline = total_deadline_from_ms(timeouts.total_timeout_ms);
    let session = connect_russh(configs, connection).await?;
    let result = execute_via_jumpserver_with_session(
        &session,
        connection,
        jump,
        target,
        command,
        timeouts.idle_timeout_ms,
        total_deadline,
    )
    .await;
    let _ = tokio::time::timeout(
        Duration::from_millis(phase_timeout_ms(timeouts.idle_timeout_ms.min(5000).max(1), total_deadline)),
        session.disconnect(Disconnect::ByApplication, "", "English"),
    )
    .await;
    result
}

async fn search_jump_targets_async(
    configs: &[Connection],
    connection: &Connection,
    jump: &JumpServerConfig,
    query: &str,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
) -> AppResult<String> {
    let total_deadline = total_deadline_from_ms(total_timeout_ms);
    let session = connect_russh(configs, connection).await?;
    let result = search_jump_targets_with_session(
        &session,
        connection,
        jump,
        query,
        idle_timeout_ms,
        total_deadline,
    )
    .await;
    let _ = tokio::time::timeout(
        Duration::from_millis(phase_timeout_ms(idle_timeout_ms.min(5000).max(1), total_deadline)),
        session.disconnect(Disconnect::ByApplication, "", "English"),
    )
    .await;
    result
}

async fn show_jump_menu_async(
    configs: &[Connection],
    connection: &Connection,
    jump: &JumpServerConfig,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
) -> AppResult<String> {
    let total_deadline = total_deadline_from_ms(total_timeout_ms);
    let session = connect_russh(configs, connection).await?;
    let result = show_jump_menu_with_session(
        &session,
        connection,
        jump,
        idle_timeout_ms,
        total_deadline,
    )
    .await;
    let _ = tokio::time::timeout(
        Duration::from_millis(phase_timeout_ms(idle_timeout_ms.min(5000).max(1), total_deadline)),
        session.disconnect(Disconnect::ByApplication, "", "English"),
    )
    .await;
    result
}

async fn execute_via_jumpserver_with_session(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    jump: &JumpServerConfig,
    target: &str,
    command: &str,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<String> {
    let (mut channel, mut buffer) =
        setup_jump_channel(session, connection, jump, target, idle_timeout_ms, total_deadline).await?;
    let result = run_jump_command(
        &mut channel,
        &mut buffer,
        command,
        idle_timeout_ms,
        total_deadline,
    )
    .await;

    // 退出目标 shell（best-effort）
    let _ = channel_send_line(&mut channel, "exit").await;

    let (output, exit_code) = result?;
    if exit_code != 0 {
        return Err(AppError::new(format_exit_failure(&output, exit_code)));
    }
    Ok(output)
}

async fn open_jump_menu_channel(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    jump: &JumpServerConfig,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<(Channel<client::Msg>, String, Regex)> {
    let mut channel = session.channel_open_session().await.map_err(|error| {
        AppError::new(format!(
            "连接 {} 打开跳板机会话失败: {}",
            connection.name, error
        ))
    })?;
    channel
        .request_pty(true, "xterm", 200, 50, 0, 0, &[])
        .await
        .map_err(|error| {
            AppError::new(format!(
                "连接 {} 跳板机分配 PTY 失败: {}",
                connection.name, error
            ))
        })?;
    channel.request_shell(true).await.map_err(|error| {
        AppError::new(format!(
            "连接 {} 跳板机请求交互 shell 失败: {}",
            connection.name, error
        ))
    })?;

    let prompt_re = Regex::new(&jump.prompt_regex)
        .map_err(|error| AppError::new(format!("promptRegex 非法: {}", error)))?;

    let mut buffer = String::new();
    expect_regex(
        &mut channel,
        &mut buffer,
        &prompt_re,
        idle_timeout_ms.min(JUMP_MENU_PROMPT_MAX_MS).max(1),
        total_deadline,
        "JumpServer 菜单 prompt",
    )
    .await?;
    Ok((channel, buffer, prompt_re))
}

/// 建立到目标主机的 PTY 通道并完成菜单/搜索进入流程，返回可复用的 channel + 输出 buffer。
async fn setup_jump_channel(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    jump: &JumpServerConfig,
    target: &str,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<(Channel<client::Msg>, String)> {
    let (mut channel, mut buffer, _) =
        open_jump_menu_channel(session, connection, jump, idle_timeout_ms, total_deadline).await?;
    let shell_re = Regex::new(&jump.shell_prompt_regex)
        .map_err(|error| AppError::new(format!("shellPromptRegex 非法: {}", error)))?;
    // 进入目标主机
    buffer.clear();
    channel_send_slow(&mut channel, target, jump.char_delay_ms).await?;
    let direct_result = expect_regex(
        &mut channel,
        &mut buffer,
        &shell_re,
        idle_timeout_ms.min(JUMP_DIRECT_ATTEMPT_MAX_MS).max(1),
        total_deadline,
        "目标主机 shell prompt (direct)",
    )
    .await;
    let entered = direct_result.is_ok();
    if !entered {
        if jump.enter_strategy == "direct" {
            return Err(AppError::new(format!(
                "无法进入目标主机 {}（direct 模式超时）",
                target
            )));
        }
        // direct-then-search: 发 search 前缀
        buffer.clear();
        let search_text = format!("{}{}", jump.search_prefix, target);
        channel_send_slow(&mut channel, &search_text, jump.char_delay_ms).await?;
        // 一些 JumpServer 需要再回车一次确认搜索结果
        let first = expect_regex(
            &mut channel,
            &mut buffer,
            &shell_re,
            idle_timeout_ms,
            total_deadline,
            "目标主机 shell prompt (search)",
        )
        .await;
        if first.is_err() {
            channel_send_line(&mut channel, "").await?;
            expect_regex(
                &mut channel,
                &mut buffer,
                &shell_re,
                idle_timeout_ms,
                total_deadline,
                "目标主机 shell prompt (search 二次回车)",
            )
            .await?;
        }
    }
    Ok((channel, buffer))
}

fn build_jump_search_text(query: &str, search_prefix: &str) -> String {
    let trimmed = query.trim();
    if search_prefix.is_empty() || trimmed.starts_with(search_prefix) {
        trimmed.to_string()
    } else {
        format!("{}{}", search_prefix, trimmed)
    }
}

fn extract_jump_search_output(
    buffer: &str,
    search_text: &str,
    prompt_re: &Regex,
) -> AppResult<String> {
    let cleaned = strip_ansi(buffer);
    let mut lines = Vec::new();
    for raw_line in cleaned.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if prompt_re.is_match(trimmed) {
            continue;
        }
        if trimmed == search_text {
            continue;
        }
        lines.push(line.to_string());
    }
    let output = lines.join("\n").trim().to_string();
    if output.is_empty() {
        return Err(AppError::new("JumpServer 搜索未返回可识别的候选结果"));
    }
    Ok(output)
}

fn extract_jump_menu_output(buffer: &str, prompt_re: &Regex) -> AppResult<String> {
    let cleaned = strip_ansi(buffer);
    let mut lines = Vec::new();
    for raw_line in cleaned.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if prompt_re.is_match(trimmed) {
            continue;
        }
        lines.push(line.to_string());
    }
    let output = lines.join("\n").trim().to_string();
    if output.is_empty() {
        return Err(AppError::new("JumpServer 菜单未返回可识别内容"));
    }
    Ok(output)
}

async fn show_jump_menu_with_session(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    jump: &JumpServerConfig,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<String> {
    let (_channel, buffer, prompt_re) =
        open_jump_menu_channel(session, connection, jump, idle_timeout_ms, total_deadline).await?;
    extract_jump_menu_output(&buffer, &prompt_re)
}

async fn search_jump_targets_with_session(
    session: &client::Handle<RusshClient>,
    connection: &Connection,
    jump: &JumpServerConfig,
    query: &str,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<String> {
    let (mut channel, mut buffer, prompt_re) =
        open_jump_menu_channel(session, connection, jump, idle_timeout_ms, total_deadline).await?;
    let search_text = build_jump_search_text(query, &jump.search_prefix);
    buffer.clear();
    channel_send_slow(&mut channel, &search_text, jump.char_delay_ms).await?;
    let first = expect_regex(
        &mut channel,
        &mut buffer,
        &prompt_re,
        idle_timeout_ms,
        total_deadline,
        "JumpServer 搜索结果",
    )
    .await;
    if first.is_err() {
        channel_send_line(&mut channel, "").await?;
        expect_regex(
            &mut channel,
            &mut buffer,
            &prompt_re,
            idle_timeout_ms,
            total_deadline,
            "JumpServer 搜索结果（二次回车）",
        )
        .await?;
    }
    extract_jump_search_output(&buffer, &search_text, &prompt_re)
}

/// 在已经进入目标 shell 的 channel 上发送 marker-wrapped 命令并解析结果。
/// 复用 buffer，调用前会先 clear，确保不残留上一次命令的输出。
/// 返回 (stdout, exit_code)。channel/marker 层面的错误才返回 Err；命令本身非零退出仍是 Ok，
/// 由调用方决定如何上报，避免误把业务退出码当作连接坏掉。
async fn run_jump_command(
    channel: &mut Channel<client::Msg>,
    buffer: &mut String,
    command: &str,
    idle_timeout_ms: u64,
    total_deadline: Option<tokio::time::Instant>,
) -> AppResult<(String, i32)> {
    // 发送 marker 包装命令
    let start_marker = format!("__JUMP_START_{}__", uuid_hex());
    let end_marker = format!("__JUMP_END_{}__", uuid_hex());
    // 后台心跳：长命令期间每 20s 向 PTY 写一行 __JUMP_HB__ 占位，避免 JumpServer 把
    // 空闲会话当成 idle 主动关闭；extract_marker_output 会按行过滤掉这些占位。
    // - `set +m` 关闭 shell job control，避免 [1] pid / Done 等异步任务提示泄漏到 stdout。
    // - 心跳必须打到 stdout 才能让 JumpServer 看见字节流，这里只过滤标识行而不重定向。
    // - 用 `\\$!` 等通过外层 format 输出 `$!` 进入 shell，由 shell 自身展开。
    let wrapped_command = wrap_command_with_heartbeat(command, EXEC_HEARTBEAT_MARKER);
    let wrapped = format!(
        "{{ set +m; printf '{start}\\n'; ( {cmd} ) & __jhpid=$!; ( while kill -0 $__jhpid 2>/dev/null; do printf '{hb}\\n'; sleep 20; done ) & __jhhb=$!; wait $__jhpid 2>/dev/null; __jhstatus=$?; kill $__jhhb >/dev/null 2>&1; wait $__jhhb >/dev/null 2>&1; printf '\\n{end}:%s\\n' \"$__jhstatus\"; }} 2>&1",
        start = start_marker,
        cmd = wrapped_command,
        hb = HEARTBEAT_MARKER,
        end = end_marker,
    );
    buffer.clear();
    channel_send_line(channel, &wrapped).await?;

    let end_pattern = Regex::new(&format!(r"{}:-?\d+", regex::escape(&end_marker)))
        .map_err(|error| AppError::new(format!("构造结束 marker 正则失败: {}", error)))?;
    // 命令执行阶段沿用外层 --timeout 预算（最低 10s），保证用户调大 timeout 时命令段也跟着放大
    expect_regex(
        channel,
        buffer,
        &end_pattern,
        idle_timeout_ms,
        total_deadline,
        "命令执行结果",
    )
    .await?;

    extract_marker_output(buffer, &start_marker, &end_marker)
}

fn format_exit_failure(output: &str, exit_code: i32) -> String {
    let mut parts = Vec::new();
    if !output.is_empty() {
        parts.push(output.to_string());
    }
    parts.push(format!("[exit code] {}", exit_code));
    parts.join("\n")
}

fn strip_exec_heartbeat_lines(output: &str) -> String {
    output
        .lines()
        .filter(|line| line.trim() != EXEC_HEARTBEAT_MARKER)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn uuid_hex() -> String {
    // 使用 OsRng 生成 16 字节随机数转为 hex，避免引入新的 uuid 依赖
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    let mut s = String::with_capacity(32);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn default_jump_blacklist() -> Vec<serde_json::Value> {
    [
        r"(^|[;&|()\s])rm(\s|$)",
        r"(^|[;&|()\s])truncate(\s|$)",
        r"(^|[;&|()\s])reboot(\s|$)",
        r"(^|[;&|()\s])shutdown(\s|$)",
        r"(^|[;&|()\s])systemctl\s+(stop|restart|reload)(\s|$)",
        r"(^|[;&|()\s])kill(\s|$)",
    ]
    .iter()
    .map(|s| serde_json::Value::String((*s).to_string()))
    .collect()
}

fn build_jump_server_entry(
    name: &str,
    host: &str,
    port: u16,
    username: &str,
    private_key: &str,
) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "host": host,
        "port": port,
        "username": username,
        "privateKey": private_key,
        "pty": true,
        "jumpServer": {
            "enabled": true,
            "promptRegex": "Opt>\\s*$",
            "shellPromptRegex": "(?m)[#$]\\s*$",
            "searchPrefix": "/",
            "charDelayMs": 60,
            "enterStrategy": "direct-then-search",
        },
        "commandBlacklist": default_jump_blacklist(),
    })
}

fn validate_jump_server_private_key(private_key: &str) -> AppResult<()> {
    let key_path = Path::new(private_key);
    if !key_path.exists() {
        return Err(AppError::new(format!(
            "私钥不存在: {}",
            key_path.display()
        )));
    }
    fs::metadata(key_path).map_err(|error| {
        AppError::new(format!(
            "私钥不可读: {}，{}",
            key_path.display(),
            error
        ))
    })?;
    load_secret_key(private_key, None).map_err(|error| {
        AppError::new(format!(
            "私钥无法被当前 SSH 栈加载: {}，{}",
            key_path.display(),
            error
        ))
    })?;
    Ok(())
}

fn run_add_jump_server(argv: Vec<String>) -> AppResult<()> {
    let global = parse_global_args(argv)?;
    if global.help {
        return print_help("add-jump-server");
    }
    if global.version {
        return print_version();
    }
    let mut args = global.args.clone();
    let name = take_option(&mut args, &["--name"])?
        .ok_or_else(|| AppError::new("缺少 --name 参数"))?;
    let host = take_option(&mut args, &["--host"])?
        .ok_or_else(|| AppError::new("缺少 --host 参数"))?;
    let port_value = take_option(&mut args, &["--port"])?;
    let username = take_option(&mut args, &["--username", "-u"])?
        .ok_or_else(|| AppError::new("缺少 --username 参数"))?;
    let private_key = take_option(&mut args, &["--private-key", "-k"])?
        .ok_or_else(|| AppError::new("缺少 --private-key 参数"))?;
    let force = args.iter().any(|item| item == "--force");
    let dry_run = args.iter().any(|item| item == "--dry-run");
    args.retain(|item| item != "--force" && item != "--dry-run");
    ensure_no_unknown_options(&args)?;
    ensure_no_extra_positionals(&args)?;

    let name = name.trim().to_string();
    let host = host.trim().to_string();
    let username = username.trim().to_string();
    let private_key = private_key.trim().to_string();
    if name.is_empty() {
        return Err(AppError::new("--name 不能为空"));
    }
    if host.is_empty() {
        return Err(AppError::new("--host 不能为空"));
    }
    if username.is_empty() {
        return Err(AppError::new("--username 不能为空"));
    }
    if private_key.is_empty() {
        return Err(AppError::new("--private-key 不能为空"));
    }

    let port = match port_value {
        Some(value) => value
            .parse::<u16>()
            .map_err(|_| AppError::new("--port 必须是 1-65535 的整数"))?,
        None => 8390,
    };
    if port == 0 {
        return Err(AppError::new("--port 不能为 0"));
    }

    validate_jump_server_private_key(&private_key)?;

    let config_path = global.config_path.clone();
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut entries: Vec<serde_json::Value> = if config_path.exists() {
        let raw = fs::read_to_string(&config_path)?;
        if raw.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&raw).map_err(|error| {
                AppError::new(format!(
                    "解析现有配置失败: {}，{}",
                    config_path.display(),
                    error
                ))
            })?
        }
    } else {
        Vec::new()
    };

    let new_entry = build_jump_server_entry(&name, &host, port, &username, &private_key);
    let existing_index = entries.iter().position(|item| {
        item.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n == name)
            .unwrap_or(false)
    });
    match existing_index {
        Some(idx) => {
            if !force {
                return Err(AppError::new(format!(
                    "连接名已存在: {}，使用 --force 覆盖",
                    name
                )));
            }
            entries[idx] = new_entry;
        }
        None => entries.push(new_entry),
    }

    if dry_run {
        println!("参数校验通过：");
        println!("- 连接名: {}", name);
        println!("- 地址: {}:{}", host, port);
        println!("- 用户: {}", username);
        println!("- 私钥可读且可被当前 SSH 栈加载: {}", private_key);
        println!(
            "- 连接名{}",
            if existing_index.is_some() { "已存在，正式写入时需配合 --force" } else { "不冲突" }
        );
        println!("去掉 --dry-run 后即可正式写入配置。");
        return Ok(());
    }

    let raw = serde_json::to_vec_pretty(&entries)?;
    write_private_file(&config_path, &raw)?;
    println!(
        "已写入 {} 连接 {} 到 {}",
        if existing_index.is_some() {
            "覆盖"
        } else {
            "新增"
        },
        name,
        config_path.display()
    );
    println!("下一步建议先执行: agentsshcli jump-menu {}", name);
    println!("确认 Opt 菜单后，再执行: agentsshcli jump-exec {} --target <hostOrIp> \"<command>\"", name);
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const TEST_OPENSSH_PRIVATE_KEY: &str = "-----BEGIN OPENSSH PRIVATE KEY-----\n\
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n\
QyNTUxOQAAACDi33mW7HNNNR/aW2QMsLQUlR+gUFzgVquBUkyBMzMuIgAAAKhHRBpPR0Qa\n\
TwAAAAtzc2gtZWQyNTUxOQAAACDi33mW7HNNNR/aW2QMsLQUlR+gUFzgVquBUkyBMzMuIg\n\
AAAEDyeXVh86D5MiczKIZGgs3LJvraN+0zffuRbHKrv5gzlOLfeZbsc001H9pbZAywtBSV\n\
H6BQXOBWq4FSTIEzMy4iAAAAImxpdXp4QGxpdXpoaXhpbmRlTWFjQm9vay1Qcm8ubG9jYW\n\
wBAgM=\n\
-----END OPENSSH PRIVATE KEY-----\n";

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
        assert_eq!(parsed.idle_timeout_ms, 30000);
        assert_eq!(parsed.total_timeout_ms, None);
    }

    #[test]
    fn parse_exec_supports_named_arguments() {
        let parsed = parse_execute_args(vec![
            "--no-cache".into(),
            "--pty".into(),
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
        assert_eq!(parsed.idle_timeout_ms, 1000);
        assert_eq!(parsed.total_timeout_ms, None);
        assert_eq!(parsed.pty, Some(true));
    }

    #[test]
    fn parse_exec_supports_total_timeout() {
        let parsed = parse_execute_args(vec![
            "--connection".into(),
            "server".into(),
            "--command".into(),
            "pwd".into(),
            "--timeout".into(),
            "1000".into(),
            "--total-timeout".into(),
            "20000".into(),
        ])
        .unwrap();
        assert_eq!(parsed.idle_timeout_ms, 1000);
        assert_eq!(parsed.total_timeout_ms, Some(20000));
    }

    #[test]
    fn parse_exec_rejects_conflicting_pty_flags() {
        let err = parse_execute_args(vec![
            "--pty".into(),
            "--no-pty".into(),
            "server".into(),
            "pwd".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("--pty 和 --no-pty"));
    }

    #[test]
    fn load_config_rejects_agent_auth() {
        let (_dir, path) = write_config(
            r#"[
              {"name":"a","host":"127.0.0.1","username":"root","agent":"/tmp/agent.sock"}
            ]"#,
        );
        let err = load_config(&path).unwrap_err();
        assert!(err
            .to_string()
            .contains("password、passwordRef 或 privateKey"));
    }

    #[test]
    fn passive_password_migration_hides_plain_password() {
        let (_dir, path) = write_config(
            r#"[{"name":"server","host":"127.0.0.1","username":"root","password":"secret"}]"#,
        );
        assert!(migrate_plain_password_for_connection(&path, "server").unwrap());
        let raw = fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("secret"));
        assert!(raw.contains(r#""password": """#));
        assert!(raw.contains(r#""passwordRef": "agentsshcli:server""#));
        let configs = load_config_for_connection(&path, "server").unwrap();
        let connection = find_connection(&configs, "server").unwrap();
        assert_eq!(connection.password.as_deref(), Some("secret"));
    }

    #[test]
    fn load_config_for_connection_ignores_unrelated_missing_password_ref() {
        let (_dir, path) = write_config(
            r#"[
              {"name":"key-server","host":"127.0.0.1","username":"root","privateKey":"/tmp/id_rsa"},
              {"name":"bad-password-server","host":"127.0.0.2","username":"root","password":"","passwordRef":"agentsshcli:missing"}
            ]"#,
        );
        let configs = load_config_for_connection(&path, "key-server").unwrap();
        let connection = find_connection(&configs, "key-server").unwrap();
        assert_eq!(connection.private_key.as_deref(), Some("/tmp/id_rsa"));
    }

    #[test]
    fn load_config_for_connection_resolves_only_target_password_ref() {
        let (_dir, path) = write_config(
            r#"[
              {"name":"target","host":"127.0.0.1","username":"root","password":"secret"},
              {"name":"bad-password-server","host":"127.0.0.2","username":"root","password":"","passwordRef":"agentsshcli:missing"}
            ]"#,
        );
        assert!(migrate_plain_password_for_connection(&path, "target").unwrap());
        let configs = load_config_for_connection(&path, "target").unwrap();
        let connection = find_connection(&configs, "target").unwrap();
        assert_eq!(connection.password.as_deref(), Some("secret"));
    }

    #[test]
    fn passive_password_migration_skips_empty_password() {
        let (_dir, path) = write_config(
            r#"[{"name":"server","host":"127.0.0.1","username":"root","password":"","passwordRef":"agentsshcli:server"}]"#,
        );
        assert!(!migrate_plain_password_for_connection(&path, "server").unwrap());
    }

    #[test]
    fn config_snapshot_detects_metadata_and_hash_changes() {
        let (_dir, path) =
            write_config(r#"[{"name":"a","host":"127.0.0.1","username":"root","password":"p"}]"#);
        let snapshot = ConfigSnapshot::read(&path).unwrap();
        assert!(snapshot.metadata_matches(&path).unwrap());
        std::thread::sleep(Duration::from_millis(5));
        fs::write(
            &path,
            r#"[{"name":"b","host":"127.0.0.1","username":"root","password":"p"}]"#,
        )
        .unwrap();
        let changed = ConfigSnapshot::read(&path).unwrap();
        assert_ne!(snapshot.hash, changed.hash);
    }

    #[test]
    fn resolve_pty_prefers_cli_then_config_then_default_false() {
        let connection = normalize_entry(
            serde_json::from_str(
                r#"{"name":"a","host":"127.0.0.1","username":"root","password":"p","pty":true}"#,
            )
            .unwrap(),
            0,
        )
        .unwrap();
        assert!(resolve_pty(&connection, None));
        assert!(!resolve_pty(&connection, Some(false)));
        let default_connection = normalize_entry(
            serde_json::from_str(
                r#"{"name":"b","host":"127.0.0.1","username":"root","password":"p"}"#,
            )
            .unwrap(),
            0,
        )
        .unwrap();
        assert!(!resolve_pty(&default_connection, None));
    }

    #[test]
    fn parse_exec_supports_command_file() {
        let parsed = parse_execute_args(vec![
            "--connection".into(),
            "server".into(),
            "--command-file".into(),
            "script.sh".into(),
        ])
        .unwrap();
        assert_eq!(parsed.connection_name, "server");
        assert_eq!(parsed.command_file.as_deref(), Some("script.sh"));
        assert_eq!(parsed.command, "");
    }

    #[test]
    fn parse_exec_rejects_mixed_command_sources() {
        let err = parse_execute_args(vec![
            "--connection".into(),
            "server".into(),
            "--command".into(),
            "pwd".into(),
            "--command-file".into(),
            "script.sh".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("--command 和 --command-file"));
    }

    #[test]
    fn resolve_exec_reads_multiline_command_file() {
        let original_dir = env::current_dir().unwrap();
        let dir = tempdir().unwrap();
        let command_file = dir.path().join("script.sh");
        fs::write(&command_file, "echo start\necho end\n").unwrap();
        env::set_current_dir(dir.path()).unwrap();
        let connection = normalize_entry(
            serde_json::from_str(
                r#"{"name":"server","host":"127.0.0.1","username":"root","password":"p"}"#,
            )
            .unwrap(),
            0,
        )
        .unwrap();
        let parsed = parse_execute_args(vec![
            "--connection".into(),
            "server".into(),
            "--command-file".into(),
            "script.sh".into(),
        ])
        .unwrap();
        let command = resolve_execute_command(&[connection], &parsed).unwrap();
        env::set_current_dir(original_dir).unwrap();
        assert_eq!(command, "echo start\necho end\n");
    }

    #[test]
    fn socks_proxy_supports_host_port_without_scheme() {
        let proxy = parse_socks_proxy("127.0.0.1:1080").unwrap();
        assert_eq!(proxy.host, "127.0.0.1");
        assert_eq!(proxy.port, 1080);
    }

    #[test]
    fn load_config_accepts_missing_jumpserver_field() {
        let (_dir, path) = write_config(
            r#"[{"name":"a","host":"127.0.0.1","username":"root","password":"p"}]"#,
        );
        let configs = load_config(&path).unwrap();
        assert_eq!(configs.len(), 1);
        assert!(configs[0].jump_server.is_none());
    }

    #[test]
    fn load_config_parses_jumpserver_field() {
        let (_dir, path) = write_config(
            r#"[{
              "name":"gw","host":"127.0.0.1","username":"root","password":"p",
              "jumpServer":{
                "enabled":true,
                "promptRegex":"Opt>\\s*$",
                "shellPromptRegex":"(?m)[#$]\\s*$",
                "searchPrefix":"/",
                "charDelayMs":40,
                "enterStrategy":"direct-then-search"
              }
            }]"#,
        );
        let configs = load_config(&path).unwrap();
        let jump = configs[0].jump_server.as_ref().unwrap();
        assert!(jump.enabled);
        assert_eq!(jump.shell_prompt_regex, "(?m)[#$]\\s*$");
        assert_eq!(jump.search_prefix, "/");
        assert_eq!(jump.char_delay_ms, 40);
        assert_eq!(jump.enter_strategy, "direct-then-search");
    }

    #[test]
    fn jump_menu_rejects_connection_without_jumpserver_enabled() {
        let (_dir, path) = write_config(
            r#"[{"name":"plain","host":"127.0.0.1","username":"root","password":"p"}]"#,
        );
        let err = run_jump_menu(vec![
            "--config".into(),
            path.to_string_lossy().into(),
            "plain".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("未配置 jumpServer"));
    }

    #[test]
    fn jump_search_rejects_connection_without_jumpserver_enabled() {
        let (_dir, path) = write_config(
            r#"[{"name":"plain","host":"127.0.0.1","username":"root","password":"p"}]"#,
        );
        let err = run_jump_search(vec![
            "--config".into(),
            path.to_string_lossy().into(),
            "plain".into(),
            "adserving".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("未配置 jumpServer"));
    }

    #[test]
    fn jump_exec_rejects_connection_without_jumpserver_enabled() {
        let (_dir, path) = write_config(
            r#"[{"name":"plain","host":"127.0.0.1","username":"root","password":"p"}]"#,
        );
        let err = run_jump_exec(vec![
            "--config".into(),
            path.to_string_lossy().into(),
            "plain".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("未配置 jumpServer"));
    }

    #[test]
    fn jump_exec_rejects_jumpserver_disabled() {
        let (_dir, path) = write_config(
            r#"[{
              "name":"gw","host":"127.0.0.1","username":"root","password":"p",
              "jumpServer":{"enabled":false}
            }]"#,
        );
        let err = run_jump_exec(vec![
            "--config".into(),
            path.to_string_lossy().into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("jumpServer.enabled 为 false"));
    }

    #[test]
    fn parse_jump_exec_supports_command_file() {
        let parsed = parse_jump_exec_args(vec![
            "--connection".into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "--command-file".into(),
            "script.sh".into(),
        ])
        .unwrap();
        assert_eq!(parsed.connection_name, "gw");
        assert_eq!(parsed.target, "host1");
        assert_eq!(parsed.command_file.as_deref(), Some("script.sh"));
        assert_eq!(parsed.command, "");
    }

    #[test]
    fn parse_jump_exec_defaults_to_cached_mode() {
        let parsed = parse_jump_exec_args(vec![
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap();
        assert!(!parsed.global.no_cache);
        assert_eq!(parsed.connection_name, "gw");
        assert_eq!(parsed.target, "host1");
        assert_eq!(parsed.command, "pwd");
        assert_eq!(parsed.idle_timeout_ms, 60000);
        assert_eq!(parsed.total_timeout_ms, None);
    }

    #[test]
    fn parse_jump_menu_defaults() {
        let parsed = parse_jump_menu_args(vec!["gw".into()]).unwrap();
        assert_eq!(parsed.connection_name, "gw");
        assert_eq!(parsed.idle_timeout_ms, 15000);
        assert_eq!(parsed.total_timeout_ms, None);
    }

    #[test]
    fn parse_jump_search_defaults() {
        let parsed = parse_jump_search_args(vec!["gw".into(), "adserving".into()]).unwrap();
        assert_eq!(parsed.connection_name, "gw");
        assert_eq!(parsed.query, "adserving");
        assert_eq!(parsed.idle_timeout_ms, 15000);
        assert_eq!(parsed.total_timeout_ms, None);
    }

    #[test]
    fn parse_jump_search_supports_named_query() {
        let parsed = parse_jump_search_args(vec![
            "--connection".into(),
            "gw".into(),
            "--query".into(),
            "api-02".into(),
            "--timeout".into(),
            "20000".into(),
            "--total-timeout".into(),
            "90000".into(),
        ])
        .unwrap();
        assert_eq!(parsed.connection_name, "gw");
        assert_eq!(parsed.query, "api-02");
        assert_eq!(parsed.idle_timeout_ms, 20000);
        assert_eq!(parsed.total_timeout_ms, Some(90000));
    }

    #[test]
    fn parse_jump_exec_supports_no_cache() {
        let parsed = parse_jump_exec_args(vec![
            "--no-cache".into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap();
        assert!(parsed.global.no_cache);
    }

    #[test]
    fn parse_jump_exec_supports_cache_ttl() {
        let parsed = parse_jump_exec_args(vec![
            "--cache-ttl".into(),
            "60000".into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap();
        assert_eq!(parsed.global.cache_ttl_ms, Some(60000));
    }

    #[test]
    fn parse_jump_exec_supports_total_timeout() {
        let parsed = parse_jump_exec_args(vec![
            "--timeout".into(),
            "15000".into(),
            "--total-timeout".into(),
            "180000".into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "pwd".into(),
        ])
        .unwrap();
        assert_eq!(parsed.idle_timeout_ms, 15000);
        assert_eq!(parsed.total_timeout_ms, Some(180000));
    }

    #[test]
    fn resolve_jump_exec_reads_multiline_command_file() {
        let dir = tempdir().unwrap();
        let command_file = dir.path().join("script.sh");
        fs::write(&command_file, "echo start\necho end\n").unwrap();
        let connection = normalize_entry(
            serde_json::from_str(
                &format!(
                    r#"{{"name":"gw","host":"127.0.0.1","username":"root","password":"p","allowedLocalPaths":["{}"],"jumpServer":{{"enabled":true}}}}"#,
                    dir.path().display()
                ),
            )
            .unwrap(),
            0,
        )
        .unwrap();
        let parsed = parse_jump_exec_args(vec![
            "--connection".into(),
            "gw".into(),
            "--target".into(),
            "host1".into(),
            "--command-file".into(),
            command_file.display().to_string(),
        ])
        .unwrap();
        let command =
            resolve_command_from_file_or_inline(&[connection], &parsed.command, parsed.command_file.as_ref())
                .unwrap();
        assert_eq!(command, "echo start\necho end\n");
    }

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1B[31mhello\x1B[0m world\x1B[1;32m!\x1B[0m";
        assert_eq!(strip_ansi(input), "hello world!");
    }

    #[test]
    fn extract_marker_output_parses_exit_code() {
        let buf = "noise\n__JUMP_START_abc__\nhello\nworld\n__JUMP_END_xyz__:0\nmore noise\n";
        let (out, code) = extract_marker_output(buf, "__JUMP_START_abc__", "__JUMP_END_xyz__")
            .unwrap();
        assert_eq!(code, 0);
        assert_eq!(out, "hello\nworld");
    }

    #[test]
    fn extract_marker_output_handles_nonzero_exit() {
        let buf = "__JUMP_START_a__\nbad\n__JUMP_END_b__:42\n";
        let (out, code) = extract_marker_output(buf, "__JUMP_START_a__", "__JUMP_END_b__")
            .unwrap();
        assert_eq!(code, 42);
        assert_eq!(out, "bad");
    }

    #[test]
    fn extract_marker_output_filters_heartbeat_and_job_control() {
        let buf = "__JUMP_START_a__\n[1] 12345\n__JUMP_HB__\nhello\n[1]+ Done ( sleep )\n__JUMP_HB__\n__JUMP_END_b__:0\n";
        let (out, code) = extract_marker_output(buf, "__JUMP_START_a__", "__JUMP_END_b__")
            .unwrap();
        assert_eq!(code, 0);
        assert_eq!(out, "hello");
    }

    #[test]
    fn extract_jump_menu_output_filters_prompt_lines() {
        let prompt = Regex::new(r"Opt>\s*$").unwrap();
        let out = extract_jump_menu_output(
            "欢迎使用 JumpServer\n1) 输入 /关键词 搜索\n2) 输入 p 查看节点\nOpt>\n",
            &prompt,
        )
        .unwrap();
        assert_eq!(out, "欢迎使用 JumpServer\n1) 输入 /关键词 搜索\n2) 输入 p 查看节点");
    }

    #[test]
    fn extract_jump_search_output_filters_prompt_lines() {
        let prompt = Regex::new(r"Opt>\s*$").unwrap();
        let out = extract_jump_search_output(
            "Opt> /adserving\n1) hwtf-adserving-api-01 172.31.1.10\n2) hwtf-adserving-api-02 172.31.1.11\nOpt>\n",
            "/adserving",
            &prompt,
        )
        .unwrap();
        assert_eq!(
            out,
            "Opt> /adserving\n1) hwtf-adserving-api-01 172.31.1.10\n2) hwtf-adserving-api-02 172.31.1.11"
        );
    }

    #[test]
    fn is_job_control_notice_matches_bash_output() {
        assert!(is_job_control_notice("[1] 12345"));
        assert!(is_job_control_notice("[2]+ Done ( cmd )"));
        assert!(is_job_control_notice("[3]- Running cmd"));
        assert!(!is_job_control_notice("[ok] value"));
        assert!(!is_job_control_notice("real output [1] mid line"));
        assert!(!is_job_control_notice(""));
    }

    #[test]
    fn build_jump_server_entry_has_expected_shape() {
        let entry = build_jump_server_entry("p.jump", "1.2.3.4", 8390, "alice", "/k.pem");
        assert_eq!(entry["name"], "p.jump");
        assert_eq!(entry["host"], "1.2.3.4");
        assert_eq!(entry["port"], 8390);
        assert_eq!(entry["username"], "alice");
        assert_eq!(entry["privateKey"], "/k.pem");
        assert_eq!(entry["pty"], true);
        assert_eq!(entry["jumpServer"]["enabled"], true);
        assert_eq!(entry["jumpServer"]["enterStrategy"], "direct-then-search");
        assert!(entry["commandBlacklist"].as_array().unwrap().len() >= 6);
    }

    fn write_test_private_key(path: &Path) {
        fs::write(path, TEST_OPENSSH_PRIVATE_KEY).unwrap();
    }

    #[test]
    fn add_jump_server_appends_to_new_config() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let key = dir.path().join("k.pem");
        write_test_private_key(&key);
        run_add_jump_server(vec![
            "--config".into(),
            cfg.display().to_string(),
            "--name".into(),
            "prod.jumpserver".into(),
            "--host".into(),
            "1.2.3.4".into(),
            "--port".into(),
            "8390".into(),
            "--username".into(),
            "alice".into(),
            "--private-key".into(),
            key.display().to_string(),
        ])
        .unwrap();
        let raw = fs::read_to_string(&cfg).unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "prod.jumpserver");
        assert_eq!(parsed[0]["jumpServer"]["enabled"], true);
    }

    #[test]
    fn add_jump_server_rejects_duplicate_without_force() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let key = dir.path().join("k.pem");
        write_test_private_key(&key);
        let base_args = || {
            vec![
                "--config".into(),
                cfg.display().to_string(),
                "--name".into(),
                "p".into(),
                "--host".into(),
                "1.1.1.1".into(),
                "--username".into(),
                "alice".into(),
                "--private-key".into(),
                key.display().to_string(),
            ]
        };
        run_add_jump_server(base_args()).unwrap();
        let err = run_add_jump_server(base_args()).unwrap_err();
        assert!(err.to_string().contains("已存在"));
        // 加 --force 应成功
        let mut force_args = base_args();
        force_args.push("--force".into());
        run_add_jump_server(force_args).unwrap();
    }

    #[test]
    fn add_jump_server_rejects_missing_private_key() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let err = run_add_jump_server(vec![
            "--config".into(),
            cfg.display().to_string(),
            "--name".into(),
            "p".into(),
            "--host".into(),
            "1.1.1.1".into(),
            "--username".into(),
            "alice".into(),
            "--private-key".into(),
            "/no/such/key.pem".into(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("私钥不存在"));
    }

    #[test]
    fn add_jump_server_rejects_blank_required_fields() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let key = dir.path().join("k.pem");
        write_test_private_key(&key);
        let err = run_add_jump_server(vec![
            "--config".into(),
            cfg.display().to_string(),
            "--name".into(),
            "   ".into(),
            "--host".into(),
            "1.1.1.1".into(),
            "--username".into(),
            "alice".into(),
            "--private-key".into(),
            key.display().to_string(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("--name 不能为空"));
    }

    #[test]
    fn add_jump_server_dry_run_does_not_write_config() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let key = dir.path().join("k.pem");
        write_test_private_key(&key);
        run_add_jump_server(vec![
            "--config".into(),
            cfg.display().to_string(),
            "--name".into(),
            "prod.jumpserver".into(),
            "--host".into(),
            "1.2.3.4".into(),
            "--username".into(),
            "alice".into(),
            "--private-key".into(),
            key.display().to_string(),
            "--dry-run".into(),
        ])
        .unwrap();
        assert!(!cfg.exists());
    }

    #[test]
    fn add_jump_server_rejects_unloadable_private_key() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.json");
        let key = dir.path().join("k.pem");
        fs::write(&key, "not-a-private-key").unwrap();
        let err = run_add_jump_server(vec![
            "--config".into(),
            cfg.display().to_string(),
            "--name".into(),
            "p".into(),
            "--host".into(),
            "1.1.1.1".into(),
            "--username".into(),
            "alice".into(),
            "--private-key".into(),
            key.display().to_string(),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("私钥无法被当前 SSH 栈加载"));
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
    total_timeout: Option<u64>,
    local_path: Option<String>,
    remote_path: Option<String>,
    cache_ttl_ms: Option<u64>,
    pty: Option<bool>,
    target: Option<String>,
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
    session: client::Handle<RusshClient>,
    last_used_at: Instant,
    ttl_ms: u64,
}

/// 缓存的 JumpServer 目标主机 PTY 通道。
/// 已经完成"进网关 → 进菜单 → 选 target → shell prompt"流程，可直接接收 marker-wrapped 命令。
/// 同一 entry 仅支持串行执行（daemon 是单线程事件循环，本身就串行处理请求）。
struct JumpPoolEntry {
    session: client::Handle<RusshClient>,
    channel: Channel<client::Msg>,
    buffer: String,
    last_used_at: Instant,
    ttl_ms: u64,
}

struct DaemonState {
    runtime: tokio::runtime::Runtime,
    config_snapshot: ConfigSnapshot,
    configs: Vec<Connection>,
    connections: HashMap<String, PoolEntry>,
    jump_connections: HashMap<String, JumpPoolEntry>,
}

impl DaemonState {
    fn new(config_path: &Path) -> AppResult<Self> {
        Ok(Self {
            runtime: tokio::runtime::Runtime::new()
                .map_err(|error| AppError::new(format!("创建 tokio runtime 失败: {}", error)))?,
            config_snapshot: ConfigSnapshot::read(config_path)?,
            configs: load_config(config_path)?,
            connections: HashMap::new(),
            jump_connections: HashMap::new(),
        })
    }

    fn run_with_timeout<T, F>(&self, timeout_ms: u64, future: F) -> AppResult<T>
    where
        F: std::future::Future<Output = AppResult<T>>,
    {
        block_with_timeout(&self.runtime, timeout_ms, future)
    }
}

fn cache_ttl(global: &GlobalArgs) -> u64 {
    global.cache_ttl_ms.unwrap_or(DEFAULT_CACHE_TTL_MS)
}

fn request_stop_daemon(config_path: &Path) -> AppResult<()> {
    let config_path = path_absolute(config_path)?;
    let socket_path = get_socket_path(&config_path)?;
    let mut stream = connect_socket(&socket_path, DAEMON_REQUEST_TIMEOUT_MS)?;
    let request = serde_json::json!({
        "operation": "stop",
        "configPath": config_path,
        "cwd": env::current_dir()?,
        "connectionName": "__daemon__"
    });
    let line = format!("{}\n", serde_json::to_string(&request)?);
    stream.write_all(line.as_bytes())?;
    stream.flush()?;
    validate_daemon_response(read_daemon_response(&mut stream)?)?;
    Ok(())
}

fn request_daemon_execute(parsed: &ExecuteArgs, command: &str) -> AppResult<String> {
    let config_path = path_absolute(&parsed.global.config_path)?;
    let request = serde_json::json!({
        "operation": "execute",
        "configPath": config_path,
        "cwd": env::current_dir()?,
        "connectionName": parsed.connection_name,
        "command": command,
        "directory": parsed.directory,
        "timeout": parsed.idle_timeout_ms,
        "totalTimeout": parsed.total_timeout_ms,
        "cacheTtlMs": cache_ttl(&parsed.global),
        "pty": parsed.pty,
    });
    let response = request_daemon(&config_path, &request)?;
    Ok(response.stdout.unwrap_or_default())
}

fn request_daemon_jump_execute(parsed: &JumpExecArgs, command: &str) -> AppResult<String> {
    let config_path = path_absolute(&parsed.global.config_path)?;
    let request = serde_json::json!({
        "operation": "jumpExecute",
        "configPath": config_path,
        "cwd": env::current_dir()?,
        "connectionName": parsed.connection_name,
        "command": command,
        "target": parsed.target,
        "timeout": parsed.idle_timeout_ms,
        "totalTimeout": parsed.total_timeout_ms,
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
    let response = read_daemon_response(&mut stream);
    if matches_empty_daemon_response(&response) {
        unlink_socket_path(&socket_path)?;
        ensure_daemon(&socket_path, config_path)?;
        let mut retry_stream = connect_socket(&socket_path, DAEMON_REQUEST_TIMEOUT_MS)?;
        retry_stream.write_all(line.as_bytes())?;
        retry_stream.flush()?;
        let retry_response = read_daemon_response(&mut retry_stream)?;
        return validate_daemon_response(retry_response);
    }
    validate_daemon_response(response?)
}

fn validate_daemon_response(response: DaemonResponse) -> AppResult<DaemonResponse> {
    if !response.ok {
        return Err(AppError::new(
            response
                .message
                .unwrap_or_else(|| "SSH 缓存进程执行失败".to_string()),
        ));
    }
    Ok(response)
}

fn matches_empty_daemon_response(response: &AppResult<DaemonResponse>) -> bool {
    matches!(response, Err(error) if error.to_string() == "SSH 缓存进程提前关闭连接")
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
    let log_path = daemon_log_path(config_path)?;
    spawn_daemon(socket_path, config_path, &log_path)?;
    wait_for_daemon(socket_path, &log_path)
}

fn spawn_daemon(socket_path: &Path, config_path: &Path, log_path: &Path) -> AppResult<()> {
    let exe = env::current_exe()?;
    let _ = fs::remove_file(log_path);
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|error| {
            AppError::new(format!(
                "打开 SSH 缓存进程日志失败: {}，{}",
                log_path.display(),
                error
            ))
        })?;
    let mut command = Command::new(exe);
    command
        .arg("__daemon")
        .arg("--socket")
        .arg(socket_path)
        .arg("--config")
        .arg(config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .current_dir(project_root()?);
    command.spawn()?;
    Ok(())
}

fn wait_for_daemon(socket_path: &Path, log_path: &Path) -> AppResult<()> {
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
    let mut message = format!(
        "启动 SSH 缓存进程失败: {}，日志: {}",
        last_error.unwrap_or_else(|| "未知错误".to_string()),
        log_path.display()
    );
    if let Some(stderr) = read_daemon_log_tail(log_path) {
        message.push_str(&format!("，stderr: {}", stderr));
    }
    Err(AppError::new(message))
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

fn daemon_log_path(config_path: &Path) -> AppResult<PathBuf> {
    let resolved = path_absolute(config_path)?;
    let parent = resolved
        .parent()
        .ok_or_else(|| AppError::new("配置文件路径缺少父目录，无法创建 SSH 缓存进程日志"))?;
    let mut hasher = Sha256::new();
    hasher.update(resolved.to_string_lossy().as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    Ok(parent.join(format!("agentsshcli-daemon-{}.log", &digest[..12])))
}

fn read_daemon_log_tail(log_path: &Path) -> Option<String> {
    let raw = fs::read_to_string(log_path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    const MAX_LOG_CHARS: usize = 1200;
    let tail: String = trimmed
        .chars()
        .rev()
        .take(MAX_LOG_CHARS)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Some(tail)
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

fn read_daemon_response<S: Read>(stream: &mut S) -> AppResult<DaemonResponse> {
    let mut header = [0_u8; DAEMON_RESPONSE_LENGTH_BYTES];
    match stream.read_exact(&mut header) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
            return Err(AppError::new("SSH 缓存进程提前关闭连接"));
        }
        Err(error) => return Err(error.into()),
    }
    let length_text = std::str::from_utf8(&header)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应长度非法: {}", error)))?;
    let length = usize::from_str_radix(length_text, 16)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应长度非法: {}", error)))?;
    let mut body = vec![0_u8; length];
    stream
        .read_exact(&mut body)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应未读完整: {}", error)))?;
    serde_json::from_slice(&body)
        .map_err(|error| AppError::new(format!("SSH 缓存进程响应非法: {}", error)))
}

fn write_daemon_response<S: Write>(stream: &mut S, response: &DaemonResponse) -> AppResult<()> {
    let body = serde_json::to_vec(response)?;
    if body.len() > u32::MAX as usize {
        return Err(AppError::new("SSH 缓存进程响应过大"));
    }
    let header = format!("{:08x}", body.len());
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(unix)]
fn run_daemon(argv: Vec<String>) -> AppResult<()> {
    let (socket_path, config_path) = parse_daemon_args(argv)?;
    unlink_socket_path(&socket_path)?;
    let listener = UnixListener::bind(&socket_path)?;
    fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600))?;
    let bound_config_path = path_absolute(&config_path)?;
    let mut state = DaemonState::new(&bound_config_path)?;
    let mut last_activity_at = Instant::now();
    loop {
        let wait_ms = next_daemon_wait_ms(&state, last_activity_at);
        listener.set_nonblocking(true)?;
        match listener.accept() {
            Ok((mut stream, _)) => {
                last_activity_at = Instant::now();
                let response =
                    match handle_daemon_stream(&mut stream, &bound_config_path, &mut state) {
                        Ok(response) => response,
                        Err(error) => DaemonResponse {
                            ok: false,
                            message: Some(error.to_string()),
                            stdout: None,
                        },
                    };
                let should_stop = response.stdout.as_deref() == Some("stop");
                write_daemon_response(&mut stream, &response)?;
                if should_stop {
                    break;
                }
                expire_connections(&mut state.connections);
                expire_jump_connections(&mut state.jump_connections);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(wait_ms.min(100)));
                expire_connections(&mut state.connections);
                expire_jump_connections(&mut state.jump_connections);
                if state.connections.is_empty()
                    && state.jump_connections.is_empty()
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
    let mut state = DaemonState::new(&bound_config_path)?;
    let mut last_activity_at = Instant::now();
    loop {
        match listener.accept() {
            Ok(mut stream) => {
                last_activity_at = Instant::now();
                let response =
                    match handle_daemon_stream(&mut stream, &bound_config_path, &mut state) {
                        Ok(response) => response,
                        Err(error) => DaemonResponse {
                            ok: false,
                            message: Some(error.to_string()),
                            stdout: None,
                        },
                    };
                let should_stop = response.stdout.as_deref() == Some("stop");
                write_daemon_response(&mut stream, &response)?;
                if should_stop {
                    break;
                }
                expire_connections(&mut state.connections);
                expire_jump_connections(&mut state.jump_connections);
            }
            Err(error) => return Err(AppError::new(error.to_string())),
        }
        if state.connections.is_empty()
            && state.jump_connections.is_empty()
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

fn next_daemon_wait_ms(state: &DaemonState, last_activity_at: Instant) -> u64 {
    if state.connections.is_empty() && state.jump_connections.is_empty() {
        return DEFAULT_CACHE_TTL_MS
            .saturating_sub(last_activity_at.elapsed().as_millis() as u64)
            .max(100);
    }
    let exec_min = state
        .connections
        .values()
        .map(|entry| {
            entry
                .ttl_ms
                .saturating_sub(entry.last_used_at.elapsed().as_millis() as u64)
                .max(100)
        })
        .min();
    let jump_min = state
        .jump_connections
        .values()
        .map(|entry| {
            entry
                .ttl_ms
                .saturating_sub(entry.last_used_at.elapsed().as_millis() as u64)
                .max(100)
        })
        .min();
    match (exec_min, jump_min) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => DEFAULT_CACHE_TTL_MS,
    }
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

fn expire_jump_connections(connections: &mut HashMap<String, JumpPoolEntry>) {
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
    state: &mut DaemonState,
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
    if raw_value.get("operation").and_then(|item| item.as_str()) == Some("stop") {
        return Ok(DaemonResponse {
            ok: true,
            message: None,
            stdout: Some("stop".to_string()),
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
    reload_daemon_config_if_changed(bound_config_path, state)?;
    resolve_password_ref_for_connection(
        bound_config_path,
        &mut state.configs,
        &request.connection_name,
    )?;
    resolve_jump_password_refs(
        bound_config_path,
        &mut state.configs,
        &request.connection_name,
    )?;
    validate_jump_hosts(&state.configs)?;
    let connection = find_connection(&state.configs, &request.connection_name)?.clone();
    if request.operation == "execute" {
        let command = request
            .command
            .as_deref()
            .ok_or_else(|| AppError::new("daemon execute 缺少 command"))?;
        validate_command(&connection, command)?;
    }

    // jumpExecute 使用独立缓存（jump_connections），key 包含 target 与 jumpServer 配置；
    // 不复用 PoolEntry / execute_remote_command_with_session_async，命中后直接在已 ready 的
    // shell channel 上发 marker 命令，失败时丢弃 entry 并重连重试一次。
    if request.operation == "jumpExecute" {
        return handle_jump_execute(&request, &connection, ttl_ms, bound_config_path, state);
    }

    let key = build_connection_key(bound_config_path, &state.configs, &connection);
    let idle_timeout_ms = request.timeout.unwrap_or(30000);
    let total_timeout_ms = request.total_timeout;
    if !state.connections.contains_key(&key) {
        let session = state.run_with_timeout(
            idle_timeout_ms,
            connect_russh(&state.configs, &connection),
        )?;
        state.connections.insert(
            key.clone(),
            PoolEntry {
                session,
                last_used_at: Instant::now(),
                ttl_ms,
            },
        );
    }
    let mut entry = state
        .connections
        .remove(&key)
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
            let pty = resolve_pty(&connection, request.pty);
            let timeouts = CommandTimeouts {
                idle_timeout_ms,
                total_timeout_ms,
            };
            let stdout_result = state.run_with_timeout(
                command_runtime_timeout_ms(timeouts.idle_timeout_ms, timeouts.total_timeout_ms),
                execute_remote_command_with_session_async(
                    &entry.session,
                    &connection,
                    &remote_command,
                    pty,
                    timeouts,
                ),
            );
            let stdout = match stdout_result {
                Ok(stdout) => stdout,
                Err(error) => {
                    let _ = state.run_with_timeout(timeouts.idle_timeout_ms.min(5000).max(1000), async {
                        entry
                            .session
                            .disconnect(Disconnect::ByApplication, "", "English")
                            .await
                            .map_err(|error| {
                                AppError::new(format!("断开失效 SSH 缓存连接失败: {}", error))
                            })
                    });
                    let session = state.run_with_timeout(
                        timeouts.idle_timeout_ms,
                        connect_russh(&state.configs, &connection),
                    )?;
                    let stdout = state
                        .run_with_timeout(
                            command_runtime_timeout_ms(timeouts.idle_timeout_ms, timeouts.total_timeout_ms),
                            execute_remote_command_with_session_async(
                                &session,
                                &connection,
                                &remote_command,
                                pty,
                                timeouts,
                            ),
                        )
                        .map_err(|retry_error| {
                            AppError::new(format!("{}；已重连重试仍失败: {}", error, retry_error))
                        })?;
                    entry.session = session;
                    stdout
                }
            };
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
            let local_path = validate_local_path(&state.configs, &local, &request.cwd)?;
            if let Err(error) = state.runtime.block_on(upload_file_with_session_async(
                &entry.session,
                &connection,
                &local_path,
                &remote,
            )) {
                return Err(error);
            }
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
            let local_path = validate_local_path(&state.configs, &local, &request.cwd)?;
            if let Some(parent) = local_path.parent() {
                fs::create_dir_all(parent)?;
            }
            if let Err(error) = state.run_with_timeout(
                request.timeout.unwrap_or(30000),
                download_file_with_session_async(&entry.session, &connection, &remote, &local_path),
            ) {
                return Err(error);
            }
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
    state.connections.insert(key, entry);
    Ok(result)
}


fn build_jump_connection_key(
    config_path: &Path,
    connection: &Connection,
    jump: &JumpServerConfig,
    target: &str,
) -> String {
    let auth = if let Some(private_key) = &connection.private_key {
        format!(
            "privateKey:{}:{}",
            private_key,
            sensitive_hash(connection.passphrase.as_deref().unwrap_or(""))
        )
    } else {
        format!(
            "password:{}",
            sensitive_hash(connection.password.as_deref().unwrap_or(""))
        )
    };
    let jump_repr = format!(
        "{}|{}|{}|{}|{}|{}|{}",
        jump.enabled,
        jump.prompt_regex,
        jump.shell_prompt_regex,
        jump.search_prefix,
        jump.char_delay_ms,
        jump.enter_strategy,
        target
    );
    let raw = format!(
        "{}|{}|{}|{}|{}|{}|{:?}|{}|{}",
        path_absolute(config_path)
            .unwrap_or_else(|_| canonical_or_absolute(config_path.to_path_buf()))
            .display(),
        connection.name,
        connection.host,
        connection.port,
        connection.username,
        target,
        connection.socks_proxy,
        auth,
        jump_repr,
    );
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    format!("jump:{:x}", hasher.finalize())
}

fn handle_jump_execute(
    request: &DaemonRequest,
    connection: &Connection,
    ttl_ms: u64,
    bound_config_path: &Path,
    state: &mut DaemonState,
) -> AppResult<DaemonResponse> {
    let command = request
        .command
        .as_deref()
        .ok_or_else(|| AppError::new("daemon jumpExecute 缺少 command"))?;
    let target = request
        .target
        .as_deref()
        .ok_or_else(|| AppError::new("daemon jumpExecute 缺少 target"))?;
    if target.trim().is_empty() {
        return Err(AppError::new("daemon jumpExecute target 不能为空"));
    }
    validate_command(connection, command)?;
    let jump = connection
        .jump_server
        .as_ref()
        .ok_or_else(|| {
            AppError::new(format!(
                "连接 {} 未配置 jumpServer，无法使用 jumpExecute",
                connection.name
            ))
        })?
        .clone();
    if !jump.enabled {
        return Err(AppError::new(format!(
            "连接 {} 的 jumpServer.enabled 为 false，拒绝使用 jumpExecute",
            connection.name
        )));
    }

    let key = build_jump_connection_key(bound_config_path, connection, &jump, target);
    let idle_timeout_ms = request.timeout.unwrap_or(30000);
    let total_timeout_ms = request.total_timeout;
    let total_deadline = total_deadline_from_ms(total_timeout_ms);
    let command_runtime_timeout_ms = command_runtime_timeout_ms(idle_timeout_ms, total_timeout_ms);

    // Pop existing entry if any, attempt to run command on it.
    let existing = state.jump_connections.remove(&key);
    let cached_hit = existing.is_some();
    let mut entry = match existing {
        Some(entry) => entry,
        None => build_jump_entry(connection, &jump, target, ttl_ms, idle_timeout_ms, total_timeout_ms, state)?,
    };

    // Run command on the (possibly cached) channel.
    // 仅在 channel/marker 层面的错误才丢弃缓存；命令业务非零退出 (exit_code != 0) 不应让缓存失效。
    let run_result = state.run_with_timeout(
        command_runtime_timeout_ms,
        run_jump_command(
            &mut entry.channel,
            &mut entry.buffer,
            command,
            idle_timeout_ms,
            total_deadline,
        ),
    );

    let (output, exit_code) = match run_result {
        Ok(pair) => pair,
        Err(error) => {
            // Drop broken entry; reconnect once and retry (only if cached, otherwise propagate).
            if !cached_hit {
                return Err(error);
            }
            let _ = state.run_with_timeout(idle_timeout_ms.min(5000).max(1000), async {
                entry
                    .session
                    .disconnect(Disconnect::ByApplication, "", "English")
                    .await
                    .map_err(|err| AppError::new(format!("断开失效跳板缓存失败: {}", err)))
            });
            let mut fresh =
                build_jump_entry(connection, &jump, target, ttl_ms, idle_timeout_ms, total_timeout_ms, state)?;
            let retry = state
                .run_with_timeout(
                    command_runtime_timeout_ms,
                    run_jump_command(
                        &mut fresh.channel,
                        &mut fresh.buffer,
                        command,
                        idle_timeout_ms,
                        total_deadline,
                    ),
                )
                .map_err(|retry_err| {
                    AppError::new(format!("{}；已重连重试仍失败: {}", error, retry_err))
                })?;
            entry = fresh;
            retry
        }
    };

    entry.last_used_at = Instant::now();
    entry.ttl_ms = ttl_ms;
    state.jump_connections.insert(key, entry);

    if exit_code != 0 {
        return Err(AppError::new(format_exit_failure(&output, exit_code)));
    }

    Ok(DaemonResponse {
        ok: true,
        message: None,
        stdout: Some(output),
    })
}

fn build_jump_entry(
    connection: &Connection,
    jump: &JumpServerConfig,
    target: &str,
    ttl_ms: u64,
    idle_timeout_ms: u64,
    total_timeout_ms: Option<u64>,
    state: &DaemonState,
) -> AppResult<JumpPoolEntry> {
    let runtime_timeout_ms = jump_runtime_timeout_ms(total_timeout_ms);
    let total_deadline = total_deadline_from_ms(total_timeout_ms);
    let session = state.run_with_timeout(runtime_timeout_ms, connect_russh(&state.configs, connection))?;
    let (channel, buffer) =
        state.run_with_timeout(
            phase_timeout_ms(idle_timeout_ms, total_deadline),
            setup_jump_channel(
                &session,
                connection,
                jump,
                target,
                idle_timeout_ms,
                total_deadline,
            ),
        )?;
    Ok(JumpPoolEntry {
        session,
        channel,
        buffer,
        last_used_at: Instant::now(),
        ttl_ms,
    })
}

fn reload_daemon_config_if_changed(config_path: &Path, state: &mut DaemonState) -> AppResult<()> {
    if state.config_snapshot.metadata_matches(config_path)? {
        return Ok(());
    }
    let current_snapshot = ConfigSnapshot::read(config_path)?;
    if current_snapshot.hash == state.config_snapshot.hash {
        state.config_snapshot = current_snapshot;
        return Ok(());
    }
    let configs = load_config(config_path)?;
    state.config_snapshot = current_snapshot;
    state.configs = configs;
    state.connections.clear();
    state.jump_connections.clear();
    Ok(())
}

fn build_connection_key(
    config_path: &Path,
    configs: &[Connection],
    connection: &Connection,
) -> String {
    let auth = if let Some(private_key) = &connection.private_key {
        format!(
            "privateKey:{}:{}",
            private_key,
            sensitive_hash(connection.passphrase.as_deref().unwrap_or(""))
        )
    } else {
        format!(
            "password:{}",
            sensitive_hash(connection.password.as_deref().unwrap_or(""))
        )
    };
    let jump = connection
        .jump_host
        .as_deref()
        .and_then(|name| find_connection(configs, name).ok())
        .map(connection_fingerprint)
        .unwrap_or_else(|| "no-jump".to_string());
    let raw = format!(
        "{}|{}|{}|{}|{}|{:?}|{:?}|{}|{}",
        path_absolute(config_path)
            .unwrap_or_else(|_| canonical_or_absolute(config_path.to_path_buf()))
            .display(),
        connection.name,
        connection.host,
        connection.port,
        connection.username,
        connection.socks_proxy,
        connection.jump_host,
        jump,
        auth
    );
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn connection_fingerprint(connection: &Connection) -> String {
    let auth = if let Some(private_key) = &connection.private_key {
        format!(
            "privateKey:{}:{}",
            private_key,
            sensitive_hash(connection.passphrase.as_deref().unwrap_or(""))
        )
    } else {
        format!(
            "password:{}",
            sensitive_hash(connection.password.as_deref().unwrap_or(""))
        )
    };
    format!(
        "{}|{}|{}|{}|{:?}|{}",
        connection.name,
        connection.host,
        connection.port,
        connection.username,
        connection.socks_proxy,
        auth
    )
}

fn sensitive_hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}
