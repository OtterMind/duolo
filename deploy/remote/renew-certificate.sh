#!/usr/bin/env bash
# 续期 Duolo 的所有证书，有证书换了才 reload Nginx。由 setup.sh 装成 /usr/local/sbin/duolo-renew-certificate，
# cron 每天跑两次。
set -euo pipefail
umask 077

dir=/opt/duolo/shared/letsencrypt
test -d "$dir/live" || exit 0
fingerprint() { cat "$dir"/live/*/fullchain.pem 2>/dev/null | sha256sum; }

before=$(fingerprint)
docker run --rm \
  -v "$dir:/etc/letsencrypt" \
  -v /opt/duolo/shared/acme:/var/www/acme \
  certbot/certbot@sha256:f70ad0adbb7e117f0fe42a63c553f28ea451edabc0148757b6efcd9735acaa20 \
  renew --webroot --webroot-path /var/www/acme --quiet
if [[ $before != "$(fingerprint)" ]]; then
  nginx -t
  systemctl reload nginx
fi
