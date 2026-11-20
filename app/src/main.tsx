import React from "react";
import ReactDOM from "react-dom/client";
import "hack-font/build/web/hack.css";

import { invoke } from "@tauri-apps/api/core";

import { App } from "./App";
import { I18nProvider } from "./i18n";
import "./styles.css";

// 界面报错转给后端写日志。
const report = (message: string) => void invoke("log", { message }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${String(e.reason)}`));

// 终端按字形测量格子大小，字体要先加载好再创建终端。
await Promise.all(["13px Hack", "bold 13px Hack"].map((font) => document.fonts.load(font))).catch(() => {});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
    </I18nProvider>
  </React.StrictMode>,
);
