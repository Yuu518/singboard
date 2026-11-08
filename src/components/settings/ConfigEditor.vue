<script setup lang="ts">
import { computed, ref, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { useToastStore } from '@/stores/toast'
import { readSingboxConfig, replaceConfig, validateSingboxConfigContent } from '@/bridge/config'
import ConfirmDialog from '@/components/common/ConfirmDialog.vue'
import type { SaveCheck } from '@/types'
import { EditorView, keymap, lineNumbers, highlightActiveLineGutter, highlightSpecialChars, drawSelection, highlightActiveLine, Decoration, ViewPlugin, ViewUpdate } from '@codemirror/view'
import { EditorState, RangeSetBuilder, type Extension } from '@codemirror/state'
import { defaultKeymap, history, historyKeymap, indentWithTab, undo, redo } from '@codemirror/commands'
import { json } from '@codemirror/lang-json'
import { syntaxHighlighting, defaultHighlightStyle, bracketMatching, foldGutter, foldKeymap, indentOnInput, HighlightStyle } from '@codemirror/language'
import { searchKeymap, highlightSelectionMatches } from '@codemirror/search'
import { autocompletion, completionKeymap, closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete'
import { lintGutter, linter, type Diagnostic } from '@codemirror/lint'
import { tags } from '@lezer/highlight'
import { parseJsonWithComments, applyJsoncModification, extractAllJsoncValueTexts, findJsoncProblems, hasJsoncComments, replaceJsoncValueText } from '@/utils/jsonc'

const props = defineProps<{
  configPath: string
  singboxPath: string
  workingDir: string
  validateBeforeSave?: boolean
}>()

const emit = defineEmits<{ saved: [check: SaveCheck] }>()
const checking = ref(false)

defineExpose({ isDirty: () => hasChanges.value, save: () => handleSave() })
const loadError = ref('')

const { pushToast } = useToastStore()

const editorContainer = ref<HTMLElement>()
let editorView: EditorView | null = null
const loading = ref(false)
const saving = ref(false)
const validating = ref(false)
const hasChanges = ref(false)
const editorMode = ref<'whole' | 'module'>('whole')

const preferredModuleOrder = [
  'log',
  'dns',
  'ntp',
  'certificate',
  'endpoints',
  'inbounds',
  'outbounds',
  'route',
  'services',
  'experimental',
] as const

const moduleDefaults: Record<string, unknown> = {
  log: {},
  dns: {},
  ntp: {},
  certificate: {},
  endpoints: [],
  inbounds: [],
  outbounds: [],
  route: {},
  services: [],
  experimental: {},
}

const activeModule = ref<string>('log')
const fullConfigObject = ref<Record<string, unknown> | null>(null)
const savedRawContent = ref('')
const wholeRawContent = ref('')
const moduleItems = computed<Array<{ key: string; label: string }>>(() => {
  const keys = new Set<string>()
  const dynamicKeys = fullConfigObject.value ? Object.keys(fullConfigObject.value) : []

  for (const key of preferredModuleOrder) {
    if (dynamicKeys.includes(key)) keys.add(key)
  }
  for (const key of dynamicKeys) {
    keys.add(key)
  }

  if (keys.size === 0) {
    for (const key of preferredModuleOrder) keys.add(key)
  }

  return Array.from(keys).map((key) => ({ key, label: key }))
})

const editorTheme = EditorView.theme({
  '&': {
    height: '100%',
    fontSize: '13px',
  },
  '.cm-scroller': {
    fontFamily: "'Cascadia Mono', 'SFMono-Regular', Consolas, monospace",
    overflow: 'auto',
  },
  '.cm-content': {
    caretColor: 'oklch(var(--bc))',
  },
  '.cm-cursor': {
    borderLeftColor: 'oklch(var(--bc))',
  },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
    backgroundColor: 'oklch(var(--p) / 0.2) !important',
  },
  '.cm-activeLine': {
    backgroundColor: 'oklch(var(--bc) / 0.05)',
  },
  '.cm-activeLineGutter': {
    backgroundColor: 'oklch(var(--bc) / 0.08)',
  },
  '.cm-gutters': {
    backgroundColor: 'transparent',
    color: 'oklch(var(--bc) / 0.4)',
    borderRight: '0.5px solid var(--hairline)',
  },
  '.cm-foldGutter': {
    width: '12px',
  },
  '.cm-tooltip': {
    backgroundColor: 'rgb(var(--glass-rgb) / var(--glass-pop-alpha))',
    backdropFilter: 'saturate(var(--glass-saturate)) blur(var(--glass-pop-blur))',
    border: 'none',
    borderRadius: '10px',
    boxShadow: 'var(--glass-highlight), var(--glass-edge), var(--glass-shadow)',
    color: 'oklch(var(--bc))',
    overflow: 'hidden',
  },
  '.cm-panels': {
    backgroundColor: 'transparent',
    color: 'oklch(var(--bc))',
  },
  '.cm-panels.cm-panels-top': {
    borderBottom: '0.5px solid var(--hairline)',
  },
  '.cm-panels.cm-panels-bottom': {
    borderTop: '0.5px solid var(--hairline)',
  },
  '.cm-panel.cm-search': {
    backgroundColor: 'transparent',
    padding: '6px 8px',
  },
  '.cm-panel.cm-search input': {
    backgroundColor: 'var(--fill)',
    color: 'oklch(var(--bc))',
    border: 'none',
    borderRadius: '8px',
    padding: '3px 8px',
  },
  '.cm-panel.cm-search button': {
    backgroundColor: 'var(--fill)',
    backgroundImage: 'none',
    color: 'oklch(var(--bc))',
    border: 'none',
    borderRadius: '9999px',
    padding: '3px 10px',
  },
  '.cm-matchingBracket': {
    backgroundColor: 'oklch(var(--p) / 0.15)',
    outline: '1px solid oklch(var(--p) / 0.4)',
  },
  '.cm-jsonc-comment': {
    color: 'oklch(var(--bc) / 0.4) !important',
    fontStyle: 'italic',
  },
  '.cm-jsonc-comment span': {
    color: 'oklch(var(--bc) / 0.4) !important',
  },
})

const highlightColors = HighlightStyle.define([
  { tag: tags.string, color: 'oklch(var(--su, 0.6 0.15 160))' },
  { tag: tags.number, color: 'oklch(var(--wa, 0.7 0.15 60))' },
  { tag: tags.bool, color: 'oklch(var(--er, 0.65 0.2 25))' },
  { tag: tags.null, color: 'oklch(var(--bc) / 0.5)' },
  { tag: tags.propertyName, color: 'oklch(var(--p, 0.6 0.2 270))' },
  { tag: tags.punctuation, color: 'oklch(var(--bc) / 0.6)' },
])

const commentMark = Decoration.mark({ class: 'cm-jsonc-comment' })

function buildCommentDecorations(view: EditorView) {
  const builder = new RangeSetBuilder<Decoration>()
  const doc = view.state.doc
  let inBlock = false
  for (let i = 1; i <= doc.lines; i++) {
    const line = doc.line(i)
    const text = line.text
    if (inBlock) {
      const endIdx = text.indexOf('*/')
      if (endIdx >= 0) {
        builder.add(line.from, line.from + endIdx + 2, commentMark)
        inBlock = false
      } else {
        builder.add(line.from, line.to, commentMark)
      }
      continue
    }
    const trimmed = text.trimStart()
    if (trimmed.startsWith('//')) {
      builder.add(line.from, line.to, commentMark)
    } else if (trimmed.startsWith('/*')) {
      const endIdx = text.indexOf('*/', text.indexOf('/*') + 2)
      if (endIdx >= 0) {
        builder.add(line.from + text.indexOf('/*'), line.from + endIdx + 2, commentMark)
      } else {
        builder.add(line.from + text.indexOf('/*'), line.to, commentMark)
        inBlock = true
      }
    } else {
      const lineIdx = text.indexOf('//')
      if (lineIdx >= 0) {
        let inStr = false
        let escaped = false
        for (let j = 0; j < text.length - 1; j++) {
          if (escaped) { escaped = false; continue }
          if (text[j] === '\\') { escaped = true; continue }
          if (text[j] === '"') { inStr = !inStr; continue }
          if (!inStr && text[j] === '/' && text[j + 1] === '/') {
            builder.add(line.from + j, line.to, commentMark)
            break
          }
        }
      }
    }
  }
  return builder.finish()
}

const commentHighlightPlugin = ViewPlugin.fromClass(
  class {
    decorations
    constructor(view: EditorView) {
      this.decorations = buildCommentDecorations(view)
    }
    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildCommentDecorations(update.view)
      }
    }
  },
  { decorations: (v) => v.decorations },
)

let editorExtensions: Extension[] = []

function createEditor() {
  if (!editorContainer.value) return

  editorExtensions = [
      lineNumbers(),
      highlightActiveLineGutter(),
      highlightSpecialChars(),
      history(),
      foldGutter(),
      drawSelection(),
      indentOnInput(),
      bracketMatching(),
      closeBrackets(),
      autocompletion(),
      highlightActiveLine(),
      highlightSelectionMatches(),
      lintGutter(),
      linter((view): Diagnostic[] => findJsoncProblems(view.state.doc.toString()).map((problem) => ({
        from: Math.min(problem.from, view.state.doc.length),
        to: Math.min(problem.to, view.state.doc.length),
        severity: 'error',
        message: problem.message,
      }))),
      EditorView.contentAttributes.of({ 'aria-label': '配置 JSON，按 Esc 后再按 Tab 可以离开编辑区' }),
      json(),
      editorTheme,
      commentHighlightPlugin,
      syntaxHighlighting(highlightColors),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      keymap.of([
        ...closeBracketsKeymap,
        ...defaultKeymap,
        ...searchKeymap,
        ...historyKeymap,
        ...foldKeymap,
        ...completionKeymap,
        indentWithTab,
        { key: 'Ctrl-/', run: () => { handleBatchComment(); return true } },
        { key: 'Mod-/', run: () => { handleBatchComment(); return true } },
        { key: 'Mod-s', run: () => { void handleSave(); return true } },
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          recomputeDirtyState()
        }
      }),
  ]
  const state = EditorState.create({ doc: '', extensions: editorExtensions })

  editorView = new EditorView({
    state,
    parent: editorContainer.value,
  })
}

function setEditorContent(content: string, keepChangeState = false, undoable = false) {
  if (!editorView) return
  if (undoable) {
    editorView.dispatch({ changes: { from: 0, to: editorView.state.doc.length, insert: content } })
  } else {
    editorView.setState(EditorState.create({ doc: content, extensions: editorExtensions }))
  }
  if (keepChangeState) recomputeDirtyState()
  else hasChanges.value = false
}

function getEditorContent(): string {
  return editorView?.state.doc.toString() ?? ''
}

function cloneDefaultValue(key: string): unknown {
  const val = moduleDefaults[key]
  if (val === undefined) return {}
  return Array.isArray(val) ? [] : {}
}

function ensureStructuredConfigFromEditor(showError = true): boolean {
  try {
    const parsed = parseJsonWithComments(getEditorContent())
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      if (showError) {
        pushToast({ message: '配置根节点必须是 JSON 对象', type: 'error' })
      }
      return false
    }
    fullConfigObject.value = parsed as Record<string, unknown>
    return true
  } catch (e: any) {
    if (showError) {
      pushToast({ message: '当前内容不是合法 JSON，无法进入分模块编辑：' + e.message, type: 'error' })
    }
    return false
  }
}

function ensureModuleType(key: string, value: unknown): boolean {
  if (!(key in moduleDefaults)) {
    return true
  }
  const shouldArray = Array.isArray(moduleDefaults[key])
  if (shouldArray) return Array.isArray(value)
  return !!value && typeof value === 'object' && !Array.isArray(value)
}

function applyEditorChangesToState(showError = true): boolean {
  const content = getEditorContent()
  if (editorMode.value === 'whole') {
    try {
      const parsed = parseJsonWithComments(content)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        if (showError) {
          pushToast({ message: '配置根节点必须是 JSON 对象', type: 'error' })
        }
        return false
      }
      fullConfigObject.value = parsed as Record<string, unknown>
      return true
    } catch (e: any) {
      if (showError) {
        pushToast({ message: 'JSON 语法错误：' + e.message, type: 'error' })
      }
      return false
    }
  }

  if (!fullConfigObject.value) {
    fullConfigObject.value = {}
  }
  try {
    const parsed = parseJsonWithComments(content)
    const key = activeModule.value
    if (!ensureModuleType(key, parsed)) {
      if (showError) {
        const expected = Array.isArray(moduleDefaults[key]) ? '数组' : '对象'
        pushToast({ message: `${key} 模块必须是 JSON ${expected}`, type: 'error' })
      }
      return false
    }
    fullConfigObject.value[key] = parsed
    return true
  } catch (e: any) {
    if (showError) {
      pushToast({ message: 'JSON 语法错误：' + e.message, type: 'error' })
    }
    return false
  }
}

function recomputeDirtyState() {
  if (editorMode.value === 'whole') {
    hasChanges.value = getEditorContent() !== savedRawContent.value
    return
  }
  hasChanges.value = composeWholeFromModules(currentModuleTexts()) !== savedRawContent.value
}

const moduleRawContents = ref<Record<string, string>>({})

function currentModuleTexts(): Record<string, string> {
  return { ...moduleRawContents.value, [activeModule.value]: getEditorContent() }
}

function isUntouchedDefault(key: string, text: string): boolean {
  return text.trim() === JSON.stringify(cloneDefaultValue(key), null, 2)
}

function composeWholeFromModules(texts: Record<string, string> = moduleRawContents.value, strict = false): string {
  const base = wholeRawContent.value
  const target = fullConfigObject.value ?? {}
  if (!base) return JSON.stringify(target, null, 2)
  try {
    const baseTexts = extractAllJsoncValueTexts(base)
    let raw = base
    for (const [key, text] of Object.entries(texts)) {
      const original = baseTexts[key]
      if (original === text) continue
      if (original === undefined && isUntouchedDefault(key, text)) continue
      raw = (original !== undefined ? replaceJsoncValueText(raw, key, text) : null)
        ?? applyJsoncModification(raw, [key], parseJsonWithComments(text))
    }
    const current = parseJsonWithComments<Record<string, unknown>>(raw)
    for (const [key, value] of Object.entries(target)) {
      if (key in texts) continue
      if (!(key in current) && isUntouchedDefault(key, JSON.stringify(value, null, 2))) continue
      if (JSON.stringify(current[key]) !== JSON.stringify(value)) {
        raw = applyJsoncModification(raw, [key], value)
      }
    }
    return raw
  } catch (error) {
    if (strict) throw error
    return JSON.stringify(target, null, 2)
  }
}

function renderModuleContent(keepChangeState = false) {
  if (!fullConfigObject.value) return
  const key = activeModule.value
  let value = fullConfigObject.value[key]
  if (value === undefined) {
    value = cloneDefaultValue(key)
    fullConfigObject.value[key] = value
  }
  const rawText = moduleRawContents.value[key]
  if (rawText !== undefined) {
    setEditorContent(rawText, keepChangeState)
  } else {
    setEditorContent(JSON.stringify(value, null, 2), keepChangeState)
  }
}

function switchMode(mode: 'whole' | 'module') {
  if (mode === editorMode.value) return

  if (!applyEditorChangesToState()) return

  const keepChanges = hasChanges.value
  if (mode === 'module') {
    if (!fullConfigObject.value && !ensureStructuredConfigFromEditor()) return
    wholeRawContent.value = getEditorContent()
    moduleRawContents.value = extractAllJsoncValueTexts(wholeRawContent.value)
    if (!moduleItems.value.some((item) => item.key === activeModule.value)) {
      activeModule.value = moduleItems.value[0]?.key ?? 'log'
    }
    editorMode.value = 'module'
    renderModuleContent(keepChanges)
    return
  }

  if (wholeRawContent.value) {
    const texts = currentModuleTexts()
    let restored: string
    try {
      restored = composeWholeFromModules(texts, true)
    } catch (error: any) {
      pushToast({ message: '无法在保留注释的前提下合并模块，先修正当前模块再切换：' + (error?.message || error), type: 'error' }, 8000)
      return
    }
    editorMode.value = 'whole'
    moduleRawContents.value = texts
    wholeRawContent.value = restored
    moduleRawContents.value = {}
    setEditorContent(restored, keepChanges)
  } else if (fullConfigObject.value) {
    editorMode.value = 'whole'
    moduleRawContents.value = {}
    setEditorContent(JSON.stringify(fullConfigObject.value, null, 2), keepChanges)
  }
}

function switchModule(next: string) {
  if (next === activeModule.value) return
  if (editorMode.value !== 'module') {
    activeModule.value = next
    return
  }
  moduleRawContents.value[activeModule.value] = getEditorContent()
  if (!applyEditorChangesToState()) return
  const keepChanges = hasChanges.value
  activeModule.value = next
  renderModuleContent(keepChanges)
}

async function loadConfig() {
  if (!props.configPath) {
    return
  }
  loading.value = true
  loadError.value = ''
  try {
    const content = await readSingboxConfig(props.configPath)
    savedRawContent.value = content
    wholeRawContent.value = content
    setEditorContent(content)
    try {
      const parsed = parseJsonWithComments(content)
      fullConfigObject.value =
        parsed && typeof parsed === 'object' && !Array.isArray(parsed)
          ? (parsed as Record<string, unknown>)
          : null
    } catch {
      fullConfigObject.value = null
    }
    recomputeDirtyState()
  } catch (e: any) {
    loadError.value = String(e?.message || e)
    setEditorContent('')
  } finally {
    loading.value = false
  }
}

function contentForSave(): string | null {
  if (editorMode.value === 'whole') return getEditorContent()
  try {
    return composeWholeFromModules(currentModuleTexts(), true)
  } catch {
    return null
  }
}

async function checkBeforeSave(content: string): Promise<SaveCheck | null> {
  if (!props.validateBeforeSave || !props.singboxPath) return 'skipped'
  try {
    await validateSingboxConfigContent(props.singboxPath, props.configPath, content, props.workingDir)
    return 'passed'
  } catch (error: any) {
    const detail = String(error?.message || error).split(/\r?\n/).map((line) => line.trim()).filter(Boolean).slice(0, 4).join('\n')
    const confirmed = await confirmDialog.value?.show({
      title: 'sing-box 校验未通过',
      message: `这是当前使用的配置，校验没有通过：\n${detail}\n\n仍然保存的话，文件会写入，但运行配置不会更新。`,
      confirmText: '仍然保存',
      cancelText: '取消',
      variant: 'danger',
    })
    return confirmed ? 'failed' : null
  }
}

function prepareSave(): { content: string; parsed?: Record<string, unknown> } | null {
  if (editorMode.value === 'whole') {
    const content = getEditorContent()
    try {
      const parsed = parseJsonWithComments(content)
      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        pushToast({ message: '配置根节点必须是 JSON 对象', type: 'error' })
        return null
      }
      return { content, parsed: parsed as Record<string, unknown> }
    } catch (e: any) {
      pushToast({ message: 'JSON 语法错误：' + e.message, type: 'error' })
      return null
    }
  }
  moduleRawContents.value[activeModule.value] = getEditorContent()
  if (!applyEditorChangesToState()) return null
  try {
    return { content: composeWholeFromModules(moduleRawContents.value, true) }
  } catch (error: any) {
    pushToast({ message: '无法在保留注释的前提下合并模块，没有保存：' + (error?.message || error), type: 'error' }, 8000)
    return null
  }
}

async function handleSave() {
  if (!props.configPath || saving.value || checking.value || loadError.value) return
  recomputeDirtyState()
  if (!hasChanges.value) {
    pushToast({ message: '没有需要保存的改动', type: 'info' })
    return
  }
  const prepared = prepareSave()
  if (!prepared) return
  checking.value = true
  let check: SaveCheck | null
  try {
    check = await checkBeforeSave(prepared.content)
  } finally {
    checking.value = false
  }
  if (check === null) return
  saving.value = true
  try {
    await replaceConfig(props.configPath, prepared.content)
    wholeRawContent.value = prepared.content
    savedRawContent.value = prepared.content
    if (editorMode.value === 'whole') {
      if (prepared.parsed) fullConfigObject.value = prepared.parsed
    } else {
      moduleRawContents.value = extractAllJsoncValueTexts(prepared.content)
      renderModuleContent()
    }
    hasChanges.value = false
    emit('saved', check)
  } catch (e: any) {
    pushToast({ message: '保存失败：' + (e?.message || e), type: 'error' })
  } finally {
    saving.value = false
  }
}

async function handleValidate() {
  if (!props.singboxPath || !props.configPath) {
    pushToast({ message: '请先在设置中配置 sing-box 路径', type: 'error' })
    return
  }
  validating.value = true
  try {
    let content: string
    if (editorMode.value === 'whole') {
      const rawContent = getEditorContent()
      try {
        const p = parseJsonWithComments(rawContent)
        if (!p || typeof p !== 'object' || Array.isArray(p)) {
          pushToast({ message: '配置根节点必须是 JSON 对象', type: 'error' })
          return
        }
      } catch (e: any) {
        pushToast({ message: 'JSON 语法错误：' + e.message, type: 'error' })
        return
      }
      content = rawContent
    } else {
      if (!applyEditorChangesToState()) return
      try {
        content = composeWholeFromModules(currentModuleTexts(), true)
      } catch (error: any) {
        pushToast({ message: '无法合并模块：' + (error?.message || error), type: 'error' })
        return
      }
    }
    await validateSingboxConfigContent(
      props.singboxPath,
      props.configPath,
      content,
      props.workingDir,
    )
    pushToast({ message: '配置文件校验通过', type: 'info' })
  } catch (e: any) {
    pushToast({ message: '校验失败：\n' + (e?.message || e), type: 'error' }, 8000)
  } finally {
    validating.value = false
  }
}

const confirmDialog = ref<InstanceType<typeof ConfirmDialog> | null>(null)

async function handleFormat() {
  if (hasJsoncComments(getEditorContent())) {
    const confirmed = await confirmDialog.value?.show({
      title: '格式化会移除注释',
      message: '格式化按标准 JSON 重新排版，当前内容里的注释会被删除。切换模块或模式之前，可以用撤销恢复。',
      confirmText: '仍然格式化',
      cancelText: '取消',
    })
    if (!confirmed) return
  }
  try {
    if (!applyEditorChangesToState()) return
    const formatted =
      editorMode.value === 'whole'
        ? JSON.stringify(fullConfigObject.value ?? {}, null, 2)
        : JSON.stringify((fullConfigObject.value ?? {})[activeModule.value], null, 2)
    setEditorContent(formatted, true, true)
    recomputeDirtyState()
    pushToast({ message: '已格式化', type: 'info' })
  } catch {}
}

function handleUndo() {
  if (editorView) undo(editorView)
}

function handleRedo() {
  if (editorView) redo(editorView)
}

function handleBatchComment() {
  if (!editorView) return

  const doc = editorView.state.doc
  const targetLineNumbers = new Set<number>()

  for (const range of editorView.state.selection.ranges) {
    const fromLine = doc.lineAt(range.from).number
    const toAnchor = range.empty ? range.to : Math.max(range.from, range.to - 1)
    const toLine = doc.lineAt(toAnchor).number
    for (let lineNo = fromLine; lineNo <= toLine; lineNo += 1) {
      targetLineNumbers.add(lineNo)
    }
  }

  if (targetLineNumbers.size === 0) {
    pushToast({ message: '请选择需要注释的行', type: 'error' })
    return
  }

  const lineInfos = Array.from(targetLineNumbers)
    .sort((a, b) => b - a)
    .map((lineNo) => {
      const line = doc.line(lineNo)
      const commentMatch = line.text.match(/^(\s*)\/\/ ?/)
      return { line, commentMatch }
    })

  const commentedLines = lineInfos.filter((info) => !!info.commentMatch)
  const uncommentedLines = lineInfos.filter((info) => !info.commentMatch)

  const changes: Array<{ from: number; to: number; insert: string }> = []
  let changedCount = 0

  if (uncommentedLines.length === 0 && commentedLines.length > 0) {
    for (const info of commentedLines) {
      const line = info.line
      const match = info.commentMatch
      if (!match) continue
      const indentLength = match[1].length
      const markerLength = match[0].length - indentLength
      const from = line.from + indentLength
      changes.push({
        from,
        to: from + markerLength,
        insert: '',
      })
      changedCount += 1
    }

    if (changes.length === 0) {
      pushToast({ message: '所选行无需处理', type: 'info' })
      return
    }

    editorView.dispatch({ changes })
    recomputeDirtyState()
    pushToast({ message: `已取消注释 ${changedCount} 行`, type: 'info' })
    return
  }

  for (const info of uncommentedLines) {
    const line = info.line
    const indentLength = (line.text.match(/^\s*/) ?? [''])[0].length
    changes.push({
      from: line.from + indentLength,
      to: line.from + indentLength,
      insert: '// ',
    })
    changedCount += 1
  }

  if (changes.length === 0) {
    pushToast({ message: '所选行无需处理', type: 'info' })
    return
  }

  editorView.dispatch({ changes })
  recomputeDirtyState()
  pushToast({ message: `已注释 ${changedCount} 行`, type: 'info' })
}

onMounted(() => {
  nextTick(() => {
    createEditor()
    loadConfig()
  })
})

onBeforeUnmount(() => {
  editorView?.destroy()
  editorView = null
})

watch(() => props.configPath, (newPath, oldPath) => {
  if (newPath && newPath !== oldPath) {
    loadConfig()
  }
})
</script>

<template>
  <div class="surface-card p-3 flex flex-col h-full min-h-0">
    <div class="flex flex-col gap-2 shrink-0">
      <div class="flex flex-wrap items-center gap-1">
        <h2 class="sr-only">配置编辑</h2>
        <button
          class="btn btn-xs btn-ghost"
          @click="handleUndo"
          title="撤销（Ctrl+Z）"
        >
          撤销
        </button>
        <button
          class="btn btn-xs btn-ghost"
          @click="handleRedo"
          title="重做（Ctrl+Y）"
        >
          重做
        </button>
        <button
          class="btn btn-xs btn-ghost"
          @click="handleFormat"
          title="格式化 JSON"
        >
          格式化
        </button>
        <button
          class="btn btn-xs btn-ghost"
          @click="handleBatchComment"
          title="批量注释所选行"
        >
          注释
        </button>
        <button
          class="btn btn-xs btn-ghost"
          :disabled="validating"
          @click="handleValidate"
          title="使用 sing-box 校验配置"
        >
          <span v-if="validating" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          {{ validating ? '校验中…' : '校验' }}
        </button>
        <button
          class="btn btn-xs disabled:bg-transparent"
          :class="hasChanges ? 'btn-primary' : 'btn-ghost'"
          :disabled="!hasChanges || saving || checking || !!loadError"
          @click="handleSave"
          title="保存（Ctrl+S）"
        >
          <span v-if="saving || checking" class="loading loading-spinner loading-xs" aria-hidden="true"></span>
          {{ checking ? '校验中…' : '保存' }}
        </button>
      </div>
      <div class="flex flex-wrap items-center gap-1.5">
        <div class="flex flex-wrap items-center gap-2">
          <div class="tabs tabs-boxed tabs-xs" role="group" aria-label="编辑方式">
            <button
              type="button"
              class="tab"
              :class="{ 'tab-active': editorMode === 'whole' }"
              :aria-pressed="editorMode === 'whole'"
              @click="switchMode('whole')"
            >
              整体编辑
            </button>
            <button
              type="button"
              class="tab"
              :class="{ 'tab-active': editorMode === 'module' }"
              :aria-pressed="editorMode === 'module'"
              @click="switchMode('module')"
            >
              分模块编辑
            </button>
          </div>
          <select
            v-if="editorMode === 'module'"
            class="select select-xs select-bordered w-36"
            aria-label="模块"
            :value="activeModule"
            @change="switchModule(($event.target as HTMLSelectElement).value)"
          >
            <option v-for="item in moduleItems" :key="item.key" :value="item.key">
              {{ item.label }}
            </option>
          </select>
        </div>
        <div class="flex-1"></div>
        <span v-if="hasChanges" role="status" class="inline-flex h-5 items-center rounded-full bg-base-content/[0.08] px-2 text-[11px] text-base-content/75">未保存</span>
      </div>
    </div>

    <div class="surface-fill relative flex-1 min-h-[160px] rounded-[var(--radius-tile)] overflow-hidden mt-2">
      <div
        ref="editorContainer"
        class="h-full w-full overflow-hidden"
      ></div>
      <div
        v-if="loading"
        class="absolute inset-0 flex items-center justify-center bg-base-100/60 text-base-content/60 backdrop-blur-sm"
      >
        加载中…
      </div>
      <div
        v-else-if="loadError"
        class="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-base-100/70 px-6 text-center backdrop-blur-sm"
        role="alert"
      >
        <p class="text-sm text-base-content/80">读取配置文件失败，没有内容可以编辑</p>
        <p class="settings-mono max-w-full break-all text-xs text-base-content/65">{{ loadError }}</p>
        <button type="button" class="btn btn-sm btn-glass px-5" @click="loadConfig">重新读取</button>
      </div>
    </div>
    <ConfirmDialog ref="confirmDialog" />
  </div>
</template>
