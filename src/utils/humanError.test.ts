import { describe, expect, it } from 'vitest'
import { failureMessage, humanizeError } from './humanError'

describe('humanizeError', () => {
  it('turns transport errors into advice', () => {
    expect(humanizeError('请求 GitHub API 失败: error sending request for url (https://api.github.com/repos/a/b/releases/latest)'))
      .toBe('网络连接失败，请检查网络，或设置下载镜像后重试')
    expect(humanizeError(new Error('operation timed out'))).toBe('连接超时，请检查网络后重试')
    expect(humanizeError('dns error: failed to lookup address information')).toBe('无法解析域名，请检查网络或 DNS 设置')
    expect(humanizeError('下载失败: HTTP 404')).toBe('下载地址不存在 (404)，请检查镜像前缀')
    expect(humanizeError('下载失败: HTTP 504 Gateway Timeout')).toBe('服务器暂时出错，请稍后再试')
    expect(humanizeError('GitHub Token 无效或已过期 (401)')).toBe('GitHub Token 无效或已过期，请在设置里重新填写')
  })

  it('explains Windows file errors', () => {
    expect(humanizeError('替换核心失败: The process cannot access the file because it is being used by another process. (os error 32)'))
      .toBe('文件正被其他程序占用，请关闭后重试')
    expect(humanizeError('Access is denied. (os error 5)')).toBe('没有权限访问该文件或目录')
  })

  it('keeps messages that are already readable and drops URLs from them', () => {
    expect(humanizeError('GitHub API 限流，请稍后再试')).toBe('GitHub API 限流，请稍后再试')
    expect(humanizeError('该版本缺少校验信息 https://example.test/x')).toBe('该版本缺少校验信息')
  })

  it('never returns an empty reason', () => {
    expect(humanizeError(undefined)).toBe('未知错误')
    expect(humanizeError({})).toBe('未知错误')
  })

  it('prefixes the action', () => {
    expect(failureMessage('检查更新失败', 'GitHub API 限流，请稍后再试')).toBe('检查更新失败：GitHub API 限流，请稍后再试')
  })
})
