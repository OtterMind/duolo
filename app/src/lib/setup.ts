// 一键安装 Agent、登录账号：哪里都能发起（Agent 列表、新建任务面板），由 App 显示同一个窗口。

type Listener = (agent: string) => void;

let request: Listener | null = null;
const ready = new Set<Listener>();

/** 打开安装 / 登录窗口：没装的先装，装好后没登录的接着登录。 */
export const requestSetup = (agent: string) => request?.(agent);

/** App 显示窗口。 */
export function onSetupRequest(listener: Listener) {
  request = listener;
  return () => {
    if (request === listener) request = null;
  };
}

/** 装好、登录好了：例如新建任务面板顺手选上它。 */
export function onSetupReady(listener: Listener) {
  ready.add(listener);
  return () => void ready.delete(listener);
}

export const notifySetupReady = (agent: string) => ready.forEach((l) => l(agent));
