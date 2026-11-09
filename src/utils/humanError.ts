const RULES: Array<[RegExp, string]> = [
  [/\bHTTP 404\b|\(404\)/i, '下载地址不存在 (404)，请检查镜像前缀'],
  [/\bHTTP 5\d\d\b/i, '服务器暂时出错，请稍后再试'],
  [/timed?\s*out|deadline has elapsed|超时/i, '连接超时，请检查网络后重试'],
  [/dns error|failed to lookup|no such host|name or service not known/i, '无法解析域名，请检查网络或 DNS 设置'],
  [/certificate|\btls\b|\bssl\b|handshake/i, 'HTTPS 证书校验失败，网络或镜像可能被拦截'],
  [/connection refused|actively refused|积极拒绝/i, '对方拒绝了连接，请检查地址是否正确'],
  [/error sending request|connection (reset|closed|aborted)|network is unreachable|broken pipe|unexpected eof/i, '网络连接失败，请检查网络，或设置下载镜像后重试'],
  [/being used by another process|os error 32/i, '文件正被其他程序占用，请关闭后重试'],
  [/access is denied|permission denied|os error 5\b/i, '没有权限访问该文件或目录'],
  [/cannot find the (file|path)|no such file|os error [23]\b/i, '找不到该文件或目录'],
]

function rawMessage(error: unknown): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  if (error && typeof error === 'object' && typeof (error as { message?: unknown }).message === 'string') {
    return (error as { message: string }).message
  }
  return ''
}

function stripNoise(message: string): string {
  return message
    .replace(/\s*(for url\s*)?\(?https?:\/\/[^\s)]+\)?/gi, '')
    .replace(/\s+/g, ' ')
    .replace(/[:：]\s*$/, '')
    .trim()
}

export function humanizeError(error: unknown): string {
  const raw = rawMessage(error)
  for (const [pattern, text] of RULES) {
    if (pattern.test(raw)) return text
  }
  return stripNoise(raw) || '未知错误'
}

export function failureMessage(action: string, error: unknown): string {
  return `${action}：${humanizeError(error)}`
}
