#!/usr/bin/env bash
# 在服务器上发布一个版本。由本机的 deploy/deploy.sh 上传源码后调用，不要手动在别的目录运行：
#
#   bash deploy/remote/release.sh            # 在 /opt/duolo/releases/<版本> 里运行
#
# 步骤：构建镜像 → 生成 APISIX 路由 → 先换备用实例、再换主实例（APISIX 在主重启时把请求切到备用）
# → 再重启一次备用，让切过去的连接回到主 → 经过 APISIX 检查 /health → 切换 current。
# 任何一步失败都换回上一个版本。
#
# 用到的环境变量（deploy.sh 传过来）：DUOLO_DOMAIN、DUOLO_APISIX_PORT、DUOLO_UPSTREAM_NODES。
set -euo pipefail

# 服务器上就是 /opt/duolo；在本机试整个流程时可以换成别的目录。
root=${DUOLO_ROOT:-/opt/duolo}
release_dir=$(pwd -P)
release=$(basename "$release_dir")
domain=${DUOLO_DOMAIN:?set DUOLO_DOMAIN}
apisix_port=${DUOLO_APISIX_PORT:-9480}
# 「主机:端口:优先级」，空格分开。优先级高的平时用，低的是备用（见 apisix/apisix.yaml.tmpl）。
nodes=${DUOLO_UPSTREAM_NODES:-server-1:8787:1 server-2:8787:0}

[[ $release_dir == "$root/releases/"* ]] || { echo "run this inside $root/releases/<release>"; exit 2; }
test -f "$root/shared/.env.production" || { echo "missing $root/shared/.env.production"; exit 2; }

export DUOLO_RELEASE=$release DUOLO_APISIX_PORT=$apisix_port DUOLO_ROOT=$root
compose=(docker compose -p duolo -f "$release_dir/deploy/compose.yaml")
previous=$(basename "$(readlink -f "$root/current" 2>/dev/null || true)")
[[ $previous == "$release" || $previous == current ]] && previous=""

log() { printf '[duolo %s] %s\n' "$(date +%H:%M:%S)" "$*"; }

# 生成 APISIX 的路由：原地改写（不换 inode），挂进容器的文件才会跟着变。
render_apisix() {
  local lines="" host port priority
  for node in $nodes; do
    IFS=: read -r host port priority <<<"$node"
    lines+="      - host: ${host}"$'\n'"        port: ${port}"$'\n'"        weight: 1"$'\n'"        priority: ${priority:-0}"$'\n'
  done
  local rendered="" line
  while IFS= read -r line; do
    if [[ $line == "__NODES__" ]]; then rendered+=$lines; else rendered+="${line//__HOST__/$domain}"$'\n'; fi
  done <"$release_dir/deploy/apisix/apisix.yaml.tmpl"
  touch "$root/shared/apisix.yaml"
  printf '%s' "$rendered" >"$root/shared/apisix.yaml"
}

# 等容器的健康检查通过。
wait_healthy() {
  local service=$1 id status
  for _ in $(seq 1 60); do
    id=$("${compose[@]}" ps -q "$service")
    status=$(docker inspect --format '{{.State.Health.Status}}' "$id" 2>/dev/null || echo missing)
    [[ $status == healthy ]] && return 0
    sleep 2
  done
  log "$service is not healthy (last status: $status)"
  "${compose[@]}" logs --tail 40 "$service" || true
  return 1
}

# 经过 APISIX（和 Nginx 转过来的一样带着域名）检查服务端。
check_gateway() {
  for _ in $(seq 1 30); do
    curl -fsS --max-time 5 -o /dev/null -H "Host: $domain" "http://127.0.0.1:$apisix_port/health" 2>/dev/null && return 0
    sleep 2
  done
  log "the gateway does not reach a healthy server"
  return 1
}

rollback() {
  log "release $release failed"
  if [[ -n $previous ]]; then
    log "switching back to $previous"
    DUOLO_RELEASE=$previous "${compose[@]}" up -d --no-deps server-1 server-2 || true
  fi
}

log "building duolo-server:$release"
docker build -q -f "$release_dir/server/Dockerfile" -t "duolo-server:$release" "$release_dir" >/dev/null

trap rollback ERR
render_apisix
"${compose[@]}" up -d --no-deps apisix
log "updating the standby"
"${compose[@]}" up -d --no-deps server-2
wait_healthy server-2
log "updating the primary (requests go to the standby meanwhile)"
"${compose[@]}" up -d --no-deps server-1
wait_healthy server-1
# 主重启时切到备用的连接还连着备用：重启备用，让它们重连到主。
log "moving connections back to the primary"
"${compose[@]}" restart server-2
wait_healthy server-2
check_gateway
trap - ERR

ln -sfn "$release_dir" "$root/current.next" && mv -Tf "$root/current.next" "$root/current"
log "released $release"

# 只留最近 3 个版本的源码和镜像。
cd "$root/releases"
for old in $(ls -1t | tail -n +4); do
  rm -rf "${root:?}/releases/$old"
  docker image rm "duolo-server:$old" >/dev/null 2>&1 || true
done
