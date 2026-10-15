import { Fragment, useCallback, useEffect, useMemo, useState } from "react";

import { ApiError } from "../api/client";
import {
  createToolsVenv,
  getToolsRuntime,
  setToolsPython,
  type ToolsRuntime,
  createTool,
  deleteTool,
  generateTool,
  getTool,
  listTools,
  runTool,
  type BuildReport,
  type ToolDetail,
  type ToolPermissions,
  type ToolRunResult,
  type ToolSummary,
} from "../api/tools";
import { usePreferences } from "../settings/PreferencesContext";
import type { MessageKey } from "../i18n/messages";
import { Modal, useConfirm } from "../ui/Modal";
import {
  Badge,
  Card,
  EmptyState,
  Field,
  Notice,
  Segmented,
} from "../ui/primitives";
import { formatRelative } from "../ui/format";
import {
  IconPlay,
  IconPlus,
  IconRefresh,
  IconSparkle,
  IconTrash,
  IconWrench,
} from "../ui/icons";

type Props = {
  open: boolean;
  onClose: () => void;
};

const PERMISSION_KEYS = [
  "network",
  "filesystem_read",
  "filesystem_write",
  "shell",
] as const;

const CODE_TEMPLATE = `def run(input_data):
    """input_data: dict from the scenario / API. Return JSON-serializable data."""
    numbers = input_data.get("numbers")
    if not isinstance(numbers, list) or not numbers:
        raise ValueError("numbers: a non-empty list is required")
    return {"count": len(numbers), "total": sum(numbers)}
`;

function parseJson(text: string): { ok: true; value: unknown } | { ok: false } {
  if (!text.trim()) return { ok: true, value: undefined };
  try {
    return { ok: true, value: JSON.parse(text) as unknown };
  } catch {
    return { ok: false };
  }
}

function pretty(v: unknown): string {
  return JSON.stringify(v, null, 2);
}

export function ToolsPanel({ open, onClose }: Props) {
  const { t } = usePreferences();
  const [tools, setTools] = useState<ToolSummary[]>([]);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [report, setReport] = useState<BuildReport | null>(null);
  const [selected, setSelected] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setTools((await listTools()).tools);
    } catch (e) {
      setMessage(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) {
      setMessage(null);
      void refresh();
    }
  }, [open, refresh]);

  async function onCreated(name: string, note: string) {
    setReport(null);
    setMessage(note);
    await refresh();
    setSelected(name);
  }

  function onFailed(e: unknown) {
    const details =
      e instanceof ApiError
        ? (e.body?.details as BuildReport | undefined)
        : undefined;
    setReport(details ?? null);
    setMessage(e instanceof Error ? e.message : String(e));
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      size="lg"
      icon={<IconWrench size={18} />}
      title={t("tools.title")}
      subtitle={t("tools.hint")}
      actions={
        <button
          type="button"
          className="icon-btn"
          onClick={() => void refresh()}
          aria-label={t("common.refresh")}
          data-tip={t("common.refresh")}
        >
          <IconRefresh size={15} className={loading ? "spin" : undefined} />
        </button>
      }
    >
      <RuntimeBar open={open} />
      <CreateTool onCreated={onCreated} onFailed={onFailed} />

      <Notice text={message} onDismiss={() => setMessage(null)} />
      {report && (
        <BuildReportView report={report} onClose={() => setReport(null)} />
      )}

      <h3 className="section-title">
        {t("tools.list")}{" "}
        <span className="section-title__count">{tools.length}</span>
      </h3>
      {tools.length === 0 ? (
        <EmptyState
          compact
          icon={<IconWrench size={20} />}
          title={t("tools.empty")}
          hint={t("tools.emptyHint")}
        />
      ) : (
        <div className="table-wrap">
          <table className="table tools-table">
            <thead>
              <tr>
                <th>{t("tools.colName")}</th>
                <th>{t("tools.colStatus")}</th>
                <th className="num">{t("tools.colSuccess")}</th>
                <th className="num">{t("tools.colAvgMs")}</th>
              </tr>
            </thead>
            <tbody>
              {tools.map((tool) => {
                const stats = tool.execution_stats;
                const rate =
                  stats.execution_count > 0
                    ? Math.round(
                        (100 * stats.success_count) / stats.execution_count,
                      )
                    : null;
                const isOpen = selected === tool.name;
                return (
                  <Fragment key={tool.id}>
                    <tr
                      className={`tools-table__row${isOpen ? " is-selected" : ""}`}
                      onClick={() => setSelected(isOpen ? null : tool.name)}
                    >
                      <td>
                        <strong className="mono">{tool.name}</strong>
                        <div className="table__sub">{tool.description}</div>
                      </td>
                      <td>
                        {tool.active_version_id ? (
                          <Badge tone="success" dot>
                            {t("tools.active")}
                          </Badge>
                        ) : (
                          <Badge>—</Badge>
                        )}
                      </td>
                      <td className="num">
                        {rate == null ? (
                          "—"
                        ) : (
                          <span
                            className={
                              rate >= 90
                                ? "tone-success"
                                : rate >= 50
                                  ? "tone-warning"
                                  : "tone-error"
                            }
                          >
                            {rate}%
                          </span>
                        )}
                        {stats.execution_count > 0 && (
                          <div className="table__sub">
                            {stats.execution_count}
                          </div>
                        )}
                      </td>
                      <td className="num mono">
                        {stats.average_duration_ms || "—"}
                      </td>
                    </tr>
                    {isOpen && (
                      <tr className="tools-table__detail">
                        <td colSpan={4}>
                          <ToolDetails
                            name={tool.name}
                            onChanged={() => void refresh()}
                            onDeleted={() => {
                              setSelected(null);
                              void refresh();
                            }}
                          />
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Modal>
  );
}

// ─── Create ──────────────────────────────────────────────────────────────────

function CreateTool({
  onCreated,
  onFailed,
}: {
  onCreated: (name: string, note: string) => Promise<void>;
  onFailed: (e: unknown) => void;
}) {
  const { t } = usePreferences();
  const [mode, setMode] = useState<"ai" | "code">("ai");
  const [busy, setBusy] = useState(false);
  const [description, setDescription] = useState("");
  const [name, setName] = useState("");
  const [code, setCode] = useState(CODE_TEMPLATE);
  const [testInput, setTestInput] = useState('{"numbers": [1, 2, 3]}');
  const [expected, setExpected] = useState('{"count": 3, "total": 6}');
  const [perms, setPerms] = useState<Partial<ToolPermissions>>({});

  const input = parseJson(testInput);
  const expect = parseJson(expected);
  const jsonOk = input.ok && expect.ok;
  const example =
    input.ok && input.value !== undefined
      ? [{ input: input.value, expected: expect.ok ? expect.value : undefined }]
      : [];

  async function submit() {
    setBusy(true);
    try {
      if (mode === "ai") {
        const res = await generateTool({
          description,
          name: name.trim() || undefined,
          examples: example,
          permissions: perms,
        });
        const repairs = res.version.repair_history.length;
        setDescription("");
        await onCreated(
          res.tool.name,
          t("tools.generated", {
            name: res.tool.name,
            tests: res.version.test_cases.length,
            repairs,
          }),
        );
      } else {
        const res = await createTool({
          name: name.trim(),
          description: description.trim() || name.trim(),
          code,
          tests: example.length ? example : undefined,
          test_input: example.length ? undefined : {},
          permissions: perms,
        });
        await onCreated(res.tool.name, t("tools.created"));
      }
    } catch (e) {
      onFailed(e);
    } finally {
      setBusy(false);
    }
  }

  const canSubmit =
    !busy &&
    jsonOk &&
    (mode === "ai"
      ? description.trim().length >= 8
      : name.trim().length >= 2 && code.includes("def run"));

  return (
    <Card
      title={t("tools.create")}
      subtitle={
        mode === "ai" ? t("tools.createHintAi") : t("tools.createHintCode")
      }
      actions={
        <Segmented
          value={mode}
          onChange={setMode}
          options={[
            {
              value: "ai",
              label: t("tools.mode.ai"),
              icon: <IconSparkle size={12} />,
            },
            {
              value: "code",
              label: t("tools.mode.code"),
              icon: <IconWrench size={12} />,
            },
          ]}
        />
      }
    >
      <div className="form tools-create">
        <Field
          label={mode === "ai" ? t("tools.task") : t("tools.description")}
          span="full"
        >
          <textarea
            rows={mode === "ai" ? 3 : 2}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder={t("tools.createPlaceholder")}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSubmit)
                void submit();
            }}
          />
        </Field>
        <div className="form__row">
          <Field
            label={t("tools.name")}
            hint={mode === "ai" ? t("tools.nameAuto") : t("tools.nameRule")}
          >
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="csv_report"
              spellCheck={false}
              className="mono"
            />
          </Field>
          <Field label={t("tools.permissions")}>
            <div className="tools-perms">
              {PERMISSION_KEYS.map((k) => (
                <label key={k} className="tools-perm">
                  <input
                    type="checkbox"
                    checked={Boolean(perms[k])}
                    onChange={(e) =>
                      setPerms((p) => ({ ...p, [k]: e.target.checked }))
                    }
                  />
                  {t(`tools.perm.${k}` as MessageKey)}
                </label>
              ))}
            </div>
          </Field>
        </div>
        {mode === "code" && (
          <Field label={t("tools.code")} span="full">
            <textarea
              rows={10}
              value={code}
              onChange={(e) => setCode(e.target.value)}
              spellCheck={false}
              className="mono tools-code"
            />
          </Field>
        )}
        <div className="form__row">
          <Field
            label={t("tools.testInput")}
            hint={
              !input.ok ? t("editor.invalidJson") : t("tools.testInputHint")
            }
          >
            <textarea
              rows={3}
              value={testInput}
              onChange={(e) => setTestInput(e.target.value)}
              spellCheck={false}
              className="mono"
            />
          </Field>
          <Field
            label={t("tools.expected")}
            hint={
              !expect.ok ? t("editor.invalidJson") : t("tools.expectedHint")
            }
          >
            <textarea
              rows={3}
              value={expected}
              onChange={(e) => setExpected(e.target.value)}
              spellCheck={false}
              className="mono"
            />
          </Field>
        </div>
        <div className="form__actions">
          <button
            type="button"
            className="btn btn--primary"
            disabled={!canSubmit}
            onClick={() => void submit()}
          >
            {busy ? (
              <span className="spinner spinner--xs" />
            ) : mode === "ai" ? (
              <IconSparkle size={14} />
            ) : (
              <IconPlus size={14} />
            )}
            {busy
              ? mode === "ai"
                ? t("tools.generating")
                : t("tools.testing")
              : mode === "ai"
                ? t("tools.generate")
                : t("tools.createButton")}
          </button>
        </div>
      </div>
    </Card>
  );
}

function BuildReportView({
  report,
  onClose,
}: {
  report: BuildReport;
  onClose: () => void;
}) {
  const { t } = usePreferences();
  return (
    <div className="code-block tools-report">
      <div className="code-block__head">
        <span>
          {t("tools.buildFailed")}
          {report.repairs.length > 0 &&
            ` · ${t("tools.repairs", { count: report.repairs.length })}`}
        </span>
        <button
          type="button"
          className="btn btn--ghost btn--xs"
          onClick={onClose}
        >
          {t("common.close")}
        </button>
      </div>
      {report.failure && (
        <p className="tone-error tools-report__reason">{report.failure}</p>
      )}
      {report.traceback && <pre>{report.traceback}</pre>}
      {report.logs && <pre className="muted">{report.logs}</pre>}
      <details>
        <summary>{t("tools.lastCode")}</summary>
        <pre>{report.code}</pre>
      </details>
    </div>
  );
}

// ─── Details / run ───────────────────────────────────────────────────────────

function ToolDetails({
  name,
  onChanged,
  onDeleted,
}: {
  name: string;
  onChanged: () => void;
  onDeleted: () => void;
}) {
  const { t, locale } = usePreferences();
  const confirm = useConfirm();
  const [detail, setDetail] = useState<ToolDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = useState("{}");
  const [result, setResult] = useState<ToolRunResult | null>(null);
  const [running, setRunning] = useState(false);
  const [tab, setTab] = useState<"run" | "code" | "tests" | "history">("run");

  const load = useCallback(async () => {
    try {
      const d = await getTool(name);
      setDetail(d);
      return d;
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return null;
    }
  }, [name]);

  useEffect(() => {
    void load().then((d) => {
      const active = d?.versions.find((v) => v.id === d.tool.active_version_id);
      const first = active?.test_cases[0]?.input;
      if (first !== undefined) setInput(pretty(first));
    });
  }, [load]);

  const active = useMemo(
    () =>
      detail?.versions.find((v) => v.id === detail.tool.active_version_id) ??
      detail?.versions[0],
    [detail],
  );
  const parsed = parseJson(input);

  async function run() {
    if (!parsed.ok) return;
    setRunning(true);
    setResult(null);
    try {
      setResult(await runTool(name, parsed.value ?? {}));
      onChanged();
      void load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRunning(false);
    }
  }

  if (!detail || !active) {
    return error ? (
      <Notice text={error} tone="error" />
    ) : (
      <div className="skeleton skeleton--row" />
    );
  }
  const perms = PERMISSION_KEYS.filter((k) => active.permissions[k]);

  return (
    <Card
      className="tools-detail"
      title={
        <span className="mono">
          {detail.tool.name} <span className="muted">v{active.version}</span>
        </span>
      }
      subtitle={detail.tool.description}
      actions={
        <button
          type="button"
          className="btn btn--ghost btn--sm"
          onClick={async () => {
            const ok = await confirm({
              title: t("tools.delete"),
              message: t("tools.deleteConfirm", { name: detail.tool.name }),
              confirmLabel: t("tools.delete"),
              danger: true,
            });
            if (!ok) return;
            try {
              await deleteTool(detail.tool.name);
              onDeleted();
            } catch (e) {
              setError(e instanceof Error ? e.message : String(e));
            }
          }}
        >
          <IconTrash size={13} />
          {t("tools.delete")}
        </button>
      }
    >
      <div className="chip-row tools-meta">
        <span className="chip">
          {t(`tools.origin.${detail.tool.origin.toLowerCase()}` as MessageKey)}
        </span>
        <span className="chip chip--mono">
          {active.runtime_config.timeout_secs}s ·{" "}
          {active.runtime_config.memory_mb} MB
        </span>
        {perms.length === 0 ? (
          <span className="chip">{t("tools.perm.none")}</span>
        ) : (
          perms.map((k) => (
            <span key={k} className="chip tone-warning">
              {t(`tools.perm.${k}` as MessageKey)}
            </span>
          ))
        )}
        {active.repair_history.length > 0 && (
          <span className="chip">
            {t("tools.repairs", { count: active.repair_history.length })}
          </span>
        )}
      </div>
      <Notice text={error} tone="error" onDismiss={() => setError(null)} />

      <Segmented
        value={tab}
        onChange={setTab}
        options={[
          { value: "run", label: t("tools.tab.run") },
          { value: "code", label: t("tools.tab.code") },
          {
            value: "tests",
            label: `${t("tools.tab.tests")} · ${active.test_cases.length}`,
          },
          {
            value: "history",
            label: `${t("tools.tab.history")} · ${detail.executions.length}`,
          },
        ]}
      />

      {tab === "run" && (
        <div className="form tools-run">
          <Field
            label={t("tools.input")}
            hint={parsed.ok ? t("tools.inputHint") : t("editor.invalidJson")}
            span="full"
          >
            <textarea
              rows={6}
              value={input}
              onChange={(e) => setInput(e.target.value)}
              spellCheck={false}
              className="mono"
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void run();
              }}
            />
          </Field>
          <div className="form__actions">
            <button
              type="button"
              className="btn btn--primary"
              disabled={running || !parsed.ok}
              onClick={() => void run()}
            >
              {running ? (
                <span className="spinner spinner--xs" />
              ) : (
                <IconPlay size={12} />
              )}
              {t("tools.run")}
            </button>
          </div>
          {result && <RunResult result={result} />}
        </div>
      )}

      {tab === "code" && (
        <div className="code-block">
          <pre>{active.source_code}</pre>
        </div>
      )}

      {tab === "tests" && (
        <div className="tools-tests">
          {active.test_cases.map((c, i) => (
            <div key={i} className="tools-test">
              <div>
                <span className="muted">{t("tools.input")}</span>
                <pre>{pretty(c.input)}</pre>
              </div>
              <div>
                <span className="muted">{t("tools.expected")}</span>
                <pre>
                  {c.expected === undefined || c.expected === null
                    ? "—"
                    : pretty(c.expected)}
                </pre>
              </div>
            </div>
          ))}
          {active.repair_history.map((r) => (
            <p key={r.attempt} className="field__hint">
              {t("tools.repairLine", {
                attempt: r.attempt,
                by: r.fixed_by === "ai" ? "AI" : "auto",
              })}
              : {r.error_message}
            </p>
          ))}
        </div>
      )}

      {tab === "history" &&
        (detail.executions.length === 0 ? (
          <p className="muted">{t("tools.noRuns")}</p>
        ) : (
          <ul className="tools-history">
            {detail.executions.map((ex) => (
              <li key={ex.id}>
                <Badge tone={ex.status === "success" ? "success" : "error"} dot>
                  {ex.status === "success" ? "OK" : ex.error_type || "Error"}
                </Badge>
                <span className="mono">v{ex.version}</span>
                <span className="mono">{ex.duration_ms} ms</span>
                <span className="muted">
                  {formatRelative(ex.created_at, locale)}
                </span>
                {ex.message && (
                  <span className="tone-error tools-history__msg">
                    {ex.message}
                  </span>
                )}
              </li>
            ))}
          </ul>
        ))}
    </Card>
  );
}

function RunResult({ result }: { result: ToolRunResult }) {
  const { t } = usePreferences();
  const ok = result.status === "success";
  return (
    <div className="code-block">
      <div className="code-block__head">
        <span>
          <Badge tone={ok ? "success" : "error"} dot>
            {ok ? t("tools.output") : result.error_type || "Error"}
          </Badge>{" "}
          {result.duration_ms != null && (
            <span className="mono muted">{result.duration_ms} ms</span>
          )}
        </span>
      </div>
      {ok ? (
        <pre>{pretty(result.output)}</pre>
      ) : (
        <p className="tone-error tools-report__reason">{result.message}</p>
      )}
      {!ok && result.traceback && <pre>{result.traceback}</pre>}
      {result.logs && (
        <details open={!ok}>
          <summary>{t("tools.logs")}</summary>
          <pre className="muted">{result.logs}</pre>
        </details>
      )}
    </div>
  );
}

// ─── Python runtime ──────────────────────────────────────────────────────────

function RuntimeBar({ open }: { open: boolean }) {
  const { t } = usePreferences()
  const confirm = useConfirm()
  const [rt, setRt] = useState<ToolsRuntime | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [log, setLog] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    getToolsRuntime()
      .then(setRt)
      .catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)))
  }, [open])

  async function act(key: string, fn: () => Promise<ToolsRuntime>) {
    setBusy(key)
    setError(null)
    try {
      setRt(await fn())
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(null)
    }
  }

  if (!rt) return error ? <Notice text={error} tone="error" /> : null
  const cur = rt.current
  const missing = rt.packages.filter((p) => !cur?.packages[p.import])

  return (
    <div className="tools-runtime">
      <div className="tools-runtime__main">
        <span className="muted">{t('tools.runtime.python')}</span>
        <select
          value={rt.mode === 'manual' ? (cur?.path ?? '') : ''}
          disabled={busy !== null}
          onChange={(e) => void act('select', () => setToolsPython(e.target.value || null))}
        >
          <option value="">
            {t('tools.runtime.auto')}
            {rt.automatic ? ` (${rt.automatic.version})` : ''}
          </option>
          {rt.candidates.map((c) => (
            <option key={c.path} value={c.path}>
              {c.version} — {c.venv ? t('tools.runtime.venv') : c.path}
              {c.broken.length > 0 ? ` · ⚠ ${t('tools.runtime.broken')}` : ''}
            </option>
          ))}
        </select>
        <span className="chip-row">
          {rt.packages.map((p) => (
            <span key={p.import} className={`chip chip--mono ${cur?.packages[p.import] ? 'tone-success' : 'muted'}`}>
              {cur?.packages[p.import] ? '✓' : '✕'} {p.import}
            </span>
          ))}
        </span>
        {missing.length > 0 && cur && cur.broken.length === 0 && (
          <button
            type="button"
            className="btn btn--sm"
            disabled={busy !== null}
            onClick={async () => {
              const names = missing.map((p) => p.pip)
              const ok = await confirm({
                title: t('tools.runtime.install'),
                message: t('tools.runtime.installConfirm', { python: cur.version, packages: names.join(', '), path: rt.venv ?? '' }),
                confirmLabel: t('tools.runtime.install'),
              })
              if (!ok) return
              await act('venv', async () => {
                const res = await createToolsVenv({ python: cur.venv ? undefined : cur.executable, packages: names })
                setLog(res.log)
                return getToolsRuntime()
              })
            }}
          >
            {busy === 'venv' ? <span className="spinner spinner--xs" /> : <IconPlus size={12} />}
            {busy === 'venv' ? t('tools.runtime.installing') : t('tools.runtime.install')}
          </button>
        )}
      </div>
      {cur && cur.broken.length > 0 && (
        <Notice tone="warning" text={t('tools.runtime.brokenHint', { version: cur.version, modules: cur.broken.join('; ') })} />
      )}
      {rt.candidates
        .filter((c) => c.broken.length > 0 && c.path !== cur?.path)
        .map((c) => (
          <p key={c.path} className="field__hint">
            ⚠ Python {c.version} ({c.path}): {c.broken.join('; ')}
          </p>
        ))}
      <Notice text={error} tone="error" onDismiss={() => setError(null)} />
      {log && (
        <details className="tools-runtime__log">
          <summary>{t('tools.logs')}</summary>
          <pre>{log}</pre>
        </details>
      )}
    </div>
  )
}
