import { parse, parseTree, modify, applyEdits, stripComments, createScanner, type ParseError, printParseErrorCode } from 'jsonc-parser'

export function hasJsoncComments(source: string): boolean {
  return stripComments(source) !== source
}

export function replaceJsoncValueText(source: string, key: string, valueText: string): string | null {
  const tree = parseTree(source, undefined, { disallowComments: false, allowTrailingComma: true })
  if (!tree || tree.type !== 'object' || !tree.children) return null
  for (const prop of tree.children) {
    if (!prop.children || prop.children.length < 2) continue
    if (prop.children[0].value !== key) continue
    const valueNode = prop.children[1]
    let text = valueText.replace(/\s+$/, '')
    if (endsWithLineComment(text)) {
      const lineStart = source.lastIndexOf('\n', prop.offset) + 1
      const indent = /^[ \t]*/.exec(source.slice(lineStart))?.[0] ?? ''
      text += `\n${indent}`
    }
    return source.slice(0, valueNode.offset) + text + source.slice(valueNode.offset + valueNode.length)
  }
  return null
}

const TOKEN_LINE_COMMENT = 12
const TOKEN_LINE_BREAK = 14
const TOKEN_WHITESPACE = 15
const TOKEN_EOF = 17

function endsWithLineComment(text: string): boolean {
  const scanner = createScanner(text, false)
  let last: number = TOKEN_EOF
  for (let kind: number = scanner.scan(); kind !== TOKEN_EOF; kind = scanner.scan()) {
    if (kind !== TOKEN_WHITESPACE && kind !== TOKEN_LINE_BREAK) last = kind
  }
  return last === TOKEN_LINE_COMMENT
}

export interface JsoncProblem {
  from: number
  to: number
  line: number
  column: number
  message: string
}

const PARSE_ERROR_TEXT: Record<string, string> = {
  InvalidSymbol: '无效的字符',
  InvalidNumberFormat: '数字格式不对',
  PropertyNameExpected: '这里应该是属性名',
  ValueExpected: '这里缺少值',
  ColonExpected: '缺少冒号',
  CommaExpected: '缺少逗号',
  CloseBraceExpected: '缺少右花括号 }',
  CloseBracketExpected: '缺少右方括号 ]',
  EndOfFileExpected: '内容在这里应该结束了',
  InvalidCommentToken: '注释格式不对',
  UnexpectedEndOfComment: '注释没有结束',
  UnexpectedEndOfString: '字符串没有结束',
  UnexpectedEndOfNumber: '数字没有结束',
  InvalidUnicode: '无效的 Unicode 转义',
  InvalidEscapeCharacter: '无效的转义字符',
  InvalidCharacter: '无效的字符',
}

export function lineColumnAt(source: string, offset: number): { line: number; column: number } {
  let line = 1
  let lineStart = 0
  const end = Math.min(offset, source.length)
  for (let i = 0; i < end; i++) {
    if (source.charCodeAt(i) === 10) {
      line++
      lineStart = i + 1
    }
  }
  return { line, column: end - lineStart + 1 }
}

export function findJsoncProblems(source: string): JsoncProblem[] {
  const errors: ParseError[] = []
  parse(source, errors, { disallowComments: false, allowTrailingComma: true, allowEmptyContent: false })
  return errors.map((error) => {
    const code = printParseErrorCode(error.error)
    const { line, column } = lineColumnAt(source, error.offset)
    return {
      from: error.offset,
      to: error.offset + Math.max(error.length, 1),
      line,
      column,
      message: PARSE_ERROR_TEXT[code] ?? code,
    }
  })
}

export function parseJsonWithComments<T = unknown>(input: string): T {
  const errors: ParseError[] = []
  const parsed = parse(input, errors, {
    disallowComments: false,
    allowTrailingComma: true,
    allowEmptyContent: false,
  })

  if (errors.length > 0) {
    const first = errors[0]
    const code = printParseErrorCode(first.error)
    const { line, column } = lineColumnAt(input, first.offset)
    throw new Error(`第 ${line} 行第 ${column} 列：${PARSE_ERROR_TEXT[code] ?? code}`)
  }

  return parsed as T
}

export function extractAllJsoncValueTexts(source: string): Record<string, string> {
  const tree = parseTree(source, undefined, { disallowComments: false, allowTrailingComma: true })
  const out: Record<string, string> = {}
  if (!tree || tree.type !== 'object' || !tree.children) return out
  for (const prop of tree.children) {
    if (!prop.children || prop.children.length < 2) continue
    const key = prop.children[0].value
    if (typeof key !== 'string') continue
    const valueNode = prop.children[1]
    out[key] = source.substring(valueNode.offset, valueNode.offset + valueNode.length)
  }
  return out
}

export function applyJsoncModification(source: string, path: (string | number)[], value: unknown): string {
  const edits = modify(source, path, value, { formattingOptions: { tabSize: 2, insertSpaces: true } })
  return applyEdits(source, edits)
}

export function extractJsoncValueText(source: string, key: string): string | null {
  const tree = parseTree(source, undefined, { disallowComments: false, allowTrailingComma: true })
  if (!tree || tree.type !== 'object' || !tree.children) return null
  for (const prop of tree.children) {
    if (!prop.children || prop.children.length < 2) continue
    const keyNode = prop.children[0]
    if (keyNode.value === key) {
      const valueNode = prop.children[1]
      return source.substring(valueNode.offset, valueNode.offset + valueNode.length)
    }
  }
  return null
}
