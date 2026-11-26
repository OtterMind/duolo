// 添加项目的几种方式，"添加项目"弹窗和首次打开页共用：
// 打开一个现有文件夹（不要求是 git 仓库）、在 ~/DuoloProjects 里新建文件夹、从 GitHub 克隆，
// 或者点这台 Mac 上找到的仓库。

import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, type CloneProgress, type Repo } from "./api";
import { useT } from "../i18n";

export interface ProjectAdder {
  /** 常见代码目录里找到、还没添加的仓库。 */
  repos: Repo[];
  busy: boolean;
  /** 添加 `path` 指向的文件夹，成功时返回项目 id（出错时已经提示过，返回 null）。 */
  add(path: string): Promise<string | null>;
  /** 弹出选文件夹的对话框再添加。 */
  choose(): Promise<string | null>;
  /** 新建文件夹 `name`（放在 `parent`，默认 ~/DuoloProjects）并加为项目。 */
  create(name: string, parent: string | null): Promise<string | null>;
  clone(url: string): Promise<string | null>;
  /** 正在克隆时的进度，不在克隆时为 null。 */
  cloning: CloneProgress | null;
  /** 取消正在进行的克隆。 */
  cancelClone(): void;
}

/** 克隆的每一步在整个进度条上占多少：下载占大头。 */
const PHASE_SPAN: Record<CloneProgress["phase"], [number, number]> = {
  connecting: [0, 0],
  preparing: [0, 5],
  receiving: [5, 85],
  resolving: [85, 95],
  checkingOut: [95, 100],
};

/** 整个克隆完成了百分之多少。 */
export function cloneOverall(p: CloneProgress): number {
  const [from, to] = PHASE_SPAN[p.phase];
  return Math.round(from + ((to - from) * (p.percent ?? 0)) / 100);
}

export function useProjectAdder(onAdded: () => Promise<unknown>, onError: (err: unknown) => void): ProjectAdder {
  const t = useT();
  const [repos, setRepos] = useState<Repo[]>([]);
  const [busy, setBusy] = useState(false);
  const [cloning, setCloning] = useState<CloneProgress | null>(null);

  const loadRepos = useCallback(() => api.discoverRepos().then(setRepos, () => setRepos([])), []);
  useEffect(() => void loadRepos(), [loadRepos]);

  const run = useCallback(
    async (job: () => Promise<string>): Promise<string | null> => {
      setBusy(true);
      try {
        const id = await job();
        await onAdded();
        void loadRepos();
        return id;
      } catch (err) {
        // 自己点的取消不算出错。
        if ((err as { code?: string } | null)?.code !== "cancelled") onError(err);
        return null;
      } finally {
        setBusy(false);
      }
    },
    [onAdded, onError, loadRepos],
  );

  const add = useCallback((path: string) => run(() => api.addProject(path)), [run]);
  const choose = useCallback(async () => {
    const chosen = await openDialog({ directory: true, title: t("addProject.open") });
    return typeof chosen === "string" ? add(chosen) : null;
  }, [add, t]);
  const create = useCallback(
    (name: string, parent: string | null) => run(() => api.createProjectFolder(name, parent)),
    [run],
  );
  const clone = useCallback(
    (url: string) => {
      setCloning({ phase: "connecting", percent: null, detail: "" });
      return run(() => api.cloneProject(url, setCloning)).finally(() => setCloning(null));
    },
    [run],
  );
  const cancelClone = useCallback(() => void api.cancelClone(), []);

  return { repos, busy, add, choose, create, clone, cloning, cancelClone };
}

/** 新建文件夹和克隆默认放在哪里（完整路径）。 */
export function useProjectsHome() {
  const [dir, setDir] = useState("");
  useEffect(() => {
    api.projectsHome().then(setDir, () => {});
  }, []);
  return dir;
}

/** 把家目录写成 `~`。 */
export const tildify = (path: string) => path.replace(/^\/Users\/[^/]+/, "~");
