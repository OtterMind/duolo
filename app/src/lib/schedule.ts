// 定时任务的时间表：界面上用几种常用的写法（每天、工作日、每周、每小时），存的是 5 段 cron（分 时 日 月 星期），
// 按这台电脑的时区。认不出来的 cron 当作"自定义"，原样显示。

import type { Translate } from "../i18n";

export type Preset =
  | { kind: "daily"; hour: number; minute: number }
  | { kind: "weekdays"; hour: number; minute: number }
  | { kind: "weekly"; day: number; hour: number; minute: number }
  | { kind: "hourly"; minute: number }
  | { kind: "custom"; cron: string };

export const PRESET_KINDS = ["daily", "weekdays", "weekly", "hourly", "custom"] as const;

export function toCron(p: Preset): string {
  switch (p.kind) {
    case "daily":
      return `${p.minute} ${p.hour} * * *`;
    case "weekdays":
      return `${p.minute} ${p.hour} * * 1-5`;
    case "weekly":
      return `${p.minute} ${p.hour} * * ${p.day}`;
    case "hourly":
      return `${p.minute} * * * *`;
    case "custom":
      return p.cron.trim();
  }
}

const NUM = "(\\d{1,2})";

export function fromCron(cron: string): Preset {
  const c = cron.trim().replace(/\s+/g, " ");
  const num = (s: string, max: number) => {
    const n = Number(s);
    return Number.isInteger(n) && n >= 0 && n <= max ? n : null;
  };
  let m = new RegExp(`^${NUM} ${NUM} \\* \\* (\\*|1-5|[0-7])$`).exec(c);
  if (m) {
    const minute = num(m[1], 59);
    const hour = num(m[2], 23);
    if (minute != null && hour != null) {
      if (m[3] === "*") return { kind: "daily", hour, minute };
      if (m[3] === "1-5") return { kind: "weekdays", hour, minute };
      return { kind: "weekly", day: Number(m[3]) % 7, hour, minute };
    }
  }
  m = new RegExp(`^${NUM} \\* \\* \\* \\*$`).exec(c);
  const minute = m ? num(m[1], 59) : null;
  if (minute != null) return { kind: "hourly", minute };
  return { kind: "custom", cron: c };
}

/** 换一种写法时尽量保留已经选的时间。 */
export function switchPreset(p: Preset, kind: Preset["kind"]): Preset {
  const hour = "hour" in p ? p.hour : 9;
  const minute = "minute" in p ? p.minute : 0;
  switch (kind) {
    case "daily":
    case "weekdays":
      return { kind, hour, minute };
    case "weekly":
      return { kind, day: "day" in p ? p.day : 1, hour, minute };
    case "hourly":
      return { kind, minute };
    case "custom":
      return { kind, cron: toCron(p) };
  }
}

/** 几点几分，按界面语言的习惯写。 */
export function clockTime(locale: string, hour: number, minute: number): string {
  return new Date(2000, 0, 1, hour, minute).toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
}

/** 星期几（0 是星期日）。 */
export function weekdayName(locale: string, day: number): string {
  // 2023-01-01 是星期日。
  return new Date(2023, 0, 1 + day).toLocaleDateString(locale, { weekday: "long" });
}

/** 时间表的说明，例如"工作日 09:00"。 */
export function describeSchedule(t: Translate, locale: string, cron: string): string {
  const p = fromCron(cron);
  switch (p.kind) {
    case "daily":
      return t("schedule.dailyAt", { time: clockTime(locale, p.hour, p.minute) });
    case "weekdays":
      return t("schedule.weekdaysAt", { time: clockTime(locale, p.hour, p.minute) });
    case "weekly":
      return t("schedule.weeklyAt", { day: weekdayName(locale, p.day), time: clockTime(locale, p.hour, p.minute) });
    case "hourly":
      return t("schedule.hourlyAt", { minute: String(p.minute).padStart(2, "0") });
    case "custom":
      return p.cron;
  }
}

/** 某个时间点（Unix 秒），例如"11月27日 周五 09:00"。 */
export function formatWhen(locale: string, secs: number): string {
  return new Date(secs * 1000).toLocaleString(locale, {
    month: "short",
    day: "numeric",
    weekday: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}
