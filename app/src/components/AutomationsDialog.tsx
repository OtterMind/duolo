import { useEffect, useRef, useState } from "react";
import { Pencil, Play, Plus, Trash2 } from "lucide-react";

import { useI18n } from "../i18n";
import type { AgentOption } from "../lib/agents";
import { api, type Automation, type AutomationRun, type Project } from "../lib/api";
import { useModalFocus } from "../lib/modal";
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

/**
 * 项目的定时任务：列表（暂停、立即运行、改、删，最近几次的结果）和编辑。
 * 到点时 Duolo 在项目里新建一个任务交给 Agent；只在 Duolo 开着时执行（关了窗口也算开着）。
 */
export function AutomationsDialog({
  project,
  agents,
  onOpenTask,
  onClose,
  onError,
}: {
  project: Project;
  agents: AgentOption[];
  onOpenTask(task: number): void;
  onClose(): void;
  onError(err: unknown): void;
}) {
  const { t, locale } = useI18n();
  const [editing, setEditing] = useState<Automation | "new" | null>(project.automations.length === 0 ? "new" : null);
  const [removing, setRemoving] = useState<number | null>(null);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      // 编辑中先回到列表；本来就没有定时任务时直接关掉。
      if (editing && project.automations.length > 0) setEditing(null);
      else onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [editing, project.automations.length, onClose]);

  const run = (id: number) =>
    api.runAutomation(id).then((tasks) => tasks[0] != null && onOpenTask(tasks[0]), onError);

  return (
    <div className="scrim" onMouseDown={onClose}>
      <div
        ref={box}
        tabIndex={-1}
        className="dialog automations"
        role="dialog"
        aria-labelledby="automations-title"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="automations-head">
          <h2 id="automations-title" className="grow">
            {t("automation.title", { project: project.name })}
          </h2>
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
            project={project}
            automation={editing === "new" ? null : editing}
            agents={agents}
            onDone={() => (project.automations.length > 0 || editing !== "new" ? setEditing(null) : onClose())}
            onSaved={() => setEditing(null)}
            onError={onError}
          />
        ) : (
          <>
            <ul className="automation-list">
              {project.automations.map((a) => (
                <li key={a.id} className={`automation-row ${a.enabled ? "" : "is-paused"}`}>
                  <div className="automation-line">
                    <AgentIcon kind={a.agent ?? ""} size={13} />
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
                        <button className="icon-btn" title={t("automation.remove")} onClick={() => setRemoving(a.id)}>
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
                  <div className="automation-when">
                    {describeSchedule(t, locale, a.cron)}
                    {" · "}
                    {!a.enabled
                      ? t("automation.paused")
                      : a.nextRunAt != null
                        ? t("automation.next", { when: formatWhen(locale, a.nextRunAt) })
                        : t("automation.never")}
                  </div>
                  {a.runs.slice(0, 3).map((r) => (
                    <RunLine key={`${r.scheduledFor}-${r.at}`} run={r} project={project} onOpenTask={onOpenTask} />
                  ))}
                </li>
              ))}
            </ul>
            <p className="automation-note">{t("automation.note")}</p>
            <div className="dialog-actions">
              <button className="btn" onClick={onClose}>
                {t("common.close")}
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

/** 一次执行的结果：什么时候、建了哪个任务，或者为什么没执行。 */
function RunLine({ run, project, onOpenTask }: { run: AutomationRun; project: Project; onOpenTask(task: number): void }) {
  const { t, locale } = useI18n();
  const when = formatWhen(locale, run.manual ? run.at : run.scheduledFor);
  const task = run.task != null ? project.tasks.find((x) => x.id === run.task) : undefined;
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
  project,
  automation,
  agents,
  onDone,
  onSaved,
  onError,
}: {
  project: Project;
  automation: Automation | null;
  agents: AgentOption[];
  onDone(): void;
  onSaved(): void;
  onError(err: unknown): void;
}) {
  const { t, locale } = useI18n();
  const [name, setName] = useState(automation?.name ?? "");
  const [prompt, setPrompt] = useState(automation?.prompt ?? "");
  const [agent, setAgent] = useState<string | null>(automation?.agent ?? null);
  const [preset, setPreset] = useState<Preset>(() =>
    automation ? fromCron(automation.cron) : { kind: "weekdays", hour: 9, minute: 0 },
  );
  const [next, setNext] = useState<{ at: number | null } | { invalid: true } | null>(null);
  const [saving, setSaving] = useState(false);
  const promptBox = useRef<HTMLTextAreaElement>(null);
  const cron = toCron(preset);
  const installed = agents.filter((a) => a.installed);

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
        project: project.id,
        prompt: prompt.trim(),
        agent,
        base: automation?.base ?? null,
        cron,
      });
      onSaved();
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
        <span className="handoff-label">{t("automation.name")}</span>
        <input
          className="text-input"
          value={name}
          placeholder={t("automation.namePlaceholder")}
          onChange={(e) => setName(e.target.value)}
        />
      </label>

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
            ? " "
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
