// 可选的 Agent（谁写、谁审）和默认选择。
//
// 选项来自环境检查：每一种 Agent 插件都列出来，本机没装的标成"未安装"并给出安装命令。
// 默认值：上次的选择 → config.toml 的默认 → 任意一个装了的；审阅优先选和写代码不同的 Agent。

import type { Check } from "./api";
import { pluginOf } from "./plugins";

export interface AgentOption {
  id: string;
  name: string;
  installed: boolean;
  /** 能不能用来审阅。 */
  reviews: boolean;
  /** 安装命令，没装时显示给用户。 */
  install: string;
  /** 装了但太旧时：装的版本和最低要求。 */
  tooOld: { version: string; minimum: string } | null;
}

export interface AgentChoice {
  writer: string;
  reviewer: string;
}

const LAST_KEY = "duolo.lastAgents";

export function agentOptions(env: Check[] | null): AgentOption[] {
  return (env ?? [])
    .filter((c) => c.kind === "agent" && c.agent)
    .map((c) => ({
      id: c.agent,
      name: c.name,
      installed: c.ok,
      reviews: pluginOf(c.agent)?.reviews ?? true,
      install: c.install,
      tooOld: c.minimum ? { version: c.version, minimum: c.minimum } : null,
    }));
}

/** 本机一个 Agent 都没装（环境检查还没结束时不算）。 */
export const noAgentInstalled = (env: Check[] | null) =>
  env !== null && agentOptions(env).every((a) => !a.installed);

export function isInstalled(env: Check[] | null, id: string) {
  const option = agentOptions(env).find((a) => a.id === id);
  // 环境检查还没结果时先当作可用，避免按钮闪一下变灰。
  return env === null || option?.installed === true;
}

export function defaultChoice(env: Check[] | null, configured: AgentChoice | null): AgentChoice {
  const options = agentOptions(env);
  const installed = options.filter((a) => a.installed).map((a) => a.id);
  const usable = (id: string | undefined) => !!id && (env === null || installed.includes(id));
  const last = loadLast();
  const writer = [last?.writer, configured?.writer, installed[0], options[0]?.id].find(usable) ?? configured?.writer ?? "";
  const reviewers = options.filter((a) => a.installed && a.reviews).map((a) => a.id);
  const canReview = (id: string | undefined) => !!id && (env === null || reviewers.includes(id));
  const others = reviewers.filter((id) => id !== writer);
  const reviewer =
    [last?.reviewer, configured?.reviewer, others[0], writer].find(canReview) ?? configured?.reviewer ?? "";
  return { writer, reviewer };
}

export function rememberChoice(choice: AgentChoice) {
  try {
    localStorage.setItem(LAST_KEY, JSON.stringify(choice));
  } catch {
    // 记不住就下次用默认。
  }
}

function loadLast(): AgentChoice | null {
  try {
    const raw = localStorage.getItem(LAST_KEY);
    return raw ? (JSON.parse(raw) as AgentChoice) : null;
  } catch {
    return null;
  }
}
