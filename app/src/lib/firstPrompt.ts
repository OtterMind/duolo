// 新任务一开始没有需求，Agent 直接打开等用户输入。这里从发给 Agent 的按键里还原出用户第一次提交的
// 那行文字，用来给任务起标题和分支名。只做近似还原：处理退格、粘贴和清行，忽略方向键等控制序列。

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

const buffers = new Map<number, string>();

/** 喂入发给 Agent 的数据；用户按下回车且这一行有内容时返回这一行。 */
export function feed(task: number, data: string): string | null {
  let line = buffers.get(task) ?? "";
  let text = data.split(PASTE_START).join("").split(PASTE_END).join("");
  // 方向键、功能键等转义序列不计入内容。
  text = text.replace(/\x1b\[[0-9;?]*[A-Za-z~]|\x1bO[A-Za-z]|\x1b./g, "");

  for (const ch of text) {
    if (ch === "\r" || ch === "\n") {
      const submitted = line.trim();
      line = "";
      // 斜杠命令（/model 等）不算需求。
      if (submitted && !submitted.startsWith("/")) {
        buffers.delete(task);
        return submitted;
      }
    } else if (ch === "\x7f" || ch === "\b") {
      line = Array.from(line).slice(0, -1).join("");
    } else if (ch === "\x03" || ch === "\x15") {
      line = ""; // ⌃C、⌃U 清空当前行
    } else if (ch >= " ") {
      line += ch;
    }
  }
  buffers.set(task, line);
  return null;
}

export function forget(task: number) {
  buffers.delete(task);
}
