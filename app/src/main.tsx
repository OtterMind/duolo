import React from "react";
import ReactDOM from "react-dom/client";

import { invoke } from "@tauri-apps/api/core";

import { App } from "./App";
import "./styles.css";

// 界面报错转给后端写日志。
const report = (message: string) => void invoke("log", { message }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${String(e.reason)}`));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
