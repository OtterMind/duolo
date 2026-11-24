// 登录：用开源的 better-auth。
//
// - 邮箱验证码登录（email-otp 插件）：输入邮箱 → 收到 6 位验证码 → 登录，第一次登录自动注册。
// - 令牌登录（bearer 插件）：电脑端和手机 App 不用 Cookie，请求时带 `Authorization: Bearer <令牌>`。
// - 匿名登录（anonymous 插件）：电脑端不用注册也能用语音识别等服务，后台自动领一个匿名身份，
//   服务器按它限额；以后用邮箱登录时合并成正式账号。
// - 每台设备一个会话；手机扫码配对时由服务器直接给同一个用户建会话（见 pairing.ts）。
// - 以后加 Apple / Google / GitHub 登录，只需要在这里加 socialProviders。
//
// 用户、会话、验证码这几张表由 better-auth 管理，迁移见 db.ts。

import { betterAuth, type BetterAuthOptions } from "better-auth";
import { anonymous, bearer, emailOTP } from "better-auth/plugins";
import type { Pool } from "pg";

import type { Config } from "./config.js";
import { matchLocale, t } from "./i18n.js";
import type { Mailer } from "./mailer.js";

/** 验证码有效期（秒）。 */
const OTP_TTL_SECONDS = 10 * 60;

export function authOptions(config: Config, pool: Pool, mailer: Mailer): BetterAuthOptions {
  return {
    database: pool,
    secret: config.authSecret,
    baseURL: config.publicUrl,
    basePath: "/v1/auth",
    // 设备会话长期有效，用一次就顺延；在别的设备上可以单独解除。
    session: { expiresIn: 60 * 60 * 24 * 90, updateAge: 60 * 60 * 24 },
    plugins: [
      emailOTP({
        otpLength: 6,
        expiresIn: OTP_TTL_SECONDS,
        allowedAttempts: 5,
        storeOTP: "hashed",
        async sendVerificationOTP({ email, otp }, ctx) {
          // 邮件按请求里的 Accept-Language 选语言。
          const locale = matchLocale(ctx?.headers?.get("accept-language") ?? ctx?.request?.headers.get("accept-language"));
          const minutes = OTP_TTL_SECONDS / 60;
          // 不等邮件发完再返回，避免从耗时上看出邮箱是否存在。
          void mailer
            .send({
              to: email,
              subject: t(locale, "email.codeSubject", { code: otp }),
              text: t(locale, "email.codeBody", { code: otp, minutes }),
            })
            .catch((err) => console.error("[mail] send failed", err));
        },
      }),
      bearer(),
      anonymous(),
    ],
  };
}

export function createAuth(config: Config, pool: Pool, mailer: Mailer) {
  return betterAuth(authOptions(config, pool, mailer));
}

export type Auth = ReturnType<typeof createAuth>;
