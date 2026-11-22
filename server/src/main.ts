// 启动服务：读配置 → 连数据库并建表 → 监听端口。

import { authOptions, createAuth } from "./auth.js";
import { buildApp } from "./app.js";
import { loadConfig } from "./config.js";
import { createPool, migrate } from "./db.js";
import { ConsoleMailer } from "./mailer.js";

const config = loadConfig();
const pool = createPool(config.databaseUrl);
const mailer = new ConsoleMailer();
await migrate(pool, authOptions(config, pool, mailer));
const app = await buildApp({ config, pool, auth: createAuth(config, pool, mailer) });
await app.listen({ port: config.port, host: config.host });

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    void app.close().then(() => pool.end()).then(() => process.exit(0));
  });
}
