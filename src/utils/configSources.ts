import type { ConfigProfile } from '@/types'

export function isAbsolutePath(path: string): boolean {
  return /^[a-zA-Z]:[/\\]/.test(path) || path.startsWith('\\\\') || path.startsWith('/')
}

export function resolveLocalPath(input: string, workingDir: string): { path: string } | { error: string } {
  const trimmed = input.trim()
  if (isAbsolutePath(trimmed)) return { path: trimmed }
  const dir = workingDir.trim()
  if (!dir) return { error: '工作目录未配置，请填写完整路径' }
  return { path: dir.endsWith('\\') || dir.endsWith('/') ? `${dir}${trimmed}` : `${dir}\\${trimmed}` }
}

export function validateRemoteAddress(url: string, profiles: ConfigProfile[], selfId?: string): string {
  if (!/^https?:\/\//i.test(url)) return '远程地址要以 http:// 或 https:// 开头'
  const existing = profiles.find((p) => p.type === 'remote' && p.id !== selfId && p.source.trim() === url)
  return existing ? `这个地址已经添加为「${existing.name}」` : ''
}

export function findLocalDuplicate(path: string, profiles: ConfigProfile[]): ConfigProfile | undefined {
  const target = path.toLowerCase()
  return profiles.find((p) => p.type === 'local' && p.source.toLowerCase() === target)
}
