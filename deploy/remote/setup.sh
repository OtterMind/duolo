#!/usr/bin/env bash
# 在服务器上准备 Duolo：目录、宿主机 Nginx 的站点、HTTPS 证书和自动续期。由本机的 deploy/deploy.sh setup
# 上传后用 root 运行，可以重复运行（已有的证书不重新申请）。
#
# 服务器上已经有别的站点在用 Nginx：只新增 /etc/nginx/conf.d/duolo.conf 和 duolo-realip.conf，
# 改之前先备份，nginx -t 不通过就换回原来的，不影响别的站点。
#
# 用到的环境变量（deploy.sh 传过来）：
#   DUOLO_DOMAIN            主域名，例如 duolo.io（www.<主域名> 跳到它）
#   DUOLO_REDIRECT_DOMAINS  跳到主域名的其他域名，空格分开，例如 "duolo.cc"（www.<域名> 也跳）
#   DUOLO_APISIX_PORT       APISIX 在本机监听的端口
#   DUOLO_CERT_EMAIL        Let's Encrypt 的联系邮箱
#   DUOLO_CLOUDFLARE        设为 1 时信任 Cloudflare 给的客户端地址（域名在 Cloudflare 开了代理）
set -euo pipefail

root=/opt/duolo
here=$(cd "$(dirname "$0")" && pwd -P)
domain=${DUOLO_DOMAIN:?set DUOLO_DOMAIN}
redirects=${DUOLO_REDIRECT_DOMAINS:-}
apisix_port=${DUOLO_APISIX_PORT:-9480}
email=${DUOLO_CERT_EMAIL:?set DUOLO_CERT_EMAIL}
cloudflare=${DUOLO_CLOUDFLARE:-0}
site=/etc/nginx/conf.d/duolo.conf
realip=/etc/nginx/duolo-realip.conf
certbot_image=certbot/certbot@sha256:f70ad0adbb7e117f0fe42a63c553f28ea451edabc0148757b6efcd9735acaa20

log() { printf '[duolo %s] %s\n' "$(date +%H:%M:%S)" "$*"; }

for tool in docker nginx curl; do
  command -v "$tool" >/dev/null || { log "missing $tool on the server"; exit 2; }
done
docker compose version >/dev/null || { log "missing docker compose"; exit 2; }

install -d -m 755 "$root" "$root/releases" "$root/shared" "$root/shared/acme"
install -d -m 700 "$root/shared/letsencrypt"
test -f "$root/shared/.env.production" || { log "missing $root/shared/.env.production"; exit 2; }

has_cert() { test -f "$root/shared/letsencrypt/live/$1/fullchain.pem"; }

# 证书的路径和 TLS 设置。
tls() {
  cat <<EOF
    ssl_certificate $root/shared/letsencrypt/live/$1/fullchain.pem;
    ssl_certificate_key $root/shared/letsencrypt/live/$1/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
EOF
}

acme() {
  cat <<EOF
    location ^~ /.well-known/acme-challenge/ {
        root $root/shared/acme;
    }
EOF
}

# 转给 APISIX：带上 WebSocket 升级，长连接一小时没有数据才断。
proxy() {
  cat <<EOF
    location / {
        proxy_pass http://127.0.0.1:$apisix_port;
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header Upgrade \$http_upgrade;
        proxy_set_header Connection \$duolo_connection_upgrade;
        proxy_set_header X-Real-IP \$remote_addr;
        proxy_set_header X-Forwarded-For \$remote_addr;
        proxy_set_header X-Forwarded-Proto \$scheme;
        proxy_buffering off;
        proxy_read_timeout 3600s;
        proxy_send_timeout 3600s;
    }
EOF
}

# 按现在有哪些证书生成站点：有证书的域名走 HTTPS，还没有的只开 80 端口（申请证书用）。
render_site() {
  local realip_line=""
  [[ $cloudflare == 1 ]] && realip_line="    include $realip;"
  cat <<EOF
# 由 Duolo 的 deploy/remote/setup.sh 生成，不要手改：重新运行 deploy.sh setup 会覆盖。
map \$http_upgrade \$duolo_connection_upgrade {
    default upgrade;
    ''      close;
}

server {
    listen 80;
    server_name $domain www.$domain;
$realip_line
    client_max_body_size 2m;
$(acme)
EOF
  if has_cert "$domain"; then
    printf '    location / {\n        return 301 https://%s$request_uri;\n    }\n}\n' "$domain"
  else
    printf '%s\n}\n' "$(proxy)"
  fi
  if has_cert "$domain"; then
    cat <<EOF

server {
    listen 443 ssl;
    server_name $domain;
$realip_line
$(tls "$domain")
    client_max_body_size 2m;
    access_log /var/log/nginx/duolo.access.log;
    error_log /var/log/nginx/duolo.error.log;
$(acme)
$(proxy)
}

server {
    listen 443 ssl;
    server_name www.$domain;
$(tls "$domain")
    return 301 https://$domain\$request_uri;
}
EOF
  fi
  local other
  for other in $redirects; do
    cat <<EOF

server {
    listen 80;
    server_name $other www.$other;
$(acme)
    location / {
        return 301 https://$domain\$request_uri;
    }
}
EOF
    if has_cert "$other"; then
      cat <<EOF

server {
    listen 443 ssl;
    server_name $other www.$other;
$(tls "$other")
    return 301 https://$domain\$request_uri;
}
EOF
    fi
  done
}

# 写进 Nginx：先备份，检查不通过就换回去。
install_site() {
  local backup="" tmp
  tmp=$(mktemp)
  render_site >"$tmp"
  if [[ -f $site ]]; then
    cmp -s "$tmp" "$site" && { rm -f "$tmp"; return 0; }
    backup=$(mktemp)
    cp -p "$site" "$backup"
  fi
  install -m 644 "$tmp" "$site"
  rm -f "$tmp"
  if ! nginx -t 2>&1; then
    log "nginx -t failed, restoring the previous $site"
    if [[ -n $backup ]]; then cp -p "$backup" "$site"; else rm -f "$site"; fi
    rm -f "$backup"
    return 1
  fi
  rm -f "$backup"
  systemctl reload nginx
  log "nginx reloaded ($site)"
}

# 申请一张证书（域名和 www）。DNS 还没生效时只提示，不影响别的域名。
issue_cert() {
  local name=$1
  has_cert "$name" && return 0
  log "requesting a certificate for $name and www.$name"
  if ! docker run --rm \
    -v "$root/shared/letsencrypt:/etc/letsencrypt" \
    -v "$root/shared/acme:/var/www/acme" \
    "$certbot_image" certonly --webroot --webroot-path /var/www/acme \
    --non-interactive --agree-tos --email "$email" --cert-name "$name" \
    -d "$name" -d "www.$name"; then
    log "no certificate for $name yet: check that $name and www.$name point to this server, then run setup again"
  fi
}

if [[ $cloudflare == 1 ]]; then
  install -m 644 "$here/../nginx/cloudflare-realip.conf" "$realip"
fi

install_site
for name in "$domain" $redirects; do issue_cert "$name"; done
install_site

# 证书自动续期：每天两次，换了证书才 reload Nginx。
install -m 755 "$here/renew-certificate.sh" /usr/local/sbin/duolo-renew-certificate
printf '%s\n' "# Duolo 的证书续期，由 deploy/remote/setup.sh 安装。" \
  "17 3,15 * * * root /usr/local/sbin/duolo-renew-certificate >>/var/log/duolo-renew.log 2>&1" \
  >/etc/cron.d/duolo-certificate
chmod 644 /etc/cron.d/duolo-certificate

for name in "$domain" $redirects; do
  if has_cert "$name"; then log "https://$name ready"; else log "https://$name not ready (no certificate)"; fi
done
