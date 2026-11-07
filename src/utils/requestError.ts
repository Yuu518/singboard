import axios from 'axios'

function normalizeReasonText(text: string): string {
  const normalized = text.replace(/\s+/g, ' ').trim()
  return normalized || '请求失败'
}

function extractReasonFromData(data: unknown): string | null {
  if (typeof data === 'string' && data.trim().length > 0) {
    return normalizeReasonText(data)
  }
  if (data && typeof data === 'object') {
    const record = data as Record<string, unknown>
    const message =
      (typeof record.message === 'string' && record.message)
      || (typeof record.error === 'string' && record.error)
      || (typeof record.msg === 'string' && record.msg)
    if (message && message.trim().length > 0) {
      return normalizeReasonText(message)
    }
  }
  return null
}

const LOCALIZED_REASONS: Record<string, string> = {
  Timeout: '超时',
  Unauthorized: '认证失败，请检查 Clash API 密钥',
  'An error occurred in the delay test': '测速出错',
}

function statusReason(status: number): string | null {
  if (status === 401) return '认证失败 (401)，请检查 Clash API 密钥'
  if (status === 403) return '没有权限 (403)'
  if (status === 404) return '接口或资源不存在 (404)'
  return null
}

export function getRequestErrorReason(error: unknown): string {
  if (axios.isAxiosError(error)) {
    const status = error.response?.status
    const knownStatus = status ? statusReason(status) : null
    if (knownStatus) {
      return knownStatus
    }

    const dataReason = extractReasonFromData(error.response?.data)
    if (dataReason) {
      return LOCALIZED_REASONS[dataReason] ?? dataReason
    }

    if (error.code === 'ECONNABORTED' || error.code === 'ETIMEDOUT') {
      return '请求超时'
    }
    if (error.code === 'ERR_NETWORK') {
      return '无法连接到 Clash API，请确认核心正在运行且地址正确'
    }
    if (status) {
      return `请求失败 (${status})`
    }

    if (typeof error.message === 'string' && error.message.trim().length > 0) {
      return normalizeReasonText(error.message)
    }
    return '请求失败'
  }

  if (error instanceof Error && error.message.trim().length > 0) {
    return normalizeReasonText(error.message)
  }
  if (typeof error === 'string' && error.trim().length > 0) {
    return normalizeReasonText(error)
  }
  return '未知错误'
}
