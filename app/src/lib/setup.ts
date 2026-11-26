// 一键安装 Agent、登录账号：哪里都能发起（Agent 列表、新建任务面板、设置里的账号），由 App 显示同一个窗口。

/** 要装好的 Agent；`account` 是要登录的账号（`new` 是新添加一个，见 `api.setupStart`）。 */
export interface SetupRequest {
  agent: string;
  account?: string;
}

type Listener = (agent: string) => void;

let request: ((r: SetupRequest) => void) | null = null;
const ready = new Set<Listener>();

/** 打开安装 / 登录窗口：没装的先装，装好后没登录的接着登录。给了 `account` 时只登录这个账号。 */
export const requestSetup = (agent: string, account?: string) => request?.({ agent, account });

/** App 显示窗口。 */
export function onSetupRequest(listener: (r: SetupRequest) => void) {
  request = listener;
  return () => {
    if (request === listener) request = null;
  };
}

/** 装好、登录好了：例如新建任务面板顺手选上它，设置里的账号列表重新查一遍。 */
export function onSetupReady(listener: Listener) {
  ready.add(listener);
  return () => void ready.delete(listener);
}

export const notifySetupReady = (agent: string) => ready.forEach((l) => l(agent));
