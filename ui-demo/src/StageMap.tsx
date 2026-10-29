import { useState } from "react"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"

type Point = {
  name: string
  kind: "player" | "bank"
  color: string
  x: number
  y: number
  z: number
  source: string
}

export function StageMap({ tick }: { tick: number }) {
  const [projection, setProjection] = useState<"iso" | "top">("iso")
  const points: Point[] = [
    { name: "P1 · SERVER", kind: "player", color: "#38bdf8", x: -18 + Math.sin(tick / 5) * 8, y: 10.2, z: -12 + Math.cos(tick / 5) * 5, source: "simulated" },
    { name: "P2 · CLIENT", kind: "player", color: "#34d399", x: 19 + Math.cos(tick / 6) * 7, y: 11.4, z: 7 + Math.sin(tick / 6) * 9, source: "simulated" },
    { name: "P3", kind: "player", color: "#fbbf24", x: -4 + Math.sin(tick / 7) * 12, y: 8.5, z: 27, source: "simulated" },
    { name: "P4", kind: "player", color: "#f472b6", x: 24, y: 12.1, z: -25 + Math.cos(tick / 8) * 12, source: "simulated" },
    { name: "BANK · Cmn", kind: "bank", color: "#e5e5e5", x: 2.5, y: 9, z: 40, source: "stage asset candidate" },
  ]

  function project(point: Point): [number, number] {
    if (projection === "top") return [400 + point.x * 5, 235 - point.z * 3.3]
    return [400 + point.x * 4.1 + point.z * 2.1, 238 + point.x * 1.15 - point.z * 1.45 - point.y * 3.2]
  }

  return (
    <div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_280px]">
      <Card className="gap-0 rounded-md border shadow-none ring-0">
        <CardHeader className="flex flex-row items-center justify-between gap-3 border-b">
          <div>
            <CardTitle>Cop_Shakedent · position plot</CardTitle>
            <p className="mt-1 text-xs text-muted-foreground">模式地形です。実際のステージメッシュやプレイヤー座標は読み込んでいません。</p>
          </div>
          <div className="flex shrink-0 gap-1">
            <Button size="sm" variant={projection === "iso" ? "secondary" : "ghost"} onClick={() => setProjection("iso")}>3D schematic</Button>
            <Button size="sm" variant={projection === "top" ? "secondary" : "ghost"} onClick={() => setProjection("top")}>X–Z</Button>
          </div>
        </CardHeader>
        <CardContent className="p-3">
          <svg viewBox="0 0 800 470" className="aspect-[800/470] w-full rounded-sm border bg-[#111316]" role="img" aria-label="ダミーのプレイヤー位置を示す模式マップ">
            <defs>
              <pattern id="map-grid" width="40" height="40" patternUnits="userSpaceOnUse">
                <path d="M 40 0 L 0 0 0 40" fill="none" stroke="#30343a" strokeWidth="1" />
              </pattern>
            </defs>
            <rect x="0" y="0" width="800" height="470" fill="url(#map-grid)" />
            {projection === "iso" ? (
              <>
                <polygon points="171,241 286,128 564,151 665,285 570,383 264,365" fill="#25292d" stroke="#777b80" strokeWidth="2" />
                <polygon points="286,128 335,91 555,116 564,151" fill="#383d42" stroke="#838991" />
                <polygon points="171,241 286,128 286,171 171,283" fill="#1b1e21" stroke="#666c72" />
                <polygon points="564,151 665,285 665,327 564,194" fill="#1b1e21" stroke="#666c72" />
                <polygon points="267,238 370,181 471,208 391,274" fill="#343a3f" stroke="#8b939a" strokeWidth="2" />
                <polygon points="445,287 544,224 604,296 530,347" fill="#30363b" stroke="#8b939a" strokeWidth="2" />
                <path d="M 180 305 L 644 339 M 210 344 L 597 387" stroke="#454b51" strokeWidth="1" strokeDasharray="6 7" />
              </>
            ) : (
              <>
                <path d="M 194 87 L 593 87 L 689 188 L 620 392 L 240 400 L 122 296 Z" fill="#25292d" stroke="#777b80" strokeWidth="2" />
                <path d="M 270 148 L 483 122 L 569 224 L 454 305 L 258 275 Z" fill="#343a3f" stroke="#8b939a" strokeWidth="2" />
                <path d="M 154 320 L 646 175" stroke="#575e64" strokeDasharray="7 6" />
              </>
            )}
            <text x="22" y="29" fill="#a3a3a3" fontSize="12" fontFamily="monospace">SCHEMATIC • NOT GAME GEOMETRY</text>
            <text x="724" y="438" fill="#a3a3a3" fontSize="12" fontFamily="monospace">X →</text>
            <text x="21" y="443" fill="#a3a3a3" fontSize="12" fontFamily="monospace">Z ↑</text>
            {points.map((point) => {
              const [x, y] = project(point)
              return (
                <g key={point.name}>
                  {projection === "iso" && point.kind === "player" && <path d={`M ${x} ${y} L ${x} ${y + point.y * 2.4}`} stroke={point.color} strokeOpacity=".6" strokeDasharray="3 4" />}
                  <circle cx={x} cy={y} r={point.kind === "bank" ? 11 : 12} fill={point.color} stroke="#111316" strokeWidth="4" />
                  <text x={x + 16} y={y + 4} fill="#f5f5f5" stroke="#111316" strokeWidth="3" paintOrder="stroke" fontSize="12" fontFamily="monospace">{point.name}</text>
                </g>
              )
            })}
          </svg>
        </CardContent>
      </Card>
      <Card className="gap-0 rounded-md border shadow-none ring-0">
        <CardHeader className="border-b">
          <CardTitle>Tracked positions</CardTitle>
          <p className="text-xs text-muted-foreground">X / Y (height) / Z</p>
        </CardHeader>
        <CardContent className="divide-y p-0">
          {points.map((point) => (
            <div key={point.name} className="px-4 py-3">
              <div className="mb-1 flex items-center justify-between gap-2">
                <span className="flex items-center gap-2 text-xs font-medium"><i className="size-2 rounded-full" style={{ backgroundColor: point.color }} />{point.name}</span>
                <Badge variant="outline" className="rounded-sm text-[10px]">{point.source}</Badge>
              </div>
              <div className="font-mono text-xs text-muted-foreground">{point.x.toFixed(1)} / {point.y.toFixed(1)} / {point.z.toFixed(1)}</div>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  )
}
