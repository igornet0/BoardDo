import type { ReactNode } from 'react'

/**
 * Small Markdown subset for AI output (headings, lists, tables, code, bold,
 * inline code, links). Renders React nodes only — no HTML injection.
 */
export function MiniMarkdown({ text, className }: { text: string; className?: string }) {
  return <div className={`md${className ? ` ${className}` : ''}`}>{renderBlocks(text)}</div>
}

function renderBlocks(text: string): ReactNode[] {
  const lines = text.replace(/\r\n/g, '\n').split('\n')
  const out: ReactNode[] = []
  let i = 0
  let key = 0

  while (i < lines.length) {
    const line = lines[i]

    if (/^\s*```/.test(line)) {
      const body: string[] = []
      i += 1
      while (i < lines.length && !/^\s*```/.test(lines[i])) {
        body.push(lines[i])
        i += 1
      }
      i += 1
      out.push(
        <pre key={key++} className="md__code">
          {body.join('\n')}
        </pre>,
      )
      continue
    }

    const heading = /^(#{1,6})\s+(.*)$/.exec(line)
    if (heading) {
      const level = heading[1].length
      const Tag = level <= 2 ? 'h4' : 'h5'
      out.push(
        <Tag key={key++} className="md__h">
          {inline(heading[2])}
        </Tag>,
      )
      i += 1
      continue
    }

    if (/^\s*\|.*\|\s*$/.test(line)) {
      const rows: string[][] = []
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) {
        const cells = lines[i].trim().slice(1, -1).split('|').map((c) => c.trim())
        if (!cells.every((c) => /^:?-{2,}:?$/.test(c))) rows.push(cells)
        i += 1
      }
      const [head, ...body] = rows
      out.push(
        <div key={key++} className="md__table-wrap">
          <table className="md__table">
            {head && (
              <thead>
                <tr>
                  {head.map((c, ci) => (
                    <th key={ci}>{inline(c)}</th>
                  ))}
                </tr>
              </thead>
            )}
            <tbody>
              {body.map((r, ri) => (
                <tr key={ri}>
                  {r.map((c, ci) => (
                    <td key={ci}>{inline(c)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      )
      continue
    }

    const bullet = /^\s*[-*•]\s+/
    const numbered = /^\s*\d+[.)]\s+/
    if (bullet.test(line) || numbered.test(line)) {
      const ordered = numbered.test(line)
      const pattern = ordered ? numbered : bullet
      const items: string[] = []
      while (i < lines.length && pattern.test(lines[i])) {
        items.push(lines[i].replace(pattern, ''))
        i += 1
      }
      const List = ordered ? 'ol' : 'ul'
      out.push(
        <List key={key++} className="md__list">
          {items.map((it, ii) => (
            <li key={ii}>{inline(it)}</li>
          ))}
        </List>,
      )
      continue
    }

    if (!line.trim()) {
      i += 1
      continue
    }

    const para: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() &&
      !/^(#{1,6})\s+/.test(lines[i]) &&
      !/^\s*```/.test(lines[i]) &&
      !bullet.test(lines[i]) &&
      !numbered.test(lines[i]) &&
      !/^\s*\|.*\|\s*$/.test(lines[i])
    ) {
      para.push(lines[i])
      i += 1
    }
    out.push(
      <p key={key++} className="md__p">
        {para.map((p, pi) => (
          <span key={pi}>
            {pi > 0 && <br />}
            {inline(p)}
          </span>
        ))}
      </p>,
    )
  }
  return out
}

const INLINE = /(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\(https?:\/\/[^)\s]+\)|https?:\/\/[^\s)<>\]]+)/g

function inline(text: string): ReactNode[] {
  const parts: ReactNode[] = []
  let last = 0
  let key = 0
  for (const m of text.matchAll(INLINE)) {
    const idx = m.index ?? 0
    if (idx > last) parts.push(text.slice(last, idx))
    const token = m[0]
    if (token.startsWith('**')) {
      parts.push(<strong key={key++}>{token.slice(2, -2)}</strong>)
    } else if (token.startsWith('`')) {
      parts.push(
        <code key={key++} className="md__code-inline">
          {token.slice(1, -1)}
        </code>,
      )
    } else if (token.startsWith('[')) {
      const link = /^\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)$/.exec(token)
      parts.push(
        link ? (
          <a key={key++} href={link[2]} target="_blank" rel="noreferrer noopener">
            {link[1]}
          </a>
        ) : (
          token
        ),
      )
    } else {
      parts.push(
        <a key={key++} href={token} target="_blank" rel="noreferrer noopener">
          {token}
        </a>,
      )
    }
    last = idx + token.length
  }
  if (last < text.length) parts.push(text.slice(last))
  return parts
}
