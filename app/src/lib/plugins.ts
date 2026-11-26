// Agent 插件：名字和图标。插件在应用启动时加载，运行中不会变，所以只取一次。

import { api, type AgentPlugin } from "./api";

const plugins = new Map<string, AgentPlugin>();

/** 取插件列表，界面显示之前调用。取不到时图标显示首字母，不影响使用。 */
export async function loadPlugins() {
  for (const plugin of await api.agentPlugins()) plugins.set(plugin.id, plugin);
}

export const pluginOf = (id: string) => plugins.get(id);

/** 所有插件。 */
export const allPlugins = () => [...plugins.values()];

/** 显示名；插件已经不在了时用 id，首字母大写。 */
export const pluginName = (id: string) => plugins.get(id)?.name ?? id.charAt(0).toUpperCase() + id.slice(1);
