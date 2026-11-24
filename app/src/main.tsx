import React from "react";
import ReactDOM from "react-dom/client";
// 字体参考 Warp：界面用 Roboto，终端和代码用 Hack，都随应用打包，三个系统看起来一样。
import "@fontsource-variable/roboto";
import "hack-font/build/web/hack.css";

import { invoke } from "@tauri-apps/api/core";

import { App } from "./App";
import { I18nProvider } from "./i18n";
import { isMac } from "./lib/platform";
import { loadPlugins } from "./lib/plugins";
import "./styles.css";

// 样式里按系统区分的地方（标题栏留白等）用 .mac / .pc。
document.documentElement.classList.add(isMac ? "mac" : "pc");

// 界面报错转给后端写日志。
const report = (message: string) => void invoke("log", { message }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${String(e.reason)}`));

// 终端按字形测量格子大小，字体要先加载好再创建终端。Agent 的图标也先取好，免得闪一下首字母。
await Promise.allSettled([
  ...["13px Hack", "bold 13px Hack"].map((font) => document.fonts.load(font)),
  loadPlugins(),
]);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>,
);
