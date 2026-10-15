# BoardDo

Visual workflow automation: **SBDO** (canvas) + **SmartDo** (execution engine).

## Phase 1

Vertical slice:

```
SBDO → create → save → run → SmartDo execute → WebSocket events → debugger
```

### Structure

```
.
├── Cargo.toml          # Rust workspace
├── crates/
│   ├── backend/        # SmartDo (API + engine + SQLite)
│   ├── shared/         # Domain contract (Workflow, Execution, …)
│   └── boarddo-telegram/
│       ├── …           # BoardDo product: SQLite, audit, trigger match
│       └── gram_api/   # git submodule → igornet0/gram_api, branch `tglib`
├── vendor/
│   └── RustBrowser/    # git submodule → igornet0/RustBrowser (optional, `make browser`)
└── SBDO/               # React canvas editor
```

Clone with submodules:

```bash
git clone --recurse-submodules <boarddo-url>
# or after a normal clone:
git submodule update --init --recursive
```

### Quick start (Makefile)

```bash
make setup   # submodules + npm install + cargo fetch
make dev     # SmartDo :8080 + SBDO :5173 together (Ctrl+C stops both)
make help    # all targets: backend, frontend, dev-tdlib, build, test, lint, ci…
```

Override ports/paths: `make dev LISTEN_ADDR=127.0.0.1:9090 FRONTEND_PORT=5180` (the Vite proxy follows `LISTEN_ADDR`).

### Backend (SmartDo)

```bash
cargo run -p smartdo
# listens on http://127.0.0.1:8080
# SQLite: boarddo.db
```

Env (optional):

- `DATABASE_URL=sqlite:boarddo.db?mode=rwc`
- `LISTEN_ADDR=127.0.0.1:8080`
- `BOARDDO_DATA_DIR=data` (per-account TDLib/mock session directories)
- `BOARDDO_TELEGRAM_CONFIG` (optional path to `telegram.toml`)
- `TELEGRAM_API_ID` / `TELEGRAM_API_HASH` — **developer override only** (never logged)
- `BRAVE_SEARCH_API_KEY` — Brave Search API for `web.search` (direct publisher links, news index)
- `SEARXNG_URL` — self-hosted SearXNG for `web.search` (`json` must be in `search.formats`)
- Headless browser for JavaScript-rendered pages (`render: auto | browser` in `web.open` /
  `web.search`) — [RustBrowser](https://github.com/igornet0/RustBrowser) (Servo), bundled as the
  `vendor/RustBrowser` submodule:

  ```bash
  make browser          # once: builds rust-browser into ~/.cache/boarddo/rust-browser (long, Servo)
  make dev              # BoardDo starts the browser on the first render, stops it when idle
  make browser-status   # built? running? (GET /api/browser/status)
  ```

  BoardDo runs it on `127.0.0.1` with a random port and token, one process for all boards and
  agents; it exits with BoardDo (stdin pipe) and after `BROWSER_IDLE_SECS` (600) without renders.
  Settings: `BROWSER_BIN` (binary path), `BROWSER_MAX_TABS` (4), `BROWSER_LOG` (`warn`),
  `BROWSER_AUTOMATION=off` (disable), or an external server instead:
  `BROWSER_AUTOMATION_URL` / `BROWSER_AUTOMATION_TOKEN`. The build lives outside the repo because
  Servo's C build scripts reject paths with spaces (`BROWSER_TARGET_DIR` to change it).
  macOS / Linux only (Unix-socket IPC).

  `auto` keeps plain HTTP and switches to the browser only for JavaScript shells
  (or when browser options such as `wait_for`, `wait_js`, `network_idle_ms`, `script`,
  `screenshot`, `capture_network` are set). 403/429 responses are not retried in the browser.

Web research pipeline (`web.open`, `web.extract`):

- structured data first: JSON-LD, OpenGraph/meta, microdata, tables, embedded app state
  (`__NEXT_DATA__`, `window.__INITIAL_STATE__`, …) — often enough without a browser;
- browser network capture (`capture_network`): fetch/XHR log + JSON responses →
  `api_candidates` (optionally `fetch_api` GETs endpoints the page hook missed);
- every page has `provenance` (url, method, fetched_at, `sha256` content hash), every
  extracted value has `evidence` (source url, method, selector / JSON path / quote);
- access problems are reported, not bypassed: `blocked` (401/403/451), `rate_limited`
  (429, `Retry-After` honoured up to 10 s, 2 retries), `challenge` (bot-check pages),
  `not_found`, `unavailable`;
- `web.extract` asks the model for fields with a source id + JSON path or exact quote and
  re-checks each value; unverifiable values are listed in `unverified` (dropped with `strict`).

Default Telegram user stack uses `MockTelegramClient` (no `tdjson`). CI and DoD tests stay on that mock.

Live user accounts need native TDLib (`tdjson`) plus BoardDo’s own Telegram application (`api_id` / `api_hash` from [my.telegram.org](https://my.telegram.org)). Do not reuse Telegram Desktop or another client’s credentials.

Users never set env vars. Startup loads application credentials in this order (later wins):

1. Compile-time embed (`BOARDDO_EMBEDDED_TELEGRAM_API_ID` / `_HASH`)
2. `config/telegram.toml` or `{BOARDDO_DATA_DIR}/telegram.toml`
3. Encrypted secret store
4. macOS Keychain
5. `TELEGRAM_API_ID` / `TELEGRAM_API_HASH` (developer override)

```bash
# macOS ARM64 — Homebrew stable tdlib 1.8.0 is too old (UPDATE_APP_TO_LOGIN).
# Use HEAD / a current TDLib build:
brew uninstall tdlib
brew install --HEAD tdlib
pkg-config --libs tdjson

cp config/telegram.toml.example config/telegram.toml
# fill api_id / api_hash for the BoardDo Telegram application, then:
cargo run -p smartdo --features tdlib
```

After the first load, `api_hash` is copied out of the toml into secure storage; remove it from the file. `cargo run -p smartdo --features tdlib` links `libtdjson` and starts a real TDLib client per user account. Missing application credentials fail authorization with a typed error, not a stub `Unavailable`.

Override the library search path if TDLib is not in Homebrew/pkg-config:

```bash
export TDLIB_DIR=/path/to/tdlib/prefix
export TDLIB_LIB_DIR=$TDLIB_DIR/lib
```

Native smoke (requires `tdjson` on the machine):

```bash
cargo test -p tglib-tdlib --features native
```

### Frontend (SBDO)

```bash
cd SBDO
npm install
npm run dev
# http://127.0.0.1:5173  (proxies /api and /ws → :8080)
```

### Checks

```bash
cargo test --workspace
cargo check --workspace
cargo check -p smartdo --features tdlib   # requires tdjson
```

### Phase 1–2 nodes

| type_id | Role |
|---------|------|
| `trigger.manual` | Manual start |
| `trigger.webhook` | `POST /api/webhooks/:workflow_id` |
| `trigger.schedule` | Cron / interval / daily (armed when workflow `active`) |
| `data.set` | Set variable |
| `data.transform` | Map fields / templates / expressions |
| `logic.condition` | Branch true/false (`expression` or compare) |
| `logic.delay` | Async sleep |
| `http.request` | Outbound HTTP (GET/POST/PUT/PATCH/DELETE) |
| `telegram.send_message` | Telegram Bot sendMessage |
| `telegram.send_photo` | Telegram Bot sendPhoto |
| `telegram.send_document` | Telegram Bot sendDocument |
| `trigger.telegram.user.message_received` | User-account inbound message (TDLib / mock) |
| `telegram.user.send_message` | User-account send (explicit `account_id`) |
| `telegram.user.forward_message` | User-account forward |
| `telegram.user.edit_message` | User-account edit |
| `telegram.user.delete_messages` | User-account delete |
| `ai.chat` | Chat on an AI connection — OpenAI-compatible or Claude (`connection_id` + prompt) |
| `github.get_latest_release` | Latest GitHub release (`connection_id`, optional `owner`/`repo`, `track_new`) |
| `debug.log` | Log message |

Expressions: `{{trigger.amount}}`, `{{nodes.http1.body.id}}`, `{{amount > 100}}`, `{{total * 0.2}}`.

Schedules persist in `workflow_schedules` and survive SmartDo restarts. Set workflow status to **active** to arm them.

### Channel / Stream / Trigger (P7.4 config lifecycle)

Configuration hierarchy only — **no event runtime**:

```
Channel → Stream → Trigger
```

Delete is non-cascading: parents with children return HTTP 409 Conflict. UI: topbar **Channels**.

Domain kernel lives in `boarddo-shared::v2`:

| Entity | Role |
|--------|------|
| **NodeContract** | Intent + typed ports + policy + capability requirements |
| **WorkflowContract** | Graph that can be published as a composite node |
| **Artifact** | Text / image / code / URL / dataset flowing between nodes |
| **RunContext** | Variables + artifacts + capability budget + agent memory |
| **AgentSpec** | Specialized workflow + tools + memory + permission ceiling |
| **Capability** | `telegram.write`, `ads.launch`, … enforced at runtime |

Direction: intention-level nodes (`Create Telegram Post`), composite workflows-as-nodes, `human.approval`, AI node class (`ai.*`), agents as constrained workflows. SmartDo stays the deterministic executor.

### Phase 4 — AI Workflow Builder (planned)

`Goal → WorkflowPlan → editable canvas → validate → SmartDo execute`. Planner proposes graphs; humans edit; engine never invents control flow at runtime.

### Goal Agent Runtime

Separate from the canvas editor agent (`/api/agent/chat`). A GoalRun loops Observe → Think → Plan → Act → Evaluate. SmartDo still executes tools. Policies, daily caps, and approval gates apply to every Act. UI: top bar **Goals**.

Showcase: template `growth` — 10 interested users / 7 days / $0 / max 50 actions/day. Telegram outreach requires approval. Inbound messages containing `interested` / `хочу` increment the metric.

### Goals (objectives)

UI: rail **Goals** (flag icon). A goal is a tree of items plus characteristics, success
criteria, deadline and the AI model to use:

```
Goal → ✦ AI fill (description, characteristics, criteria, items)
     → ✦ split items into sub-items (max depth 3)
     → Run: every leaf item executes through a scenario
     → ✦ AI evaluation against the success criteria
     → ✦ refine (optional, N auto rounds): gap-closing items, run continues
```

| Item kind | Scenario |
|-----------|----------|
| `ai` | generated: `trigger.manual → ai.chat` (id `result`) |
| `research` | generated: `trigger.manual → web.search → ai.chat` |
| `workflow` | any existing scenario you pick |
| `manual` | done by a person — the run pauses and resumes when marked done |

Generated scenarios are real workflows (`🎯 Goal · 1.2 Item`), open them on the canvas from the item.
Edited on the canvas, they are never overwritten. Every item scenario receives the goal context in
`trigger`: `{{trigger.brief}}` (goal, characteristics, criteria), `{{trigger.task}}` (the item),
`{{trigger.context}}` (results of dependencies or earlier items), `{{trigger.query}}`; the result
is the `result` node's output (else the last node with output).

Items run one at a time in tree order respecting `depends_on`; transient AI errors are retried once;
progress, statuses and the activity log stream over `WS /ws/goals` (`objective_updated`).
Without an explicit connection a goal uses the first Claude connection, then any AI connection.

### Tools (Python)

Tools are small Python modules with `run(input_data)` that scenarios (node **Tool**, `tool.run`),
agents (`tool.*`) and the API call. Rail **Tools**:

- **With AI** — describe the task (plus an example input / expected output): the model writes the
  code, schemas and tests; BoardDo runs them in the sandbox and sends failures (traceback, logs,
  mismatching values) back for repair, up to 3 rounds. Generated tests are advisory — the model may
  drop one whose expectation is wrong; your examples are authoritative.
- **Code** — your own module; activated only when the checks and tests pass. A new version
  (`PUT /api/tools/{name}`) that fails keeps the previous one active.

Sandbox (`crates/backend/src/tools/harness.py`): fresh temp dir per run (removed after), cleared
environment, AST check (no eval/exec, dunder attributes, importlib, ctypes, …), an audit hook that
enforces the tool's permissions (network, file read/write outside its directory, processes), memory
limit (RSS watchdog; `RLIMIT_AS` on Linux) and timeout; prints go to logs, never into the result.
It is defence in depth for a local subprocess, not a container.

Interpreter: chosen in **Tools** (stored), else `BOARDDO_PYTHON`, else the newest *healthy*
`python3.x` found — each candidate is run and must import pyexpat/ssl/sqlite3/… (e.g. a Homebrew
build whose `pyexpat` cannot load the system libexpat is skipped: xml and pip are broken there).
*Install packages* creates `<data dir>/tools-venv` from the chosen interpreter and installs the
allowlisted packages (numpy, pandas, requests, beautifulsoup4) from PyPI; system Python is untouched.
API: `GET/PUT /api/tools/runtime`, `POST /api/tools/runtime/venv`.

API: `GET/POST /api/tools`, `POST /api/tools/generate`, `GET/PUT/DELETE /api/tools/{name}`,
`POST /api/tools/{name}/run`; errors are JSON `{error, kind, details}` with 400/404/409/422.

### ProjectHub extension

[Project Hub](https://github.com/igornet0/ProjectShit) (projects on this machine, `.brdd` analysis,
tasks, GitHub issues) plugs into BoardDo as an extension. It is **not installed by default**: rail
**ProjectHub** → *Download, build and test*. BoardDo clones the repository (or uses a local checkout),
builds `project-hub-server` into `~/.cache/boarddo/extensions/projecthub` (`PROJECTHUB_DIR`), runs its
`--self-test`, and only then offers *Enable*. Enabled, it runs as a sidecar on a random loopback port
with a random token (restarted if it dies) and shares the Project Hub desktop app's database unless
another path is set.

**Already installed?** BoardDo detects Project Hub on startup and in the panel: the desktop app
(`/Applications/Project Hub.app` ships `project-hub-server` next to its executable), a
`project-hub-server` on `PATH` / in `~/.cargo/bin`, `PROJECTHUB_BIN`, or an earlier BoardDo build.
A detected server is adopted after its self-test (installed, not enabled) — no download or build.
App updates are followed by version; if the binary disappears the extension shows *Binary missing*.
*Disconnect* forgets an adopted installation without touching the app, and it is not re-adopted
until *Use this installation* is pressed. API: `POST /api/extensions/projecthub/detect`,
`POST /api/extensions/projecthub/use-installed` (`{"binary": "…"}` optional, must be a detected one).

What appears after enabling:

- **ProjectHub panel** — projects, `.brdd` (summary, changes, AI notes), git, project commands
  (policy: off · detected · any), tasks with GitHub issues; *AI analysis* writes an engineering brief
  into `.brdd/ai-summary.md`; *Suggest tasks* lets the agent propose work and creates the picked tasks;
  *Create goal* starts a goal linked to the project.
- **Nodes** `projecthub.*` — projects, project, brdd, code (tree/file/search), git, tasks, task.create,
  task.update, command, github (sync / create issue / issues); the editor agent knows them too.
- **Goals** linked to a project — AI items get the project's `.brdd` digest as context, and the new
  item kind **ProjectHub task** becomes a task there (source `boarddo`, `external_ref
  objective:<goal>/<item>`). The goal waits; a tracker polls every 15 s and completes the item when the
  task is done in Project Hub (or its GitHub issue is closed), then resumes the goal. Ticking the item
  off in BoardDo closes the task in Project Hub.

API: `GET /api/extensions`, `POST /api/extensions/projecthub/{install,self-test,enable,disable,restart,uninstall}`,
`PUT /api/extensions/projecthub/config`, proxy `/api/projecthub/*` → `project-hub-server /api/v1/*`,
`POST /api/projecthub-ai/projects/{id}/{analyze,suggest-tasks}`.

### AlertWim extension (log window)

[AlertWim](https://github.com/igornet0/AlertWim) is a small desktop window (Tauri) that shows alerts
and logs from scenarios. It listens on `127.0.0.1:47821` with a per-machine token and announces itself
in `~/.alertwim/endpoint.json`; BoardDo reads that file and **creates the `AlertWim` connection
automatically** (`type: alertwim`, base_url + token secret), refreshing it if the address changes.

- **Not on the machine?** Settings → **AlertWim** → *Install from GitHub*: BoardDo clones the repository
  and runs `cargo build --release` into `~/.cache/boarddo/extensions/alertwim` (`ALERTWIM_DIR`), then
  starts the window. Already installed copies are detected: `ALERTWIM_BIN`, `/Applications/AlertWim.app`,
  `~/Applications/AlertWim.app`, a BoardDo build, `alertwim` on `PATH`.
- **Node** `alertwim.alert` — `level` (info · success · warn · error · debug), `title`, `message`,
  `source`, `data`, `show`, `focus`. With `launch` (default) a closed AlertWim is started first, so a
  scenario run opens the window by itself; parallel runs share one launch. A missing window does not
  fail the scenario unless `fail_on_error` is set.
- AlertWim runs in its own process group, so it stays open when `make dev` is stopped.

API: `GET /api/extensions/alertwim`, `POST /api/extensions/alertwim/{install,detect,launch,test}`.
Env: `ALERTWIM_HOME` (discovery dir), `ALERTWIM_BIN`, `ALERTWIM_REPO`, `ALERTWIM_BRANCH`, `ALERTWIM_DIR`.

### Claude

Add a connection of type `anthropic` (Settings → AI agent → Add provider → **Claude (Anthropic)**,
or Connections). It speaks the native Messages API (`x-api-key`, `anthropic-version: 2023-06-01`)
and plugs into every chat surface: `ai.*` nodes, the editor agent, the campaign planner and goals.

```bash
POST /api/connections
{
  "name": "Claude",
  "type": "anthropic",
  "config": { "provider": "anthropic", "base_url": "https://api.anthropic.com/v1", "default_model": "claude-opus-5-5" },
  "secret": { "api_key": "sk-ant-…" }
}
```

Models: `claude-opus-5-5` (default), `claude-sonnet-5-5`, `claude-haiku-4-5`, `claude-fable-5-1`.
Sampling params are only sent to models that accept them, short calls run at low effort, thinking
models get a token floor, and `fallbacks: "default"` is enabled for Opus 5.x / Sonnet 5.5 / Fable on
the Anthropic API. A safety decline (`stop_reason: "refusal"`) surfaces as an error. Claude
connections are text-only: `ai.image` / `ai.audio` / `ai.video` need an OpenAI-compatible one.

### Connections / Secrets

Secrets never enter workflow JSON or API responses:

```bash
POST /api/connections
{
  "name": "Production API",
  "type": "http",
  "config": { "base_url": "https://api.example.com" },
  "secret": { "auth": { "type": "bearer", "token": "…" } }
}
```

Response only has `id`, `name`, `type`, `config`, `has_secret`, `enabled`.

`http.request` uses `"connection_id": "<uuid>"`. Set `BOARDDO_SECRETS_KEY` (64 hex chars) in production.

### API

- `GET/POST /api/workflows`
- `GET/PUT/DELETE /api/workflows/:id`
- `POST /api/workflows/:id/validate`
- `POST /api/workflows/:id/run`
- `GET /api/workflows/:id/schedules`
- `GET/POST /api/connections`
- `GET/PUT/DELETE /api/connections/:id`
- `POST /api/connections/:id/test`
- `GET/POST /api/goals`
- `GET/PUT/DELETE /api/goals/:id`
- `POST /api/goals/:id/start|stop`
- `POST /api/goals/:id/events` (`lead.interested`)
- `GET /api/goal-runs`, `/api/goal-runs/:id/{activity,experiments,memory}`
- `GET /api/approvals`, `POST /api/approvals/:id/approve|reject`
- `GET /api/agent-templates`
- `GET/POST /api/objectives`, `GET/PUT/DELETE /api/objectives/:id`
- `POST /api/objectives/:id/assist` (`mode`: `fill` | `decompose` + `item_id` | `refine`; `apply`)
- `POST /api/objectives/:id/run` (`retry_failed`, `reset`, `item_id`), `POST /api/objectives/:id/stop`
- `PATCH /api/objectives/:id/items/:item_id` (`status`, `result`, `workflow_id`, `unlink_workflow`)
- `POST /api/objectives/:id/items/:item_id/scenario` — create/refresh the item scenario without running
- `WS /ws/goals` — GoalRun live events
- `GET/POST /api/channels`
- `GET/PUT/DELETE /api/channels/:id`
- `GET/POST /api/streams` (`?channel_id=`)
- `GET/PUT/DELETE /api/streams/:id`
- `GET/POST /api/triggers` (`?stream_id=`)
- `GET/PUT/DELETE /api/triggers/:id`
- `POST /api/webhooks/:workflow_id`
- `GET /api/executions`, `/api/executions/:id`, `/api/executions/:id/nodes`
- `WS /ws` — execution live events
- `GET/POST /api/telegram/accounts`
- `GET/DELETE /api/telegram/accounts/:id`
- `POST /api/telegram/accounts/:id/connect|disconnect|start|stop`
- `GET /api/telegram/accounts/:id/status`
- `POST /api/telegram/accounts/:id/auth/phone|code|password`
- `GET /api/telegram/accounts/:id/chats`
- `GET /api/telegram/accounts/:id/messages`
- `POST /api/telegram/accounts/:id/messages/send`
- `GET /api/telegram/audit`
- `WS /ws/telegram` — user-account domain events

Bot API (`telegram.send_*` + `connection_id` / `bot_token`) is unchanged. User accounts are a separate stack (TDLib sessions on disk, never in SQLite).

`cargo test -p boarddo-telegram` prints `TELEGRAM_ENGINE_STATUS=READY` when the vertical slice passes.
