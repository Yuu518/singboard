import { parse, type ParseError } from 'jsonc-parser'
import {
  copyToRunningConfig,
  fetchUrl,
  getRemoteConfigPath,
  replaceConfig,
  validateSingboxConfigContent,
} from '@/bridge/config'
import type { ConfigProfile } from '@/types'

export const SUBSCRIPTION_USER_AGENT = 'sing-box'

export interface RemoteRefreshContext {
  singboxPath: string
  workingDir: string
  active: boolean | (() => boolean)
  stillExists?: () => boolean
}

export class ConfigRemovedError extends Error {
  constructor() {
    super('配置已被删除，已放弃这次更新')
    this.name = 'ConfigRemovedError'
  }
}

function resolveActive(active: boolean | (() => boolean)): boolean {
  return typeof active === 'function' ? active() : active
}

export interface RemoteRefreshResult {
  path: string
  changed: boolean
  appliedToRunning: boolean
}

function reason(error: unknown): string {
  if (error instanceof Error) return error.message
  return String(error ?? '')
}

export function firstLine(text: string): string {
  return text.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? text.trim()
}

export function assertConfigShape(content: string): void {
  const errors: ParseError[] = []
  const value = parse(content, errors, { allowTrailingComma: true, disallowComments: false })
  if (errors.length > 0 || value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error('下载的内容不是 sing-box 配置，可能是订阅的错误页、Clash YAML 或 base64 订阅')
  }
}

export async function refreshRemoteConfig(
  profile: ConfigProfile,
  context: RemoteRefreshContext,
): Promise<RemoteRefreshResult> {
  const content = await fetchUrl(profile.source, SUBSCRIPTION_USER_AGENT)
  assertConfigShape(content)
  const path = await getRemoteConfigPath(profile.id)
  if (context.singboxPath) {
    try {
      await validateSingboxConfigContent(context.singboxPath, path, content, context.workingDir || undefined)
    } catch (error) {
      throw new Error(`sing-box 校验未通过：${reason(error)}`)
    }
  }
  if (context.stillExists && !context.stillExists()) throw new ConfigRemovedError()
  const changed = await replaceConfig(path, content)
  let appliedToRunning = false
  if (context.singboxPath && resolveActive(context.active) && (!context.stillExists || context.stillExists())) {
    appliedToRunning = (await copyToRunningConfig(path)).changed
  }
  return { path, changed, appliedToRunning }
}

export const MAX_UPDATE_INTERVAL_HOURS = 576

export function normalizeUpdateInterval(hours: unknown): number {
  const value = Number(hours)
  if (!Number.isFinite(value) || value <= 0) return 0
  return Math.min(MAX_UPDATE_INTERVAL_HOURS, Math.max(1, Math.round(value)))
}

function timeOf(iso?: string): number {
  if (!iso) return 0
  const value = new Date(iso).getTime()
  return Number.isNaN(value) ? 0 : value
}

export function currentVersionAt(profile: Pick<ConfigProfile, 'lastUpdated' | 'lastLocalChangeAt'>): number {
  return Math.max(timeOf(profile.lastUpdated), timeOf(profile.lastLocalChangeAt))
}

export function describeRefreshError(error: unknown): string {
  return reason(error) || '未知错误'
}
