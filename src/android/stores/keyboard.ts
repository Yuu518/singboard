import { ref } from 'vue'

const REVEAL_MARGIN = 16

export const keyboardVisible = ref(false)

const rootStyle = document.documentElement.style
let keyboardInset = 0
let liftedDialog: HTMLElement | null = null
let liftedBy = 0

function isTextField(element: Element | null): element is HTMLElement {
  if (!(element instanceof HTMLElement)) return false
  if (element.isContentEditable) return true
  return element instanceof HTMLTextAreaElement || element instanceof HTMLInputElement
}

function scrollParent(element: HTMLElement, boundary: HTMLElement | null): HTMLElement | null {
  for (let node = element.parentElement; node && node !== boundary; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    if ((overflowY === 'auto' || overflowY === 'scroll') && node.scrollHeight > node.clientHeight) return node
  }
  return null
}

function dropDialog() {
  liftedDialog?.style.removeProperty('translate')
  liftedDialog = null
  liftedBy = 0
}

function liftDialog(overlay: HTMLElement, dialog: HTMLElement, overflow: number) {
  if (liftedDialog !== dialog) dropDialog()
  const limit = overlay.getBoundingClientRect().top + parseFloat(getComputedStyle(overlay).paddingTop)
  const room = Math.max(dialog.getBoundingClientRect().top - limit, 0)
  const lift = Math.min(overflow, room)
  if (lift <= 0) return
  liftedDialog = dialog
  liftedBy += lift
  dialog.style.translate = `0 ${-liftedBy}px`
}

function revealFocused() {
  if (!keyboardVisible.value) return
  const element = document.activeElement
  if (!isTextField(element)) return
  const overflow = element.getBoundingClientRect().bottom - (window.innerHeight - keyboardInset - REVEAL_MARGIN)
  if (overflow <= 0) return
  const overlay = element.closest<HTMLElement>('.safe-overlay')
  const scroller = scrollParent(element, overlay)
  if (scroller) {
    scroller.scrollBy({ top: overflow, behavior: 'smooth' })
    return
  }
  const dialog = overlay?.firstElementChild
  if (overlay && dialog instanceof HTMLElement) liftDialog(overlay, dialog, overflow)
}

function applyInset(inset: number) {
  keyboardInset = Number.isFinite(inset) && inset > 0 ? inset : 0
  keyboardVisible.value = keyboardInset > 0
  if (keyboardVisible.value) {
    rootStyle.setProperty('--keyboard-inset', `${keyboardInset}px`)
    requestAnimationFrame(() => requestAnimationFrame(revealFocused))
  } else {
    rootStyle.removeProperty('--keyboard-inset')
    dropDialog()
  }
}

window.addEventListener('singboard-keyboard', (event) => {
  applyInset(Number((event as CustomEvent).detail))
})
document.addEventListener('focusin', () => requestAnimationFrame(revealFocused))
