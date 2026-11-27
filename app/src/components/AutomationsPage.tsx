import { useEffect, useRef, useState } from "react";
import { MessageSquare, Pencil, Play, Plus, Trash2 } from "lucide-react";

import { useI18n } from "../i18n";
import type { AgentOption } from "../lib/agents";
import { api, type Automation, type AutomationRun, type Snapshot } from "../lib/api";
import { findTask } from "../lib/format";
import {
  describeSchedule,
  formatWhen,
  fromCron,
  PRESET_KINDS,
  switchPreset,
  toCron,
  weekdayName,
  type Preset,
} from "../lib/schedule";
import { AgentIcon } from "./AgentIcon";

const ICON = { size: 14, strokeWidth: 1.75 };

const PRESET_LABEL = {
  daily: "schedule.daily",
  weekdays: "schedule.weekdays",
  weekly: "schedule.weekly",
  hourly: "schedule.hourly",
  custom: "schedule.custom",
} as const;

/** 编辑时"在哪里做"的选择：不指定项目、每次在某个项目里新建任务、发进某个任务。 */
type Place = { kind: "none" } | { kind: "project"; project: string } | { kind: "task"; task: number };

const placeValue = (p: Place) => (p.kind === "none" ? "none" : p.kind === "project" ? `p:${p.project}` : `t:${p.task}`);

function parsePlace(value: string): Place {
  if (value.startsWith("p:")) return { kind: "project", project: value.slice(2) };
  if (value.startsWith("t:")) return { kind: "task", task: Number(value.slice(2)) };
  return { kind: "none" };
}

/**
 * 定时任务页面（侧边栏顶部的闹钟进来）：所有定时任务按在哪里做分组，可以只看一个项目。
 * 到点时 Duolo 交给 Agent 一个需求：每次在项目里新建任务，或者发进一个已有的任务接着对话；
 * 不针对哪个项目的放在专门的项目里。只在 Duolo 开着时执行（关了窗口也算开着）。
 */
export function AutomationsPage({
  snap,
  filter,
  currentProject,
  agents,
  onFilter,
  onOpenTask,
  onError,
}: {
  snap: Snapshot;
  /** 只看这个项目的；null 时看全部。 */
  filter: string | null;
  /** 新建时默认的项目（正在看的任务所在的项目）。 */
  currentProject: string | null;
  agents: AgentOption[];
  onFilter(project: string | null): void;
  onOpenTask(task: number): void;
  onError(err: unknown): void;
}) {
  const { t } = useI18n();
  const [editing, setEditing] = useState<Automation | "new" | null>(null);
  const [removing, setRemoving] = useState<number | null>(null);
  const scratch = snap.automationsProject;
  const projectName = (id: string) =>
    id === scratch ? t("automation.noProject") : (snap.projects.find((p) => p.id === id)?.name ?? id);

  useEffect(() => {
    if (!editing) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      setEditing(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [editing]);

  // 分组：不指定项目的在最前，其余按侧边栏里项目的顺序；项目被移除了的排在最后。
  const shown = snap.automations.filter((a) => filter == null || a.project === filter);
  const order = [scratch, ...snap.projects.map((p) => p.id)].filter((id): id is string => id != null);
  const groups = [...new Set([...order, ...shown.map((a) => a.project)])]
    .map((project) => ({ project, items: shown.filter((a) => a.project === project) }))
    .filter((g) => g.items.length > 0);
  const filters = [...new Set(snap.automations.map((a) => a.project))];

  const run = (id: number) =>
    api.runAutomation(id).then((tasks) => tasks[0] != null && onOpenTask(tasks[0]), onError);

  const defaultPlace = (): Place => {
    const project = filter ?? currentProject;
    return project == null || project === scratch ? { kind: "none" } : { kind: "project", project };
  };

  return (
    <div className="automations-page">
      <div className="topbar" data-tauri-drag-region />
      <div className="automations-scroll">
        <div className="automations-body">
          <div className="automations-head">
            <h1 className="grow">{t("automation.title")}</h1>
            {!editing && (
              <button className="btn small" onClick={() => setEditing("new")}>
                <Plus {...ICON} />
                {t("automation.new")}
              </button>
            )}
          </div>

          {editing ? (
            <Editor
              key={editing === "new" ? "new" : editing.id}
              snap={snap}
              automation={editing === "new" ? null : editing}
              initialPlace={defaultPlace()}
              agents={agents}
              onDone={() => setEditing(null)}
              onError={onError}
            />
          ) : (
            <>
              {filters.length >= 2 && (
                <div className="handoff-agents">
                  <button className={`chip ${filter == null ? "is-open" : ""}`} onClick={() => onFilter(null)}>
                    {t("automation.all")}
                  </button>
                  {filters.map((id) => (
                    <button key={id} className={`chip ${filter === id ? "is-open" : ""}`} onClick={() => onFilter(id)}>
                      <span className="ellipsis">{projectName(id)}</span>
                    </button>
                  ))}
                </div>
              )}
              {groups.length === 0 && (
                <div className="automations-empty">
                  <p>{t("automation.empty")}</p>
                  <p className="dim">{t("automation.askAgent")}</p>
                </div>
              )}
              {groups.map((g) => (
                <section key={g.project} className="automation-group">
                  <h2>{projectName(g.project)}</h2>
                  <ul className="automation-list">
                    {g.items.map((a) => (
                      <li key={a.id} className={`automation-row ${a.enabled ? "" : "is-paused"}`}>
                        <div className="automation-line">
                          {a.task != null ? (
                            <MessageSquare size={13} strokeWidth={1.75} className="dim" />
                          ) : (
                            <AgentIcon kind={a.agent ?? ""} size={13} />
                          )}
                          <span className="grow ellipsis automation-name" title={a.prompt}>
                            {a.name}
                          </span>
                          {removing === a.id ? (
                            <>
                              <button className="btn small" onClick={() => setRemoving(null)}>
                                {t("common.cancel")}
                              </button>
                              <button
                                className="btn small primary danger"
                                onClick={() => {
                                  setRemoving(null);
                                  api.removeAutomation(a.id).catch(onError);
                                }}
                              >
                                {t("automation.remove")}
                              </button>
                            </>
                          ) : (
                            <>
                              <button
                                className="icon-btn"
                                title={t("automation.runNow")}
                                disabled={a.runRequested}
                                onClick={() => void run(a.id)}
                              >
                                <Play {...ICON} />
                              </button>
                              <button className="icon-btn" title={t("automation.edit")} onClick={() => setEditing(a)}>
                                <Pencil {...ICON} />
                              </button>
                              <button
                                className="icon-btn"
                                title={t("automation.remove")}
                                onClick={() => setRemoving(a.id)}
                              >
                                <Trash2 {...ICON} />
                              </button>
                              <input
                                type="checkbox"
                                className="switch"
                                checked={a.enabled}
                                title={t(a.enabled ? "automation.pause" : "automation.resume")}
                                onChange={(e) => api.setAutomationEnabled(a.id, e.target.checked).catch(onError)}
                              />
                            </>
                          )}
                        </div>
                        <When automation={a} snap={snap} onOpenTask={onOpenTask} />
                        {a.runs.slice(0, 3).map((r) => (
                          <RunLine key={`${r.scheduledFor}-${r.at}`} run={r} snap={snap} onOpenTask={onOpenTask} />
                        ))}
                      </li>
                    ))}
                  </ul>
                </section>
              ))}
              <p className="automation-note">{t("automation.note")}</p>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** 时间表、下一次的时间；发进已有任务的再写是哪个任务。 */
function When({ automation: a, snap, onOpenTask }: { automation: Automation; snap: Snapshot; onOpenTask(task: number): void }) {
  const { t, locale } = useI18n();
  const task = a.task != null ? findTask(snap, a.task)?.task : undefined;
  return (
    <div className="automation-when">
      {describeSchedule(t, locale, a.cron)}
      {" · "}
      {!a.enabled
        ? t("automation.paused")
        : a.nextRunAt != null
          ? t("automation.next", { when: formatWhen(locale, a.nextRunAt) })
          : t("automation.never")}
      {a.task != null && " · "}
      {a.task != null &&
        (task ? (
          <button className="link-btn" onClick={() => onOpenTask(task.id)}>
            {t("automation.inTask", { task: task.title })}
          </button>
        ) : (
          t("automation.taskGone")
        ))}
    </div>
  );
}

/** 一次执行的结果：什么时候、建了（发进了）哪个任务，或者为什么没执行。 */
function RunLine({ run, snap, onOpenTask }: { run: AutomationRun; snap: Snapshot; onOpenTask(task: number): void }) {
  const { t, locale } = useI18n();
  const when = formatWhen(locale, run.manual ? run.at : run.scheduledFor);
  const task = findTask(snap, run.task)?.task;
  let text: string;
  switch (run.outcome) {
    case "starting":
      text = t("automation.runStarting");
      break;
    case "started":
      text = task ? task.title : t("automation.runClosed");
      break;
    case "skipped":
      text = t(`automation.skip.${run.code ?? "missed"}` as never);
      break;
    case "failed":
      text =
        run.code && run.code !== "internal"
          ? t("automation.runFailed", { reason: t(`error.${run.code}` as never, run.params) })
          : t("automation.runFailedUnknown");
      break;
  }
  return (
    <div className={`automation-run is-${run.outcome}`}>
      <span className="automation-run-when">
        {when}
        {run.manual && ` · ${t("automation.manual")}`}
      </span>
      {task ? (
        <button className="link-btn ellipsis" onClick={() => onOpenTask(task.id)}>
          {text}
        </button>
      ) : (
        <span className="ellipsis">{text}</span>
      )}
    </div>
  );
}

function Editor({
  snap,
  automation,
  initialPlace,
  agents,
  onDone,
  onError,
}: {
  snap: Snapshot;
  automation: Automation | null;
  initialPlace: Place;
  agents: AgentOption[];
  onDone(): void;
  onError(err: unknown): void;
}) {
  const { t, locale } = useI18n();
  const scratch = snap.automationsProject;
  const [name, setName] = useState(automation?.name ?? "");
  const [prompt, setPrompt] = useState(automation?.prompt ?? "");
  const [place, setPlace] = useState<Place>(() =>
    !automation
      ? initialPlace
      : automation.task != null
        ? { kind: "task", task: automation.task }
        : automation.project === scratch
          ? { kind: "none" }
          : { kind: "project", project: automation.project },
  );
  const [agent, setAgent] = useState<string | null>(automation?.agent ?? null);
  const [preset, setPreset] = useState<Preset>(() =>
    automation ? fromCron(automation.cron) : { kind: "weekdays", hour: 9, minute: 0 },
  );
  const [next, setNext] = useState<{ at: number | null } | { invalid: true } | null>(null);
  const [saving, setSaving] = useState(false);
  const promptBox = useRef<HTMLTextAreaElement>(null);
  const cron = toCron(preset);
  const installed = agents.filter((a) => a.installed);
  const projects = snap.projects.filter((p) => p.id !== scratch);
  const tasks = snap.projects.flatMap((p) => p.tasks.filter((x) => !x.untitled).map((task) => ({ task, project: p })));

  useEffect(() => promptBox.current?.focus(), []);

  // 下一次的时间：改时间表时预览（自定义写错了会提示）。
  useEffect(() => {
    let alive = true;
    const timer = window.setTimeout(() => {
      api.previewSchedule(cron).then(
        (at) => alive && setNext({ at }),
        () => alive && setNext({ invalid: true }),
      );
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [cron]);

  const invalid = next != null && "invalid" in next;
  const canSave = prompt.trim() !== "" && !invalid && !saving;
  const save = async () => {
    if (!canSave) return;
    setSaving(true);
    try {
      await api.saveAutomation(automation?.id ?? null, {
        name: name.trim(),
        project: place.kind === "project" ? place.project : null,
        task: place.kind === "task" ? place.task : null,
        prompt: prompt.trim(),
        agent: place.kind === "task" ? null : agent,
        base: place.kind === "project" && automation?.project === place.project ? automation.base : null,
        cron,
      });
      onDone();
    } catch (err) {
      onError(err);
      setSaving(false);
    }
  };

  const time = (p: { hour: number; minute: number }) => (
    <input
      type="time"
      className="text-input time-input"
      value={`${String(p.hour).padStart(2, "0")}:${String(p.minute).padStart(2, "0")}`}
      onChange={(e) => {
        const [h, m] = e.target.value.split(":").map(Number);
        if (Number.isInteger(h) && Number.isInteger(m)) setPreset({ ...preset, hour: h, minute: m } as Preset);
      }}
    />
  );

  return (
    <>
      <label className="handoff-field">
        <span className="handoff-label">{t("automation.prompt")}</span>
        <textarea
          ref={promptBox}
          className="text-input automation-prompt"
          rows={4}
          value={prompt}
          placeholder={t("automation.promptPlaceholder")}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !e.nativeEvent.isComposing) void save();
          }}
        />
      </label>

      <label className="handoff-field">
        <span className="handoff-label">{t("automation.where")}</span>
        <select className="text-input" value={placeValue(place)} onChange={(e) => setPlace(parsePlace(e.target.value))}>
          <option value="none">{t("automation.noProject")}</option>
          {projects.length > 0 && (
            <optgroup label={t("automation.whereNewTask")}>
              {projects.map((p) => (
                <option key={p.id} value={`p:${p.id}`}>
                  {p.name}
                </option>
              ))}
            </optgroup>
          )}
          {tasks.length > 0 && (
            <optgroup label={t("automation.whereInTask")}>
              {tasks.map(({ task, project }) => (
                <option key={task.id} value={`t:${task.id}`}>
                  {`${project.id === scratch ? t("automation.noProject") : project.name} · ${task.title}`}
                </option>
              ))}
            </optgroup>
          )}
        </select>
        <span className="automation-preview">
          {t(
            place.kind === "none"
              ? "automation.whereNoneHint"
              : place.kind === "project"
                ? "automation.whereNewTaskHint"
                : "automation.whereInTaskHint",
          )}
        </span>
      </label>

      <label className="handoff-field">
        <span className="handoff-label">{t("automation.name")}</span>
        <input
          className="text-input"
          value={name}
          placeholder={t("automation.namePlaceholder")}
          onChange={(e) => setName(e.target.value)}
        />
      </label>

      {place.kind !== "task" && (
        <div className="handoff-field">
          <span className="handoff-label">{t("automation.agent")}</span>
          <div className="handoff-agents">
            <button className={`chip ${agent == null ? "is-open" : ""}`} aria-pressed={agent == null} onClick={() => setAgent(null)}>
              {t("automation.defaultAgent")}
            </button>
            {installed.map((a) => (
              <button
                key={a.id}
                className={`chip ${a.id === agent ? "is-open" : ""}`}
                aria-pressed={a.id === agent}
                onClick={() => setAgent(a.id)}
              >
                <AgentIcon kind={a.id} size={13} />
                {a.name}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="handoff-field">
        <span className="handoff-label">{t("automation.when")}</span>
        <div className="handoff-agents">
          {PRESET_KINDS.map((kind) => (
            <button
              key={kind}
              className={`chip ${preset.kind === kind ? "is-open" : ""}`}
              aria-pressed={preset.kind === kind}
              onClick={() => setPreset(switchPreset(preset, kind))}
            >
              {t(PRESET_LABEL[kind])}
            </button>
          ))}
        </div>
        <div className="schedule-inputs">
          {preset.kind === "weekly" && (
            <select
              className="text-input"
              value={preset.day}
              onChange={(e) => setPreset({ ...preset, day: Number(e.target.value) })}
            >
              {[1, 2, 3, 4, 5, 6, 0].map((d) => (
                <option key={d} value={d}>
                  {weekdayName(locale, d)}
                </option>
              ))}
            </select>
          )}
          {(preset.kind === "daily" || preset.kind === "weekdays" || preset.kind === "weekly") && time(preset)}
          {preset.kind === "hourly" && (
            <label className="schedule-minute">
              {t("schedule.atMinute")}
              <input
                type="number"
                min={0}
                max={59}
                className="text-input"
                value={preset.minute}
                onChange={(e) => {
                  const m = Number(e.target.value);
                  if (Number.isInteger(m) && m >= 0 && m <= 59) setPreset({ kind: "hourly", minute: m });
                }}
              />
            </label>
          )}
          {preset.kind === "custom" && (
            <input
              className="text-input mono grow"
              value={preset.cron}
              placeholder="0 9 * * 1-5"
              spellCheck={false}
              onChange={(e) => setPreset({ kind: "custom", cron: e.target.value })}
            />
          )}
        </div>
        <span className={`automation-preview ${invalid ? "is-invalid" : ""}`}>
          {next == null
            ? " "
            : "invalid" in next
              ? t("schedule.invalid")
              : next.at != null
                ? t("automation.next", { when: formatWhen(locale, next.at) })
                : t("automation.never")}
          {preset.kind === "custom" && !invalid && ` · ${t("schedule.customHint")}`}
        </span>
      </div>

      <div className="dialog-actions">
        <button className="btn" onClick={onDone}>
          {t("common.cancel")}
        </button>
        <button className="btn primary" disabled={!canSave} onClick={() => void save()}>
          {saving ? t("common.pleaseWait") : t(automation ? "automation.save" : "automation.create")}
        </button>
      </div>
    </>
  );
}
