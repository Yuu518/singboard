import { describe, expect, it } from 'vitest'
import { extractAllJsoncValueTexts, findJsoncProblems, hasJsoncComments, parseJsonWithComments, replaceJsoncValueText } from './jsonc'

describe('jsonc helpers', () => {
  it('detects comments but not slashes inside strings', () => {
    expect(hasJsoncComments('{ "a": 1 // note\n}')).toBe(true)
    expect(hasJsoncComments('{ "url": "https://example.com/a" }')).toBe(false)
  })

  it('reports syntax problems with line and column in Chinese', () => {
    const source = '{\n  "log": {\n    "level": "info"\n  \n}'
    const problems = findJsoncProblems(source)
    expect(problems[0]).toMatchObject({ line: 5, column: 2, message: '缺少右花括号 }' })
    expect(() => parseJsonWithComments('{\n  "a": 1,\n  "b"\n}')).toThrow('第 4 行第 1 列：缺少冒号')
  })

  it('keeps a trailing line comment from swallowing the following comma', () => {
    const source = '{\n  "log": {},\n  "dns": {}\n}'
    const replaced = replaceJsoncValueText(source, 'log', '{ "level": "info" } // note')!
    expect(replaced).toBe('{\n  "log": { "level": "info" } // note\n  ,\n  "dns": {}\n}')
    expect(parseJsonWithComments(replaced)).toEqual({ log: { level: 'info' }, dns: {} })
  })

  it('extracts every top-level value text in one pass', () => {
    expect(extractAllJsoncValueTexts('{ "log": { "level": "info" }, /* x */ "dns": [] }')).toEqual({
      log: '{ "level": "info" }',
      dns: '[]',
    })
  })

  it('replaces one top-level value and leaves the rest untouched', () => {
    const source = '{\n  // keep\n  "log": { "level": "info" },\n  "dns": {}\n}'
    expect(replaceJsoncValueText(source, 'log', '{ "level": "warn" }')).toBe('{\n  // keep\n  "log": { "level": "warn" },\n  "dns": {}\n}')
    expect(replaceJsoncValueText(source, 'route', '{}')).toBeNull()
  })
})
