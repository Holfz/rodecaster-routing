/**
 * Names the operator has typed in, kept on this machine only.
 *
 * The backend's label table is deliberately conservative: sources 10 and 20
 * have no label the evidence supports, so it reports them as `source 10`
 * and this app must not invent better. An alias the operator typed themselves
 * is not a guess about the protocol, so it lives here — separate from anything
 * read off the console, never sent to it, and the reported label stays in the
 * tooltip so the real identity is one hover away.
 *
 * Inputs are keyed by console source index and outputs by output column, both
 * of which are addresses rather than positions, so a rename survives a
 * reconnect and a change of visible columns.
 */
const KEY = 'rodecaster-deck.names'

interface Stored {
  inputs: Record<string, string>
  outputs: Record<string, string>
}

const inputs = reactive<Record<number, string>>({})
const outputs = reactive<Record<number, string>>({})
let loaded = false

function persist() {
  try {
    localStorage.setItem(KEY, JSON.stringify({ inputs, outputs } satisfies Stored))
  } catch {
    // Private browsing and locked-down profiles throw here; a lost alias is
    // not worth an error banner.
  }
}

function load() {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Stored>
    Object.assign(inputs, saved.inputs ?? {})
    Object.assign(outputs, saved.outputs ?? {})
  } catch {
    // Same.
  }
}

export function useLocalNames() {
  if (!loaded && import.meta.client) {
    load()
    loaded = true
  }

  const table = (kind: 'input' | 'output') => (kind === 'input' ? inputs : outputs)

  /** The name to show: the operator's alias if there is one, else the console's. */
  function display(kind: 'input' | 'output', key: number, reported: string): string {
    return table(kind)[key] || reported
  }

  /** An empty name, or one identical to the console's, clears the alias. */
  function rename(kind: 'input' | 'output', key: number, name: string, reported: string) {
    const value = name.trim()
    if (!value || value === reported) delete table(kind)[key]
    else table(kind)[key] = value
    persist()
  }

  const count = computed(() => Object.keys(inputs).length + Object.keys(outputs).length)

  function reset() {
    for (const k of Object.keys(inputs)) delete inputs[Number(k)]
    for (const k of Object.keys(outputs)) delete outputs[Number(k)]
    persist()
  }

  return { display, rename, reset, count }
}
