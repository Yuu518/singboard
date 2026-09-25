const DB_NAME = 'singboard'
const STORE_NAME = 'assets'
const BACKGROUND_KEY = 'background'

function openDatabase(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(DB_NAME, 1)
    request.onupgradeneeded = () => {
      if (!request.result.objectStoreNames.contains(STORE_NAME)) {
        request.result.createObjectStore(STORE_NAME)
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

async function withStore<T>(mode: IDBTransactionMode, run: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await openDatabase()
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = db.transaction(STORE_NAME, mode)
      const request = run(transaction.objectStore(STORE_NAME))
      transaction.oncomplete = () => resolve(request.result)
      transaction.onerror = () => reject(transaction.error)
      transaction.onabort = () => reject(transaction.error)
    })
  } finally {
    db.close()
  }
}

export async function loadBackground(): Promise<Blob | null> {
  const value = await withStore<unknown>('readonly', (store) => store.get(BACKGROUND_KEY))
  return value instanceof Blob ? value : null
}

export async function saveBackground(blob: Blob): Promise<void> {
  await withStore('readwrite', (store) => store.put(blob, BACKGROUND_KEY))
}

export async function deleteBackground(): Promise<void> {
  await withStore('readwrite', (store) => store.delete(BACKGROUND_KEY))
}
