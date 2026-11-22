// 添加项目的几种方式，"添加项目"弹窗和首次打开页共用：
// 打开一个现有文件夹（不要求是 git 仓库）、在 ~/DuoloProjects 里新建文件夹、从 GitHub 克隆，
// 或者点这台 Mac 上找到的仓库。

import { useCallback, useEffect, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import { api, type Repo } from "./api";
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
}

export function useProjectAdder(onAdded: () => Promise<unknown>, onError: (err: unknown) => void): ProjectAdder {
  const t = useT();
  const [repos, setRepos] = useState<Repo[]>([]);
  const [busy, setBusy] = useState(false);

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
        onError(err);
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
  const clone = useCallback((url: string) => run(() => api.cloneProject(url)), [run]);

  return { repos, busy, add, choose, create, clone };
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
