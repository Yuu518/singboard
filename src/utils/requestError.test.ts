import { AxiosError, AxiosHeaders } from 'axios'
import { describe, expect, it } from 'vitest'
import { getRequestErrorReason } from './requestError'

function responseError(status: number, data: unknown) {
  const config = { headers: new AxiosHeaders() }
  return new AxiosError('Request failed with status code ' + status, 'ERR_BAD_RESPONSE', config, null, {
    status,
    statusText: '',
    headers: {},
    config,
    data,
  })
}

describe('request error reason', () => {
  it('explains authentication failures instead of echoing the core', () => {
    expect(getRequestErrorReason(responseError(401, { message: 'Unauthorized' }))).toBe('认证失败 (401)，请检查 Clash API 密钥')
  })

  it('translates known core messages and keeps unknown ones', () => {
    expect(getRequestErrorReason(responseError(504, { message: 'Timeout' }))).toBe('超时')
    expect(getRequestErrorReason(responseError(500, { message: 'outbound not found' }))).toBe('outbound not found')
  })

  it('describes network failures and timeouts in Chinese', () => {
    expect(getRequestErrorReason(new AxiosError('Network Error', 'ERR_NETWORK'))).toBe('无法连接到 Clash API，请确认核心正在运行且地址正确')
    expect(getRequestErrorReason(new AxiosError('timeout of 15000ms exceeded', 'ECONNABORTED'))).toBe('请求超时')
  })

  it('falls back to the status code when the core sends no message', () => {
    expect(getRequestErrorReason(responseError(502, ''))).toBe('请求失败 (502)')
  })
})
