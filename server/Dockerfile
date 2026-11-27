# Duolo 服务端的生产镜像。在仓库根目录构建（要带上根目录的 locales/）：
#   docker build -f server/Dockerfile -t duolo-server .
# 第一阶段装全部依赖、编译 TypeScript；第二阶段只带运行需要的依赖、编译结果、文案和网页（site/）。

FROM node:22.22.1-bookworm-slim AS build
WORKDIR /app
# 和本地、CI 用同一个大版本的 pnpm。
RUN corepack enable && corepack prepare pnpm@10.33.0 --activate
COPY server/package.json server/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY server/tsconfig.json ./
COPY server/src ./src
RUN pnpm build && pnpm prune --prod

FROM node:22.22.1-bookworm-slim
WORKDIR /app
ENV NODE_ENV=production PORT=8787 HOST=0.0.0.0 DUOLO_LOCALES_DIR=/app/locales DUOLO_SITE_DIR=/app/site
COPY --from=build /app/package.json ./
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/dist ./dist
COPY locales ./locales
COPY server/site ./site
USER node
EXPOSE 8787
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=3 \
  CMD node -e "fetch('http://127.0.0.1:8787/health').then(r=>process.exit(r.ok?0:1)).catch(()=>process.exit(1))"
CMD ["node", "--no-warnings", "dist/main.js"]
