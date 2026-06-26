<div align="center">

# agent-ssh-cli

A CLI-based SSH agent tool that maps ssh-mcp-server capabilities into remote operations callable by agents.

Remote exec · File upload · File download · Connection config · Command whitelist · Command blacklist · Agent Skill integration

<p>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli"><img src="https://img.shields.io/badge/CLI-agentsshcli-2ea44f" alt="CLI agentsshcli"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/blob/main/LICENSE"><img src="https://img.shields.io/badge/License-MIT-green" alt="License MIT"></a>
  <a href="https://nodejs.org/"><img src="https://img.shields.io/badge/Node.js-%3E%3D18-339933?logo=node.js&logoColor=white" alt="Node.js >=18"></a>
  <a href="https://www.npmjs.com/"><img src="https://img.shields.io/badge/npm-%3E%3D8-CB3837?logo=npm&logoColor=white" alt="npm >=8"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli"><img src="https://img.shields.io/badge/sys-win%2Fmac%2Flinux-0078D6" alt="sys win/mac/linux"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/releases"><img src="https://img.shields.io/badge/release-v0.1.7-blue" alt="release v0.1.7"></a>
  <a href="https://github.com/2Red1Blue/agent-ssh-cli/pulls"><img src="https://img.shields.io/badge/PRs-welcome-brightgreen" alt="PRs welcome"></a>
</p>

[AI One-Click Installation](#ai-one-click-installation) · [Manual Installation](#manual-installation) · [Configuration](#configuration) · [Uninstall and Cleanup](#uninstall-and-cleanup) · [License](#license) · [Friendly Links](#friendly-links)

[中文](README.md) | English

</div>

## Introduction
This project references the SSH operation design from [classfang/ssh-mcp-server](https://github.com/classfang/ssh-mcp-server) and rewrites it as an independent CLI. Thanks to the original project for the ideas and capability foundation.

#### What it can do:
- Free your hands and automate server operations
- Deploy code and update Docker deployments
- Configure nginx and certificates
- Do anything SSH can do

#### Its capabilities:
- List SSH server connections from local configuration
- Execute commands on a specified remote server
- Upload local files to a remote server with temporary files, resume, and retry
- Download files from a remote server to local
- Restrict executable commands through command allowlists and blocklists
- Restrict upload and download access scopes through a local path allowlist

## Upload Reliability

Uploads are written to `<remotePath>.part` first, with resume metadata in `<remotePath>.part.meta`. After the temporary file size is verified, it is renamed to the final target path. If an upload is interrupted, running the same upload again resumes from the existing `.part` size when the local file metadata still matches.

For `--no-cache` uploads, use `Ctrl+C` to stop the current CLI process. In daemon mode, `agentsshcli stop-daemon` stops the connection-pool process, but it affects other tasks in the same daemon and is not a precise per-upload cancel operation.

## AI One-Click Installation

```text
Please read https://github.com/2Red1Blue/agent-ssh-cli/blob/main/AI_INSTALL.md, follow the instructions to install the CLI, and add `SKILL.md`.
```

This prompt still works after the new installer changes. Once the AI reads `AI_INSTALL.md`, it should continue through the full first-run flow:

- detect multiple npm globals and install into each unique global prefix
- choose target clients interactively: `codex`, `claude`, `opencode`, `hermes`, or `custom`
- choose a primary client
- choose whether secondary clients reuse the primary skill/env-map via symlink or get separate copies
- install `agent-ssh-cli` and `log-analyze`
- initialize `~/.agent-ssh-cli/config.json` plus the primary `env-map.md` template
- tell the user to restart the client and continue the interactive config/mapping setup until `log-analyze` appears

## Manual Installation
### Requirements

- Node.js `>= 18`
- npm `>= 8`
- Local network access to the target SSH server
- If private key authentication is used, the private key file must be readable by the current user
- Prebuilt platform packages support macOS arm64/x64, Linux x64/arm64, and Windows x64

### Installation Steps

1. Install globally:

```bash
for npm_bin in $(which -a npm 2>/dev/null | awk '!seen[$0]++'); do
  prefix="$("$npm_bin" prefix -g 2>/dev/null)" || continue
  case " ${SEEN_PREFIXES:-} " in
    *" $prefix "*) continue ;;
  esac
  SEEN_PREFIXES="${SEEN_PREFIXES:-} $prefix"
  "$npm_bin" install -g @2red1blue/agentsshcli || exit 1
done
agentsshcli --help
```

If the machine has multiple Node/npm installations (for example Hermes, a system Node install, and Homebrew Node), prefer the command above so the package is installed into every unique global npm prefix instead of only one tool-specific global directory.

To explicitly enter the interactive client-selection flow, run:

```bash
agentsshcli install-ai --interactive
```

2. Import SKILL.md:

Manual SKILL.md copying is no longer the recommended default path. Prefer:

```bash
agentsshcli install-ai --interactive
```

Use manual or custom-path installation only when:

- the target client is not one of the built-in clients
- the user explicitly wants a custom skills root
- the skill should be installed into a project-local directory instead of a global one

For an unknown client, use:

```bash
agentsshcli install-ai --clients custom --client-root custom=/absolute/path/to/skills
```

## Configuration

Initialize the configuration. The format parameters are compatible with ssh-mcp-server:

```bash
mkdir -p ~/.agent-ssh-cli
```

Edit `~/.agent-ssh-cli/config.json` and fill in the real connection information. The default configuration file path can also be overridden with an environment variable:

You can change the configuration location with the following environment variable:
```bash
AGENT_SSH_CONFIG=/path/to/config.json
```

The configuration file is an array, and each item represents one server:

- `name`: Connection name, must be unique
- `host`: SSH host address
- `username`: SSH username
- `password` / `passwordRef` / `privateKey`: Authentication method; keep exactly one authentication type among password, password reference, and private key
- `port`: SSH port, defaults to `22`
- `passphrase`: Private key passphrase, only used with `privateKey`
- `socksProxy`: SOCKS5 proxy address, for example `socks5://127.0.0.1:1080`; the scheme can also be omitted as `127.0.0.1:1080`
- `jumpHost`: Jump host connection name, must reference another `name` in the config file
- `pty`: Whether to allocate a pseudo-terminal, defaults to `false`; it can also be enabled per command with `exec --pty`
- `allowedLocalPaths`: Extra local paths allowed for upload or download writes
- `commandWhitelist`: Command whitelist regular expression array
- `commandBlacklist`: Command blacklist regular expression array

`commandWhitelist` and `commandBlacklist` use JavaScript `RegExp` syntax, not POSIX regular expressions. Write whitespace as `\\s`; do not use `[:space:]`.

See the full example in [example.config.json](example.config.json). Store real connection information in `~/.agent-ssh-cli/config.json`.

To reduce password leakage from configuration files, password authentication is passively encrypted the first time that server is used: after you write a plaintext `password`, the next `exec`, `upload`, or `download` for that server encrypts the password into `secrets.json` under the config directory, creates a local `secret.key`, then writes the config back with an empty `password` and a `passwordRef`. Later runs decrypt through `passwordRef`. To change the password, replace the empty `password` with the new plaintext password; the next connection overwrites the old encrypted value.

Reference configuration

```json
[
  {
    "name": "password-server",
    "host": "192.0.2.10",
    "port": 22,
    "username": "root",
    "password": "",
    "passwordRef": "agentsshcli:password-server",
    "jumpHost": "jump-server",
    "commandBlacklist": [
      "(^|[;&|()\\s])rm(\\s|$)",
      "(^|[;&|()\\s])shutdown(\\s|$)",
      "(^|[;&|()\\s])reboot(\\s|$)"
    ]
  },
  {
    "name": "jump-server",
    "host": "198.51.100.20",
    "port": 22,
    "username": "ubuntu",
    "privateKey": "/path/to/jump_key",
    "passphrase": "******",
    "socksProxy": "socks5://127.0.0.1:1080"
  },
  {
    "name": "key-server",
    "host": "198.51.100.10",
    "port": 22,
    "username": "deploy",
    "privateKey": "/path/to/id_rsa",
    "passphrase": "******",
    "pty": false,
    "allowedLocalPaths": [
      "./tmp",
      "./dist"
    ],
    "commandWhitelist": [
      "^pwd$",
      "^ls(\\s|$)",
      "^cat\\s+/var/log/app\\.log$"
    ],
    "commandBlacklist": [
      "(^|[;&|()\\s])rm(\\s|$)",
      "(^|[;&|()\\s])shutdown(\\s|$)",
      "(^|[;&|()\\s])reboot(\\s|$)"
    ]
  }
]
```

Test command

```bash
agentsshcli list
agentsshcli exec --no-cache password-server "pwd"
agentsshcli exec --pty password-server "tty"
agentsshcli exec password-server --command-file ./script.sh --timeout 60000
```

Installation is complete.

## Uninstall and Cleanup

Update to the latest version:

```bash
npm install -g @2red1blue/agentsshcli@latest
```

Remove:

```bash
npm uninstall -g @2red1blue/agentsshcli
npm cache clean --force
# Delete the configuration file
rm -rf ~/.agent-ssh-cli
```

## License

[MIT](LICENSE)

## Friendly Links

- [LINUX DO - A New Ideal Community](https://linux.do/)
