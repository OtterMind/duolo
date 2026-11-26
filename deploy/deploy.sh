#!/usr/bin/env bash
# 把 Duolo 服务端发布到服务器。在本机运行；服务器地址、密钥都从本机的文件读，不进仓库：
#
#   deploy/.env.deploy       服务器、域名等（从 .env.deploy.example 复制）
#   deploy/.env.production   服务端的环境变量，上传到服务器的 /opt/duolo/shared/.env.production
#
# 用法：
#   deploy/deploy.sh init       生成上面两个文件（随机的登录密钥），再手动填
#   deploy/deploy.sh check      检查 SSH、服务器上的 Docker / Nginx、域名解析
#   deploy/deploy.sh db-init    在托管 PostgreSQL 上建 duolo 用户和数据库，写进 .env.production
#   deploy/deploy.sh setup      上传环境变量，配置 Nginx 站点、申请证书（可重复运行）
#   deploy/deploy.sh release    发布当前提交（HEAD；没提交的改动不会发布）
#   deploy/deploy.sh status     看容器和网关的状态
#   deploy/deploy.sh logs [服务] 看日志（server-1、server-2、apisix）
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd -P)
repo=$(cd "$here/.." && pwd -P)
deploy_env=${DUOLO_DEPLOY_ENV:-$here/.env.deploy}
production_env=${DUOLO_PRODUCTION_ENV:-$here/.env.production}
remote_root=/opt/duolo

log() { printf '[duolo] %s\n' "$*"; }
die() { printf '[duolo] %s\n' "$*" >&2; exit 1; }

init() {
  if [[ -f $deploy_env ]]; then log "$deploy_env already exists"; else
    cp "$here/.env.deploy.example" "$deploy_env" && chmod 600 "$deploy_env"
    log "created $deploy_env: fill in DUOLO_SSH and the rest"
  fi
  if [[ -f $production_env ]]; then log "$production_env already exists"; else
    (umask 077 && sed "s|^BETTER_AUTH_SECRET=.*|BETTER_AUTH_SECRET=$(openssl rand -hex 32)|" \
      "$here/.env.production.example" >"$production_env")
    log "created $production_env with a random BETTER_AUTH_SECRET: set DATABASE_URL (or run db-init)"
  fi
}

load() {
  [[ -f $deploy_env ]] || die "missing $deploy_env (run: deploy/deploy.sh init)"
  set -a
  # shellcheck disable=SC1090
  source "$deploy_env"
  set +a
  : "${DUOLO_SSH:?set DUOLO_SSH in $deploy_env}"
  : "${DUOLO_DOMAIN:?set DUOLO_DOMAIN in $deploy_env}"
  DUOLO_APISIX_PORT=${DUOLO_APISIX_PORT:-9480}
  ssh_cmd=(ssh -o BatchMode=yes -o ConnectTimeout=10 -p "${DUOLO_SSH_PORT:-22}")
  [[ -n ${DUOLO_SSH_KEY:-} ]] && ssh_cmd+=(-i "$DUOLO_SSH_KEY")
  ssh_cmd+=("$DUOLO_SSH")
  # 不是 root 登录时用 sudo（要求免密码）。
  sudo=""
  [[ ${DUOLO_SSH%%@*} == root ]] || sudo="sudo -n"
}

# 传给服务器脚本的变量，值都加引号，防止空格和特殊字符。
remote_env() {
  local name out=""
  for name in DUOLO_DOMAIN DUOLO_REDIRECT_DOMAINS DUOLO_APISIX_PORT DUOLO_UPSTREAM_NODES DUOLO_CERT_EMAIL DUOLO_CLOUDFLARE; do
    [[ -n ${!name:-} ]] && out+=" $name=$(printf '%q' "${!name}")"
  done
  printf '%s' "$out"
}

check() {
  load
  log "ssh $DUOLO_SSH"
  "${ssh_cmd[@]}" "$sudo env $(remote_env) bash -s" <<'EOF'
set -u
ok() { printf '  ok   %s\n' "$*"; }
bad() { printf '  FAIL %s\n' "$*"; fail=1; }
fail=0
command -v docker >/dev/null && ok "$(docker --version)" || bad "docker not installed"
docker compose version >/dev/null 2>&1 && ok "$(docker compose version)" || bad "docker compose not installed"
command -v nginx >/dev/null && ok "$(nginx -v 2>&1)" || bad "nginx not installed"
grep -rqs "conf.d/\*.conf" /etc/nginx/nginx.conf && ok "nginx includes conf.d/*.conf" || bad "nginx.conf does not include /etc/nginx/conf.d/*.conf"
nginx -t >/dev/null 2>&1 && ok "nginx -t" || bad "nginx -t fails before any change"
port=${DUOLO_APISIX_PORT:-9480}
if ss -ltn "sport = :$port" | grep -q LISTEN && ! docker ps --format '{{.Names}}' | grep -q '^duolo-apisix'; then
  bad "port $port is used by something else"
else ok "port $port"; fi
df -h / | awk 'NR==2 { print "  info disk free: " $4 }'
free -m | awk '/Mem:/ { print "  info memory available: " $7 " MB" }'
exit $fail
EOF
  log "DNS (from 1.1.1.1)"
  local name
  for name in "$DUOLO_DOMAIN" "www.$DUOLO_DOMAIN" ${DUOLO_REDIRECT_DOMAINS:-}; do
    printf '  %-20s %s\n' "$name" "$(dig +short @1.1.1.1 "$name" A | tr '\n' ' ')"
    [[ $name == www.* || $name == "$DUOLO_DOMAIN" ]] || printf '  %-20s %s\n' "www.$name" "$(dig +short @1.1.1.1 "www.$name" A | tr '\n' ' ')"
  done
}

# 在托管的 PostgreSQL 上建 duolo 用户和数据库。管理员连接串只在这次用，不存到服务器上。
db_init() {
  load
  : "${DUOLO_DB_ADMIN_URL:?set DUOLO_DB_ADMIN_URL in $deploy_env (an admin connection string)}"
  [[ -f $production_env ]] || die "missing $production_env (run: deploy/deploy.sh init)"
  if grep -q '^DATABASE_URL=.\+' "$production_env"; then die "DATABASE_URL is already set in $production_env"; fi
  local user=${DUOLO_DB_NAME:-duolo} password
  password=$(openssl rand -hex 24)
  # postgres://管理员:密码@主机:端口/库?参数 → postgres://duolo:新密码@主机:端口/duolo?参数
  local rest=${DUOLO_DB_ADMIN_URL#*://}
  local hostpart=${rest#*@}
  local query=""
  [[ $hostpart == *\?* ]] && query="?${hostpart#*\?}"
  hostpart=${hostpart%%/*}
  hostpart=${hostpart%%\?*}
  local url="postgres://$user:$password@$hostpart/$user$query"
  log "creating role and database $user on $hostpart (via the server)"
  # 连接串和密码走标准输入，不出现在服务器的命令行里。
  { printf 'export PGURL=%q\n' "$DUOLO_DB_ADMIN_URL"
    cat <<EOF
docker run --rm -i -e PGURL postgres:17-alpine sh -c 'psql -v ON_ERROR_STOP=1 -q "\$PGURL"' <<'SQL'
DO \$\$ BEGIN
  IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = '$user') THEN
    CREATE ROLE $user LOGIN PASSWORD '$password';
  ELSE
    ALTER ROLE $user LOGIN PASSWORD '$password';
  END IF;
END \$\$;
GRANT $user TO CURRENT_USER;
SELECT 'CREATE DATABASE $user OWNER $user' WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = '$user')\gexec
SQL
EOF
  } | "${ssh_cmd[@]}" "$sudo bash -s"
  printf 'DATABASE_URL=%s\n' "$url" >>"$production_env"
  log "DATABASE_URL written to $production_env"
}

setup() {
  load
  : "${DUOLO_CERT_EMAIL:?set DUOLO_CERT_EMAIL in $deploy_env}"
  [[ -f $production_env ]] || die "missing $production_env (run: deploy/deploy.sh init)"
  grep -q '^DATABASE_URL=.\+' "$production_env" || die "set DATABASE_URL in $production_env (or run db-init)"
  grep -q '^BETTER_AUTH_SECRET=.\{32,\}' "$production_env" || die "set BETTER_AUTH_SECRET (32+ characters) in $production_env"
  log "uploading $remote_root/shared/.env.production"
  "${ssh_cmd[@]}" "$sudo sh -c 'umask 077 && mkdir -p $remote_root/shared && cat > $remote_root/shared/.env.production'" <"$production_env"
  # setup.sh 要用到 deploy/ 下的其他文件：把当前提交的 deploy/ 传到临时目录再运行。
  local dir=/tmp/duolo-setup-$$
  git -C "$repo" archive --format=tar HEAD deploy | "${ssh_cmd[@]}" "mkdir -p $dir && tar -x -C $dir"
  "${ssh_cmd[@]}" "$sudo env $(remote_env) bash $dir/deploy/remote/setup.sh; status=\$?; rm -rf $dir; exit \$status"
}

release() {
  load
  [[ -z $(git -C "$repo" status --porcelain -- server deploy locales) ]] ||
    log "warning: uncommitted changes in server/, deploy/ or locales/ are not deployed"
  local name
  name=$(date -u +%Y%m%d-%H%M%S)-$(git -C "$repo" rev-parse --short HEAD)
  local dir=$remote_root/releases/$name
  log "uploading $name"
  git -C "$repo" archive --format=tar HEAD server deploy locales |
    "${ssh_cmd[@]}" "$sudo sh -c 'mkdir -p $dir && tar -x -C $dir'"
  "${ssh_cmd[@]}" "cd $dir && $sudo env $(remote_env) bash deploy/remote/release.sh"
  log "check: curl https://$DUOLO_DOMAIN/health"
  curl -fsS --max-time 10 "https://$DUOLO_DOMAIN/health" && echo || log "https://$DUOLO_DOMAIN/health is not reachable from here yet"
}

compose_remote() {
  local args
  args=$(printf '%q ' "$@")
  "${ssh_cmd[@]}" "cd $remote_root/current && $sudo env DUOLO_RELEASE=\$(basename \$(readlink -f $remote_root/current)) DUOLO_APISIX_PORT=$DUOLO_APISIX_PORT docker compose -p duolo -f deploy/compose.yaml $args"
}

status() {
  load
  compose_remote ps
  "${ssh_cmd[@]}" "curl -fsS -H 'Host: $DUOLO_DOMAIN' http://127.0.0.1:$DUOLO_APISIX_PORT/health" && echo
}

logs() {
  load
  compose_remote logs --tail 200 "$@"
}

case ${1:-} in
  init) init ;;
  check) check ;;
  db-init) db_init ;;
  setup) setup ;;
  release) release ;;
  status) status ;;
  logs) shift; logs "$@" ;;
  *) sed -n '2,15p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
