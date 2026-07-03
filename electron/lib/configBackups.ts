// Each ~/.ssh/config sync writes a timestamped `config.bak.<ts>`. Left unchecked
// these accumulate forever, so we keep only the newest `max`. The timestamp
// suffix is ISO-derived (`:`/`.` → `-`), which sorts chronologically as a string.

export function backupsToPrune(filenames: string[], max: number): string[] {
  const baks = filenames.filter((f) => f.startsWith('config.bak.')).sort()
  return baks.slice(0, Math.max(0, baks.length - max))
}
