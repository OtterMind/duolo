// 发邮件。开发时只把内容打到日志里；上线时换成真正的邮件服务（SMTP、Resend 等），实现同一个接口即可。

export interface Mail {
  to: string;
  subject: string;
  text: string;
}

export interface Mailer {
  send(mail: Mail): Promise<void>;
}

/** 开发用：打印到日志，验证码直接从日志里看。 */
export class ConsoleMailer implements Mailer {
  async send(mail: Mail) {
    console.log(`[mail] to=${mail.to} subject=${JSON.stringify(mail.subject)}\n${mail.text}`);
  }
}

/** 测试用：记下发出的邮件。 */
export class MemoryMailer implements Mailer {
  readonly sent: Mail[] = [];
  async send(mail: Mail) {
    this.sent.push(mail);
  }
}
