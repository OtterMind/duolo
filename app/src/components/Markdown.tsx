import type { MouseEvent, ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { api } from "../lib/api";

/**
 * Agent 写的 Markdown：标题、列表、表格、代码块、引用、链接等（GitHub 的写法）。
 * 生成 React 元素，不用原始 HTML：内容里的 HTML 标签只会当作文字显示。
 * 链接用默认浏览器打开，只开 http / https。
 */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={COMPONENTS}>
        {text}
      </ReactMarkdown>
    </div>
  );
}

const COMPONENTS: Components = {
  a: ({ href, children }) => <Link href={href ?? ""}>{children}</Link>,
  // 表格放进能横向滚动的框里，太宽时不撑开整个对话。
  table: ({ children }) => (
    <div className="md-table">
      <table>{children}</table>
    </div>
  ),
};

/** 网址后面紧跟的中文、全角标点（"https://a.com/x，看到"）不算网址：自动识别的链接在这里截断。 */
const NOT_URL = /[\u0080-\uffff]/;

function Link({ href, children }: { href: string; children: ReactNode }) {
  const web = /^https?:\/\//i.test(href);
  if (!web) return <span className="md-link-text">{children}</span>;
  let url = href;
  let rest = "";
  // 只截断自动识别的（显示的文字就是网址本身）；[文字](网址) 写法的照原样。
  if (typeof children === "string" && /^https?:\/\//i.test(children)) {
    const cut = children.search(NOT_URL);
    if (cut > 0) {
      url = children.slice(0, cut);
      rest = children.slice(cut);
    }
  }
  const open = (e: MouseEvent) => {
    e.preventDefault();
    void api.openUrl(url).catch(() => {});
  };
  return (
    <>
      <a href={url} title={url} onClick={open}>
        {rest ? url : children}
      </a>
      {rest}
    </>
  );
}
