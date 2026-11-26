# 部署服务端

把 `server/` 发布到一台已经有别的站点的 Linux 服务器上，不占用它们的端口，也不改它们的 Nginx 配置。

```
用户 ──443──▶ 宿主机 Nginx（证书、域名跳转）──▶ 127.0.0.1:9480 APISIX ──▶ server-1（主）
                                                                    └─▶ server-2（备）
```

- **宿主机 Nginx** 管 80/443 和证书。Duolo 只新增 `/etc/nginx/conf.d/duolo.conf`，每次改之前先备份，`nginx -t` 不通过就换回去。
- **APISIX**（standalone 模式，不用 etcd）只监听本机端口，按域名路由，对上游做健康检查。
- **两个服务端实例，主备**。服务端现在只支持单实例：在线设备的连接在进程内存里，同一个账号的设备必须连到同一台。
  所以平时只用主，主重启或不健康时 APISIX 自动切到备，发布时轮流重启，不断线。服务端支持跨实例转发之后，
  把 `DUOLO_UPSTREAM_NODES` 里的优先级设成一样，就是多台同时分流，也可以加别的服务器上的节点。
- 所有文件在服务器的 `/opt/duolo`：`releases/<版本>` 是每次发布的源码（留最近 3 个），`current` 指向正在用的版本，
  `shared/` 放环境变量、证书和 APISIX 的路由。

## 本机的配置（不进仓库）

服务器地址、密钥只放在本机的两个文件里，都在 `.gitignore` 里：

| 文件 | 内容 |
| --- | --- |
| `deploy/.env.deploy` | SSH 地址、域名、证书邮箱、APISIX 端口。模板：`.env.deploy.example` |
| `deploy/.env.production` | 服务端的环境变量（数据库、登录密钥、豆包等），上传到服务器。模板：`.env.production.example` |

## 第一次部署

服务器要有 Docker（带 compose 插件）和 Nginx，并且 `nginx.conf` 引入了 `conf.d/*.conf`。登录用户是 root，
或者能免密码 `sudo`。

```sh
deploy/deploy.sh init       # 生成两个配置文件，再填 DUOLO_SSH 等
deploy/deploy.sh check      # 检查 SSH、Docker、Nginx、端口和域名解析
deploy/deploy.sh db-init    # 可选：用 DUOLO_DB_ADMIN_URL 在托管 PostgreSQL 上建 duolo 库，写进 .env.production
```

在域名服务商的控制台加解析，都指向服务器的公网地址：

| 域名 | 记录 |
| --- | --- |
| `duolo.io`、`www.duolo.io` | A → 服务器地址 |
| `duolo.cc`、`www.duolo.cc` | A → 服务器地址（会 301 跳到 `https://duolo.io`） |

域名在 Cloudflare 时，第一次申请证书前先用「仅 DNS」（灰色云朵）；证书申请好以后可以开代理（橙色云朵），
SSL 模式选「完全（严格）」，并在 `.env.deploy` 里设 `DUOLO_CLOUDFLARE=1`。

```sh
deploy/deploy.sh setup      # 上传环境变量、配置 Nginx 站点、申请证书、装续期任务
deploy/deploy.sh release    # 发布
```

`setup` 可以重复运行：某个域名的解析还没生效、证书没申请到时，等生效了再运行一次就行。证书每天检查两次续期
（`/etc/cron.d/duolo-certificate`），换了证书才 reload Nginx。

## 日常发布

```sh
deploy/deploy.sh release    # 发布当前提交（HEAD），没提交的改动不会发布
deploy/deploy.sh status     # 容器状态和经过网关的 /health
deploy/deploy.sh logs server-1
```

`release` 的步骤（`remote/release.sh`）：构建镜像 → 生成 APISIX 路由 → 先换备、再换主 → 重启一次备，让切过去的
连接回到主 → 经过 APISIX 检查 `/health` → 切换 `current`。任何一步失败都换回上一个版本。

改了 `.env.production` 之后运行 `setup` 上传，再 `release` 一次让服务端用上新的环境变量。
