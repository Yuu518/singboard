import { describe, expect, it } from 'vitest'
import { findLocalDuplicate, isAbsolutePath, resolveLocalPath, validateRemoteAddress } from './configSources'
import type { ConfigProfile } from '@/types'

const profiles: ConfigProfile[] = [
  { id: 'r1', name: '机场 A', type: 'remote', source: 'https://sub.example.com/a', autoUpdateInterval: 0 },
  { id: 'l1', name: '家里', type: 'local', source: 'D:\\sb\\home.json', autoUpdateInterval: 0 },
]

describe('config sources', () => {
  it('recognises absolute Windows, UNC and POSIX paths', () => {
    expect(isAbsolutePath('C:\\a.json')).toBe(true)
    expect(isAbsolutePath('\\\\nas\\share\\a.json')).toBe(true)
    expect(isAbsolutePath('/etc/a.json')).toBe(true)
    expect(isAbsolutePath('a.json')).toBe(false)
  })

  it('resolves a bare file name against the working directory', () => {
    expect(resolveLocalPath('config.json', 'D:\\sb')).toEqual({ path: 'D:\\sb\\config.json' })
    expect(resolveLocalPath('config.json', 'D:\\sb\\')).toEqual({ path: 'D:\\sb\\config.json' })
    expect(resolveLocalPath(' C:\\x.json ', '')).toEqual({ path: 'C:\\x.json' })
    expect(resolveLocalPath('config.json', '')).toEqual({ error: '工作目录未配置，请填写完整路径' })
  })

  it('rejects malformed or already added remote addresses but allows keeping your own', () => {
    expect(validateRemoteAddress('sub.example.com/a', profiles)).toContain('http://')
    expect(validateRemoteAddress('https://sub.example.com/a', profiles)).toBe('这个地址已经添加为「机场 A」')
    expect(validateRemoteAddress('https://sub.example.com/a', profiles, 'r1')).toBe('')
    expect(validateRemoteAddress('https://other.example.com', profiles)).toBe('')
  })

  it('finds an already added local file regardless of case', () => {
    expect(findLocalDuplicate('d:\\SB\\home.json', profiles)?.name).toBe('家里')
    expect(findLocalDuplicate('D:\\sb\\work.json', profiles)).toBeUndefined()
  })
})
