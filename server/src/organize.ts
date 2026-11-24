// 把口述原话整理成一条可以直接发给编程 Agent 的指令：去掉口头语和重复，改口只留最后的说法，
// 按热词改正代码里的名字（"use state" → useState）。调用 OpenAI 兼容的聊天接口（豆包、通义、DeepSeek、OpenAI 都支持），
// 模型的地址、密钥、名字都来自环境变量，密钥不进仓库。

export interface OrganizeConfig {
  /** OpenAI 兼容接口的地址，到 `/v1` 为止，例如 `https://ark.cn-beijing.volces.com/api/v3`。 */
  baseUrl: string;
  /** 地址、密钥、模型有一个为空就不整理，客户端拿到的就是原话。 */
  apiKey: string;
  model: string;
  /** 最多等多久，超时就用原话。 */
  timeoutMs: number;
}

export interface OrganizeInput {
  text: string;
  hotwords: string[];
  /** 最近的对话，新的在前。 */
  context: string[];
}

const SYSTEM = `You clean up voice transcripts. The user message contains what a user said to a coding agent, from speech recognition; \
it may contain filler words, repetitions, self-corrections or recognition errors.
Rewrite it as one instruction that can be sent to the coding agent directly:
- remove filler words and repetitions;
- when the user corrects themselves, keep only the final version;
- fix obvious recognition errors; when the speech refers to one of the listed names, use its exact spelling (for example "use state" -> useState);
- do not add requirements the user did not state; do not answer or explain;
- write in the same language the user spoke;
- output only the instruction itself, without quotes or prefixes.`;

/** 给模型的用户消息：名字、最近的对话、原话。 */
export function organizePrompt({ text, hotwords, context }: OrganizeInput): string {
  const parts: string[] = [];
  if (hotwords.length) parts.push(`Names in this project:\n${hotwords.slice(0, 40).join(", ")}`);
  if (context.length) parts.push(`Recent conversation (newest first):\n${context.slice(0, 6).map((c) => `- ${c}`).join("\n")}`);
  parts.push(`Transcript:\n${text}`);
  return parts.join("\n\n");
}

/** 整理；没配置、超时、出错或结果为空时返回 undefined（用原话）。 */
export async function organize(config: OrganizeConfig, input: OrganizeInput): Promise<string | undefined> {
  if (!config.baseUrl || !config.apiKey || !config.model || !input.text.trim()) return undefined;
  try {
    const res = await fetch(`${config.baseUrl.replace(/\/+$/, "")}/chat/completions`, {
      method: "POST",
      headers: { "content-type": "application/json", authorization: `Bearer ${config.apiKey}` },
      body: JSON.stringify({
        model: config.model,
        temperature: 0,
        max_tokens: Math.max(256, input.text.length * 3),
        messages: [
          { role: "system", content: SYSTEM },
          { role: "user", content: organizePrompt(input) },
        ],
      }),
      signal: AbortSignal.timeout(config.timeoutMs),
    });
    if (!res.ok) {
      console.error(`[voice] organize failed: ${res.status}`);
      return undefined;
    }
    const body = (await res.json()) as { choices?: { message?: { content?: string } }[] };
    const out = body.choices?.[0]?.message?.content?.trim();
    return out ? out : undefined;
  } catch (err) {
    console.error("[voice] organize", (err as Error).message);
    return undefined;
  }
}
